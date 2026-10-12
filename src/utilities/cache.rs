use crate::api::API_URLS;
use crate::debug_log;
use anyhow::Result;
use dashmap::{DashMap, DashSet};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::sync::atomic::AtomicU64;
use std::time::{Duration, SystemTime};
use tokio::fs::{File, metadata, read_dir, remove_file, rename}; // Added read_dir
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, Semaphore};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum AssetType {
    Audio,
    Image,
    /// Full-resolution artwork (Cloudflare `public` variant), cached separately
    /// from the 512x512 thumbnail so the two never collide.
    FullImage,
}

#[derive(Clone)]
enum DownloadStatus {
    InFlight(Arc<Notify>),
    Complete(PathBuf),
}

static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn cache_dir() -> &'static PathBuf {
    CACHE_DIR.get_or_init(|| {
        let dir = dirs::cache_dir()
            .expect("OS cache directory must exist")
            .join("neurokaraoke-desktop");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::create_dir_all(dir.join("assets"));
        let _ = std::fs::create_dir_all(dir.join("playlists"));
        dir
    })
}

pub fn playlist_cache_dir() -> PathBuf {
    let p = cache_dir().join("playlists");
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn is_cache_fresh(path: &Path, max_age: Duration) -> bool {
    if let Ok(metadata) = std::fs::metadata(path)
        && let Ok(modified) = metadata.modified() {
            return modified.elapsed().unwrap_or_default() < max_age;
        }
    false
}

pub struct PersistentMediaCache {
    client: Client,
    network_gate: Arc<Semaphore>,
    in_flight: DashMap<(Uuid, AssetType), DownloadStatus>,
    /// Resolved on-disk path for each asset key, memoized after the first
    /// successful filesystem scan. Lets `get_cached_path` answer hits with a
    /// single map lookup instead of stat-ing up to 6 candidate filenames per
    /// frame.
    resolved: DashMap<(Uuid, AssetType), PathBuf>,
    /// Keys confirmed to be absent from disk. Memorizes negative results so a
    /// missing artwork is not re-stat-ed on every frame.
    negative: DashSet<(Uuid, AssetType)>,
    /// Access-order tracker for LRU eviction of `in_flight`: maps each asset key
    /// to the monotonic counter value of its last access. Bumped on every
    /// `get_cached_path` hit and `get_or_download_image` insert.
    in_flight_last_used: DashMap<(Uuid, AssetType), u64>,
    /// Monotonic access counter shared across all clones of the cache.
    in_flight_counter: Arc<AtomicU64>,
}

impl PersistentMediaCache {
    pub fn new(client: Client, max_concurrent_downloads: usize) -> Self {
        Self {
            client,
            network_gate: Arc::new(Semaphore::new(max_concurrent_downloads)),
            in_flight: DashMap::new(),
            resolved: DashMap::new(),
            negative: DashSet::new(),
            in_flight_last_used: DashMap::new(),
            in_flight_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Helper to get the canonical base assets directory path
    fn get_assets_dir() -> PathBuf {
        cache_dir().join("assets")
    }

    pub fn get_cached_path(&self, id: Uuid, asset_type: AssetType) -> Option<PathBuf> {
        let key = (id, asset_type);

        // 1. In-flight completed record.
        if let Some(status) = self.in_flight.get(&key)
            && let DownloadStatus::Complete(path) = status.value() {
                self.touch_in_flight(key);
                return Some(path.clone());
            }

        // 2. Resolved-path cache: a single map lookup, no filesystem stat.
        if let Some(path) = self.resolved.get(&key) {
            let path = path.value().clone();
            self.touch_in_flight(key);
            return Some(path);
        }

        // 3. Negative cache: we already know this asset is not on disk, so skip
        //    the 6-extension scan entirely on every frame.
        if self.negative.contains(&key) {
            return None;
        }

        // 4. First access for this key: scan candidate filenames once, then
        //    memoize the result. The disk mtime is flushed once here; subsequent
        //    hits rely on the in-memory LRU counter, and mtime is refreshed to
        //    disk only during eviction sweeps.
        let found = match asset_type {
            AssetType::Image | AssetType::FullImage => ["webp", "jpeg", "jpg", "png", "gif", "bin"]
                .iter()
                .map(|ext| Self::get_asset_path(id, Some(ext)))
                .find(|p| std::fs::metadata(p).is_ok()),
            AssetType::Audio => {
                let p = Self::get_asset_path(id, None);
                if std::fs::metadata(&p).is_ok() {
                    Some(p)
                } else {
                    None
                }
            }
        };

        match found {
            Some(path) => {
                self.resolved.insert(key, path.clone());
                self.touch_in_flight(key);
                touch_file(&path);
                Some(path)
            }
            None => {
                self.negative.insert(key);
                None
            }
        }
    }

    /// Global baseline path helper for Windows, Linux, and macOS
    fn get_asset_path(id: Uuid, extension: Option<&str>) -> PathBuf {
        let dir = Self::get_assets_dir();
        if let Some(ext) = extension {
            dir.join(format!("{}.{}", id, ext))
        } else {
            dir.join(id.to_string())
        }
    }

    pub fn remove_asset(&self, id: Uuid, asset_type: AssetType) {
        let key = (id, asset_type);
        self.in_flight.remove(&key);
        self.resolved.remove(&key);
        self.negative.remove(&key);
        for ext in &["webp", "jpeg", "jpg", "png", "gif", "bin", "tmp"] {
            let path = Self::get_asset_path(id, Some(ext));
            if path.exists() {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Number of entries currently held in the in-flight/complete asset map.
    /// Used for memory profiling: this map grows with every downloaded asset
    /// and is the artwork path cache that the LRU sweep must keep bounded.
    pub fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }

    /// Records a monotonic access timestamp for an asset key so LRU eviction
    /// can identify the least-recently-used entries.
    fn touch_in_flight(&self, key: (Uuid, AssetType)) {
        let t = self.in_flight_counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.in_flight_last_used.insert(key, t);
    }

    /// Evicts the in-memory `in_flight` map down to `max_entries`. First drops
    /// complete entries whose disk file has been removed, then, beyond the cap,
    /// drops the least-recently-used entries, including in-flight ones. When an
    /// in-flight entry is dropped its `Notify` is fired so tasks blocked on it
    /// wake up, loop, re-check disk, and re-download if needed — otherwise
    /// stalled downloads would park their waiters forever. The disk cache is
    /// the source of truth, so a dropped entry is re-resolved (and re-downloaded
    /// if needed) on the next access.
    pub fn evict_in_flight(&self, max_entries: usize) {
        // Drop complete entries whose disk file no longer exists.
        self.in_flight.retain(|key, status| {
            let keep = match status {
                DownloadStatus::Complete(path) => std::fs::metadata(path).is_ok(),
                DownloadStatus::InFlight(_) => true,
            };
            if !keep {
                self.in_flight_last_used.remove(key);
                self.resolved.remove(key);
                self.negative.remove(key);
            } else if let DownloadStatus::Complete(path) = status {
                // Flush the LRU mtime to disk once per sweep for kept entries,
                // so the hot `get_cached_path` path no longer needs to touch the
                // file on every frame.
                touch_file(path);
            }
            keep
        });

        if self.in_flight.len() <= max_entries {
            return;
        }
        let mut candidates: Vec<((Uuid, AssetType), u64)> = self
            .in_flight
            .iter()
            .map(|e| {
                let t = self
                    .in_flight_last_used
                    .get(e.key())
                    .map(|v| *v.value())
                    .unwrap_or(0);
                (*e.key(), t)
            })
            .collect();
        candidates.sort_by_key(|(_, t)| *t);
        let mut to_remove = self.in_flight.len() - max_entries;
        for (key, _) in candidates {
            if to_remove == 0 {
                break;
            }
            // Waking the notify of an evicted in-flight entry releases every
            // task blocked on it; they loop, see the entry is gone, and
            // re-check disk / re-download on the next access.
            if let Some((_, status)) = self.in_flight.remove(&key)
                && let DownloadStatus::InFlight(notify) = status
            {
                notify.notify_waiters();
            }
            self.in_flight_last_used.remove(&key);
            self.resolved.remove(&key);
            self.negative.remove(&key);
            to_remove -= 1;
        }
    }

    /// Returns `true` when a download for this asset is currently in flight.
    /// UI renderers call this to avoid spawning a duplicate download task on
    /// every frame for artwork that is already being fetched (spawning per
    /// frame for stalled/missing art accumulates blocked tasks unboundedly).
    pub fn has_in_flight(&self, id: Uuid, asset_type: AssetType) -> bool {
        self.in_flight
            .get(&(id, asset_type))
            .map(|s| matches!(s.value(), DownloadStatus::InFlight(_)))
            .unwrap_or(false)
    }

    /// Scans the cache assets directory, ensures it exists, and purges all incomplete `.tmp` files.
    pub async fn cleanup_stale_downloads() -> Result<()> {
        let dir_path = Self::get_assets_dir();

        // Ensure the folder architecture exists. If missing, build it out.
        if !metadata(&dir_path).await.is_ok() {
            if let Err(e) = tokio::fs::create_dir_all(&dir_path).await {
                return Err(anyhow::anyhow!(
                    "Failed to build baseline cache tree: {}",
                    e
                ));
            }
            return Ok(()); // Brand new folder, no .tmp files can possibly be here yet
        }

        let mut entries = read_dir(&dir_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file()
                && let Some(ext) = path.extension()
                    && ext == "tmp" {
                        let _ = remove_file(&path).await;
                    }
        }
        Ok(())
    }

    pub fn cleanup_stale_downloads_sync() -> Result<()> {
        let dir_path = Self::get_assets_dir();
        if !dir_path.exists() {
            std::fs::create_dir_all(&dir_path)?;
            return Ok(());
        }
        for entry in std::fs::read_dir(&dir_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file()
                && let Some(ext) = path.extension()
                    && ext == "tmp" {
                        drop(remove_file(&path))
                    }
        }
        Ok(())
    }

    pub async fn get_or_download_image(
        &self,
        cloudflare_id: Uuid,
        url: String,
        asset_type: AssetType,
    ) -> Result<PathBuf> {
        debug_log!(
            "🔍 [Cache] Requesting image: {} (URL: {})",
            cloudflare_id,
            url
        );
        let key = (cloudflare_id, asset_type);

        loop {
            if let Some(status) = self.in_flight.get(&key) {
                match status.value() {
                    DownloadStatus::Complete(path) => {
                        debug_log!("✅ [Cache] Found in-flight Complete for: {}", cloudflare_id);
                        self.touch_in_flight(key);
                        return Ok(path.clone());
                    }
                    DownloadStatus::InFlight(notify) => {
                        debug_log!(
                            "⏳ [Cache] Waiting for in-flight download for: {}",
                            cloudflare_id
                        );
                        let notify_clone = notify.clone();
                        drop(status);
                        notify_clone.notified().await;
                        continue;
                    }
                }
            }

            // Fallback: Check if the file is already safely persisted on disk
            let potential_path = Self::get_asset_path(cloudflare_id, Some("webp"));
            if metadata(&potential_path).await.is_ok() {
                debug_log!("💿 [Cache] Found persisted file for: {}", cloudflare_id);
                self.in_flight
                    .insert(key, DownloadStatus::Complete(potential_path.clone()));
                self.resolved.insert(key, potential_path.clone());
                self.negative.remove(&key);
                self.touch_in_flight(key);
                return Ok(potential_path);
            }

            debug_log!(
                "🌐 [Cache] File not in cache/disk. Starting download for: {}",
                cloudflare_id
            );

            let notify = Arc::new(Notify::new());
            match self.in_flight.entry(key) {
                dashmap::mapref::entry::Entry::Occupied(_) => {
                    continue;
                }
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    entry.insert(DownloadStatus::InFlight(notify.clone()));
                }
            }

            let result = self.execute_download(url.clone(), cloudflare_id).await;

            return match result {
                Ok(saved_path) => {
                    debug_log!("🎉 [Cache] Successfully downloaded: {}", cloudflare_id);
                    self.in_flight
                        .insert(key, DownloadStatus::Complete(saved_path.clone()));
                    self.resolved.insert(key, saved_path.clone());
                    self.negative.remove(&key);
                    self.touch_in_flight(key);
                    notify.notify_waiters();
                    Ok(saved_path)
                }
                Err(e) => {
                    debug_log!("❌ [Cache] Download failed for {}: {}", cloudflare_id, e);
                    self.in_flight.remove(&key);
                    self.in_flight_last_used.remove(&key);
                    self.resolved.remove(&key);
                    // The asset is confirmed absent from disk; memoize the miss so
                    // `get_cached_path` stops re-stat-ing it every frame.
                    self.negative.insert(key);
                    notify.notify_waiters();
                    Err(e)
                }
            };
        }
    }

    async fn execute_download(&self, url: String, id: Uuid) -> Result<PathBuf> {
        let _permit = self.network_gate.acquire().await?;

        let dir_path = Self::get_assets_dir();
        if let Err(e) = tokio::fs::create_dir_all(&dir_path).await {
            return Err(anyhow::anyhow!("Failed to create assets directory: {}", e));
        }

        debug_log!("🚀 [Cache] Fetching from network: {}", url);
        // Per-request timeout: image payloads are small, so a stalled
        // response must fail fast and release the in-flight entry plus its
        // blocked waiters instead of hanging the download task forever.
        let response = self.client.get(&url).timeout(Duration::from_secs(30)).send().await?;
        let status = response.status();
        if !status.is_success() {
            debug_log!("⚠️ [Cache] Network returned status: {}", status);
            return Err(anyhow::anyhow!("HTTP Download error: {}", status));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        debug_log!(
            "📦 [Cache] Downloaded bytes, content-type: {:?}",
            content_type
        );

        let body_bytes = response.bytes().await?;

        let extension = match content_type.as_deref() {
            Some("image/jpeg") | Some("image/jpg") => "jpeg",
            Some("image/png") => "png",
            Some("image/gif") => "gif",
            Some("image/webp") => "webp",
            _ => "bin",
        };

        let final_path = Self::get_asset_path(id, Some(extension));
        let tmp_path = final_path.with_extension("tmp");

        if tokio::fs::metadata(&tmp_path).await.is_ok() {
            let _ = remove_file(&tmp_path).await;
        }

        let write_result = async {
            let mut file = File::create(&tmp_path).await?;
            file.write_all(&body_bytes).await?;
            file.flush().await?;
            Ok::<(), std::io::Error>(())
        }
        .await;

        if let Err(e) = write_result {
            let _ = remove_file(&tmp_path).await;
            return Err(anyhow::anyhow!("Failed writing cache payload: {}", e));
        }

        if let Err(e) = rename(&tmp_path, &final_path).await {
            let _ = remove_file(&tmp_path).await;
            return Err(anyhow::anyhow!(
                "Failed promoting temporary cache file: {}",
                e
            ));
        }

        Ok(final_path)
    }

    pub async fn clear_cache(&self) -> Result<()> {
        let dir_path = Self::get_assets_dir();

        // 1. Wipe out everything in the in-memory stampede protection tracker
        self.in_flight.clear();
        self.resolved.clear();
        self.negative.clear();
        self.in_flight_last_used.clear();

        // 2. Iterate and delete all asset files from the filesystem
        if metadata(&dir_path).await.is_ok() {
            let mut entries = read_dir(&dir_path).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() {
                    let _ = remove_file(path).await;
                }
            }
        }

        // 3. Clean up all cached files and .ron files sitting inside the cache directory
        let assets_dir = Self::get_assets_dir();
        let cache_base = assets_dir.parent().expect("Should have a parent directory");

        if metadata(&cache_base).await.is_ok() {
            let mut entries = read_dir(&cache_base).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.is_file() {
                    let _ = remove_file(&path).await;
                }
            }
        }

        debug_log!(
            "🧹 [Cache System] Global asset storage and all cached state files permanently cleared."
        );
        Ok(())
    }

    pub async fn get_or_download_audio(
        &self,
        client: &Client,
        song_uuid: Uuid,
        url: String,
    ) -> Result<File> {
        let key = (song_uuid, AssetType::Audio);

        loop {
            if let Some(status) = self.in_flight.get(&key) {
                match status.value() {
                    DownloadStatus::Complete(path) => {
                        return Ok(File::open(path).await?);
                    }
                    DownloadStatus::InFlight(notify) => {
                        let notify_clone = notify.clone();
                        drop(status); // Release DashMap read lock before waiting
                        notify_clone.notified().await;
                        continue; // Loop back and check file descriptors
                    }
                }
            }

            let final_path = Self::get_asset_path(song_uuid, None);
            if metadata(&final_path).await.is_ok() {
                self.in_flight
                    .insert(key, DownloadStatus::Complete(final_path.clone()));
                return Ok(File::open(&final_path).await?);
            }

            let notify = Arc::new(Notify::new());
            match self.in_flight.entry(key) {
                dashmap::mapref::entry::Entry::Occupied(_) => continue, // Squeezed out by a race condition, try again
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    entry.insert(DownloadStatus::InFlight(notify.clone()));
                }
            }

            // STEP 4: Execute Atomic Byte Stream to local disk
            // Limit concurrent network operations through your semaphore gate
            let download_result = async {
                let _permit = self.network_gate.acquire().await?;

                let dir_path = Self::get_assets_dir();
                if let Err(e) = tokio::fs::create_dir_all(&dir_path).await {
                    return Err(anyhow::anyhow!("Failed to create assets directory: {}", e));
                }

                let response = client.get(&url).send().await?;
                let status = response.status();
                if !status.is_success() {
                    return Err(anyhow::anyhow!("HTTP Stream download error: {}", status));
                }

                let tmp_path = final_path.with_extension("tmp");
                if metadata(&tmp_path).await.is_ok() {
                    let _ = remove_file(&tmp_path).await;
                }

                // Chunked body streaming to avoid massive runtime RAM spikes
                let mut bytes_stream = response.bytes_stream();
                {
                    use futures_util::StreamExt; // ensure you have futures or futures-util in Cargo.toml
                    let mut file = File::create(&tmp_path).await?;
                    while let Some(chunk_result) = bytes_stream.next().await {
                        let chunk = chunk_result?;
                        file.write_all(&chunk).await?;
                    }
                    file.flush().await?;
                }

                // Promote temp tracking file atomically inside the operating system
                if let Err(e) = rename(&tmp_path, &final_path).await {
                    let _ = remove_file(&tmp_path).await;
                    return Err(anyhow::anyhow!(
                        "Failed promoting audio temporary file: {}",
                        e
                    ));
                }

                Ok(final_path.clone())
            }
            .await;

            // STEP 5: Register outcomes and alert waiting threads
            return match download_result {
                Ok(saved_path) => {
                    self.in_flight
                        .insert(key, DownloadStatus::Complete(saved_path.clone()));
                    notify.notify_waiters();
                    Ok(File::open(&saved_path).await?)
                }
                Err(e) => {
                    self.in_flight.remove(&key);
                    notify.notify_waiters();
                    Err(e)
                }
            };
        }
    }

    pub async fn evict_expired(&self, max_age: Duration) -> Result<usize> {
        self.evict_expired_in(&Self::get_assets_dir(), max_age)
            .await
    }

    pub async fn evict_expired_in(&self, dir_path: &Path, max_age: Duration) -> Result<usize> {
        if !metadata(&dir_path).await.is_ok() {
            return Ok(0);
        }

        let mut evicted_count = 0;
        let mut entries = read_dir(&dir_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if path.extension().and_then(|s| s.to_str()) == Some("tmp") {
                    continue;
                }
                if !is_cache_fresh(&path, max_age)
                    && remove_file(&path).await.is_ok() {
                        evicted_count += 1;
                        self.in_flight.retain(|_, status| {
                            if let DownloadStatus::Complete(p) = status {
                                p != &path
                            } else {
                                true
                            }
                        });
                    }
            }
        }
        Ok(evicted_count)
    }

    pub async fn evict_to_fit_size(&self, max_size_bytes: u64) -> Result<usize> {
        self.evict_to_fit_size_in(&Self::get_assets_dir(), max_size_bytes)
            .await
    }

    pub async fn evict_to_fit_size_in(
        &self,
        dir_path: &Path,
        max_size_bytes: u64,
    ) -> Result<usize> {
        if !metadata(&dir_path).await.is_ok() {
            return Ok(0);
        }

        struct FileInfo {
            path: PathBuf,
            size: u64,
            modified: SystemTime,
        }

        let mut files = Vec::new();
        let mut total_size: u64 = 0;

        let mut entries = read_dir(&dir_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() {
                if path.extension().and_then(|s| s.to_str()) == Some("tmp") {
                    continue;
                }
                if let Ok(meta) = metadata(&path).await {
                    let size = meta.len();
                    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    total_size += size;
                    files.push(FileInfo {
                        path,
                        size,
                        modified,
                    });
                }
            }
        }

        let mut evicted_count = 0;
        if total_size > max_size_bytes {
            files.sort_by_key(|a| a.modified);

            for file in files {
                if total_size <= max_size_bytes {
                    break;
                }
                if remove_file(&file.path).await.is_ok() {
                    total_size = total_size.saturating_sub(file.size);
                    evicted_count += 1;
                    self.in_flight.retain(|_, status| {
                        if let DownloadStatus::Complete(p) = status {
                            p != &file.path
                        } else {
                            true
                        }
                    });
                }
            }
        }

        Ok(evicted_count)
    }

    pub async fn evict(&self, max_age: Duration, max_size_bytes: u64) -> Result<usize> {
        let _ = Self::cleanup_stale_downloads().await;
        let expired_count = self.evict_expired(max_age).await?;
        let size_count = self.evict_to_fit_size(max_size_bytes).await?;
        Ok(expired_count + size_count)
    }

    #[allow(dead_code)]
    pub async fn evict_in(
        &self,
        dir_path: &Path,
        max_age: Duration,
        max_size_bytes: u64,
    ) -> Result<usize> {
        let expired_count = self.evict_expired_in(dir_path, max_age).await?;
        let size_count = self.evict_to_fit_size_in(dir_path, max_size_bytes).await?;
        Ok(expired_count + size_count)
    }
}

pub fn get_thumbnail_url(cloudflare_id: Option<&str>, absolute_path: &str, fit: &str) -> String {
    let clean_cf = cloudflare_id
        .filter(|s| !s.is_empty() && *s != "null")
        .map(|s| s.trim());
    let clean_abs = if absolute_path.is_empty() || absolute_path == "null" {
        None
    } else {
        Some(absolute_path.trim())
    };

    if let Some(abs) = clean_abs
        && (abs.starts_with("http://") || abs.starts_with("https://")) {
        return abs.to_string();
    }

    if let Some(id) = clean_cf {
        format!(
            "{}/{}/{}/width=512,height=512,fit={}",
            API_URLS.images, API_URLS.account_hash, id, fit
        )
    } else if let Some(abs) = clean_abs {
        if abs.starts_with("/WxURxyML82UkE7gY-PiBKw/") || abs.starts_with("WxURxyML82UkE7gY-PiBKw/")
        {
            let path_clean = abs.trim_start_matches('/');
            format!(
                "{}/{}/width=512,height=512,fit={}",
                API_URLS.images,
                path_clean.replace("/public", "").replace("/thumbnail", ""),
                fit
            )
        } else if abs.starts_with("http://") || abs.starts_with("https://") {
            abs.to_string()
        } else {
            format!(
                "{}/{}/width=512,height=512,fit={}",
                API_URLS.storage,
                abs.trim_start_matches('/'),
                fit
            )
        }
    } else {
        "".to_string()
    }
}

/// Builds the full-resolution Cloudflare Images delivery URL (the `public` variant,
/// no width/height constraints). Mirrors `get_thumbnail_url`'s branch structure.
pub fn get_full_image_url(cloudflare_id: Option<&str>, absolute_path: &str) -> String {
    let clean_cf = cloudflare_id
        .filter(|s| !s.is_empty() && *s != "null")
        .map(|s| s.trim());
    let clean_abs = if absolute_path.is_empty() || absolute_path == "null" {
        None
    } else {
        Some(absolute_path.trim())
    };

    if let Some(abs) = clean_abs
        && (abs.starts_with("http://") || abs.starts_with("https://"))
    {
        return abs.to_string();
    }

    if let Some(id) = clean_cf {
        format!(
            "{}/{}/{}{}",
            API_URLS.images, API_URLS.account_hash, id, "/public"
        )
    } else if let Some(abs) = clean_abs {
        if abs.starts_with("/WxURxyML82UkE7gY-PiBKw/") || abs.starts_with("WxURxyML82UkE7gY-PiBKw/")
        {
            let path_clean = abs.trim_start_matches('/');
            format!(
                "{}/{}/public",
                API_URLS.images,
                path_clean.replace("/public", "").replace("/thumbnail", "")
            )
        } else if abs.starts_with("http://") || abs.starts_with("https://") {
            abs.to_string()
        } else {
            format!(
                "{}/{}/public",
                API_URLS.storage,
                abs.trim_start_matches('/')
            )
        }
    } else {
        "".to_string()
    }
}

pub fn touch_file(path: &Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utilities::util::create_reqwest_client;
    use std::env::temp_dir;
    use std::fs::File;
    use std::time::SystemTime;
    use tokio::fs::{create_dir_all, write};
    use tokio::runtime::Runtime;

    #[test]
    fn test_cache_eviction_async() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = temp_dir().join(format!("neurokaraoke_test_cache_{}", Uuid::new_v4()));
            let dir = temp_dir.join("assets");
            let _ = create_dir_all(&dir).await;

            let file1 = dir.join("test_old.bin");
            let file2 = dir.join("test_new.bin");

            write(&file1, b"old file content").await.unwrap();
            write(&file2, b"new file content").await.unwrap();

            let old_time = SystemTime::now() - Duration::from_secs(7200);
            let _ = File::options()
                .write(true)
                .open(&file1)
                .and_then(|f| f.set_modified(old_time));

            let client = create_reqwest_client();
            let cache = PersistentMediaCache::new(client, 4);

            let evicted = cache
                .evict_in(&dir, Duration::from_secs(3600), 1)
                .await
                .unwrap();
            assert!(evicted >= 1);
            assert!(!file1.exists());

            // Clean up isolated test temp directory
            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        });
    }

    #[test]
    fn test_evict_in_flight_respects_lru() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir =
                temp_dir().join(format!("neurokaraoke_test_inflight_{}", Uuid::new_v4()));
            let dir = temp_dir.join("assets");
            let _ = create_dir_all(&dir).await;

            let client = create_reqwest_client();
            let cache = PersistentMediaCache::new(client, 4);

            // Insert three complete entries, each backed by a real disk file,
            // in access order: id1 first, id2 second, id3 last.
            let ids: Vec<Uuid> = vec![Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
            for id in &ids {
                let path = dir.join(format!("{}.webp", id));
                write(&path, b"art bytes").await.unwrap();
                cache
                    .in_flight
                    .insert((*id, AssetType::Image), DownloadStatus::Complete(path.clone()));
                cache.touch_in_flight((*id, AssetType::Image));
            }
            assert_eq!(cache.in_flight_len(), 3);

            // Evict down to 2: the least-recently-used (id1) must be dropped.
            cache.evict_in_flight(2);
            assert_eq!(cache.in_flight_len(), 2);
            assert!(!cache.in_flight.contains_key(&(ids[0], AssetType::Image)));
            assert!(cache.in_flight.contains_key(&(ids[1], AssetType::Image)));
            assert!(cache.in_flight.contains_key(&(ids[2], AssetType::Image)));

            // Evicting under the cap is a no-op.
            cache.evict_in_flight(10);
            assert_eq!(cache.in_flight_len(), 2);

            // Entries whose disk file is gone are dropped even under the cap.
            let _ = remove_file(dir.join(format!("{}.webp", ids[2]))).await;
            cache.evict_in_flight(10);
            assert_eq!(cache.in_flight_len(), 1);

            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        });
    }

    #[test]
    fn test_evict_in_flight_wakes_blocked_waiters() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let client = create_reqwest_client();
            let cache = PersistentMediaCache::new(client, 4);

            // A stalled download parks its entry as InFlight with a Notify
            // that its waiters block on.
            let id = Uuid::new_v4();
            let key = (id, AssetType::Image);
            let notify = Arc::new(Notify::new());
            cache.in_flight.insert(key, DownloadStatus::InFlight(notify.clone()));
            cache.touch_in_flight(key);

            // A waiter blocks on the Notify, mimicking `get_or_download_image`
            // callers parked on a stalled download.
            let notify_waiter = notify.clone();
            let waiter = tokio::spawn(async move {
                tokio::time::timeout(Duration::from_secs(2), notify_waiter.notified())
                    .await
                    .is_ok()
            });

            // `notify_waiters()` only wakes already-registered waiters, so let
            // the waiter task reach `notified()` before evicting.
            tokio::time::sleep(Duration::from_millis(50)).await;

            // Evicting past the cap must drop the in-flight entry AND wake its
            // waiters, so the blocked task is released instead of parked forever.
            cache.evict_in_flight(0);
            assert_eq!(cache.in_flight_len(), 0);
            assert!(!cache.in_flight_last_used.contains_key(&key));
            assert!(waiter.await.unwrap());
        });
    }

    #[test]
    fn test_has_in_flight_detects_inflight() {
        let rt = Runtime::new().unwrap();
        rt.block_on(async {
            let client = create_reqwest_client();
            let cache = PersistentMediaCache::new(client, 4);
            let id = Uuid::new_v4();
            let key = (id, AssetType::Image);

            // No entry -> not in flight.
            assert!(!cache.has_in_flight(id, AssetType::Image));

            // InFlight entry -> in flight, so renderers skip duplicate spawns.
            cache
                .in_flight
                .insert(key, DownloadStatus::InFlight(Arc::new(Notify::new())));
            assert!(cache.has_in_flight(id, AssetType::Image));

            // Complete entry -> not in flight; a re-download may be spawned.
            cache.in_flight.insert(
                key,
                DownloadStatus::Complete(PathBuf::from("/tmp/x.webp")),
            );
            assert!(!cache.has_in_flight(id, AssetType::Image));

            // Other asset types are tracked independently.
            assert!(!cache.has_in_flight(id, AssetType::FullImage));
        });
    }
}
