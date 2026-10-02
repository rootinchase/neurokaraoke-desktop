use crate::api::API_URLS;
use crate::debug_log;
use anyhow::Result;
use dashmap::DashMap;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::fs::{File, metadata, read_dir, remove_file, rename}; // Added read_dir
use tokio::io::AsyncWriteExt;
use tokio::sync::{Notify, Semaphore};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum AssetType {
    Audio,
    Image,
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
        dir
    })
}

pub fn is_cache_fresh(path: &std::path::Path, max_age: Duration) -> bool {
    if let Ok(metadata) = std::fs::metadata(path) {
        if let Ok(modified) = metadata.modified() {
            return modified.elapsed().unwrap_or_default() < max_age;
        }
    }
    false
}

pub struct PersistentMediaCache {
    client: Client,
    network_gate: Arc<Semaphore>,
    in_flight: DashMap<(Uuid, AssetType), DownloadStatus>,
}

impl PersistentMediaCache {
    pub fn new(client: Client, max_concurrent_downloads: usize) -> Self {
        Self {
            client,
            network_gate: Arc::new(Semaphore::new(max_concurrent_downloads)),
            in_flight: DashMap::new(),
        }
    }

    /// Helper to get the canonical base assets directory path
    fn get_assets_dir() -> PathBuf {
        cache_dir().join("assets")
    }

    pub fn get_cached_path(&self, id: Uuid, asset_type: AssetType) -> Option<PathBuf> {
        let key = (id, asset_type);

        // 1. Check in-flight status for completed records
        if let Some(status) = self.in_flight.get(&key) {
            if let DownloadStatus::Complete(path) = status.value() {
                touch_file(path);
                return Some(path.clone());
            }
        }

        // 2. Fallback check for raw files existing on the operating system storage drive
        match asset_type {
            AssetType::Image => {
                for ext in &["webp", "jpeg", "jpg", "png", "gif", "bin"] {
                    let potential_path = Self::get_asset_path(id, Some(ext));
                    if std::fs::metadata(&potential_path).is_ok() {
                        touch_file(&potential_path);
                        return Some(potential_path);
                    }
                }
            }
            AssetType::Audio => {
                let potential_path = Self::get_asset_path(id, None);
                if std::fs::metadata(&potential_path).is_ok() {
                    touch_file(&potential_path);
                    return Some(potential_path);
                }
            }
        }

        None
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
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "tmp" {
                        let _ = remove_file(&path).await;
                    }
                }
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
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "tmp" {
                        let _ = remove_file(&path);
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn get_or_download_image(&self, cloudflare_id: Uuid, url: String) -> Result<PathBuf> {
        debug_log!(
            "🔍 [Cache] Requesting image: {} (URL: {})",
            cloudflare_id,
            url
        );
        let key = (cloudflare_id, AssetType::Image);

        loop {
            if let Some(status) = self.in_flight.get(&key) {
                match status.value() {
                    DownloadStatus::Complete(path) => {
                        debug_log!("✅ [Cache] Found in-flight Complete for: {}", cloudflare_id);
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
                    notify.notify_waiters();
                    Ok(saved_path)
                }
                Err(e) => {
                    debug_log!("❌ [Cache] Download failed for {}: {}", cloudflare_id, e);
                    self.in_flight.remove(&key);
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
        let response = self.client.get(&url).send().await?;
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
        let dir_path = Self::get_assets_dir();
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
                if !is_cache_fresh(&path, max_age) {
                    if remove_file(&path).await.is_ok() {
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
        }
        Ok(evicted_count)
    }

    pub async fn evict_to_fit_size(&self, max_size_bytes: u64) -> Result<usize> {
        let dir_path = Self::get_assets_dir();
        if !metadata(&dir_path).await.is_ok() {
            return Ok(0);
        }

        struct FileInfo {
            path: PathBuf,
            size: u64,
            modified: std::time::SystemTime,
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
                    let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
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
            files.sort_by(|a, b| a.modified.cmp(&b.modified));

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_eviction_async() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let dir = PersistentMediaCache::get_assets_dir();
            let _ = tokio::fs::create_dir_all(&dir).await;

            let file1 = dir.join("test_old.bin");
            let file2 = dir.join("test_new.bin");

            tokio::fs::write(&file1, b"old file content").await.unwrap();
            tokio::fs::write(&file2, b"new file content").await.unwrap();

            let old_time = std::time::SystemTime::now() - Duration::from_secs(7200);
            let _ = std::fs::File::options()
                .write(true)
                .open(&file1)
                .and_then(|f| f.set_modified(old_time));

            let client = Client::new();
            let cache = PersistentMediaCache::new(client, 4);

            let evicted = cache.evict(Duration::from_secs(3600), 1).await.unwrap();
            assert!(evicted >= 1);
            assert!(!file1.exists());

            let _ = tokio::fs::remove_dir_all(dir.parent().unwrap()).await;
        });
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

pub fn touch_file(path: &std::path::Path) {
    if let Ok(file) = std::fs::File::options().write(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}
