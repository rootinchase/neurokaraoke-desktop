use crate::api::API_URLS;
use crate::debug_log;
use anyhow::Result;
use dashmap::DashMap;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
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
        dirs::cache_dir()
            .expect("OS cache directory must exist")
            .join("neurokaraoke-desktop")
    })
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
        dirs::cache_dir()
            .expect("OS cache directory must exist")
            .join("neurokaraoke-desktop/assets")
    }

    pub fn get_cached_path(&self, id: Uuid, asset_type: AssetType) -> Option<PathBuf> {
        let key = (id, asset_type);

        // 1. Check in-flight status for completed records
        if let Some(status) = self.in_flight.get(&key) {
            if let DownloadStatus::Complete(path) = status.value() {
                return Some(path.clone());
            }
        }

        // 2. Fallback check for raw files existing on the operating system storage drive
        let ext = match asset_type {
            AssetType::Image => Some("webp"),
            AssetType::Audio => None,
        };
        let potential_path = Self::get_asset_path(id, ext);
        if std::fs::metadata(&potential_path).is_ok() {
            return Some(potential_path);
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

    pub async fn get_or_download_image(&self, cloudflare_id: Uuid, url: String) -> Result<PathBuf> {
        debug_log!("🔍 [Cache] Requesting image: {} (URL: {})", cloudflare_id, url);
        let key = (cloudflare_id, AssetType::Image);

        loop {
            if let Some(status) = self.in_flight.get(&key) {
                match status.value() {
                    DownloadStatus::Complete(path) => {
                        debug_log!("✅ [Cache] Found in-flight Complete for: {}", cloudflare_id);
                        return Ok(path.clone());
                    }
                    DownloadStatus::InFlight(notify) => {
                        debug_log!("⏳ [Cache] Waiting for in-flight download for: {}", cloudflare_id);
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

            debug_log!("🌐 [Cache] File not in cache/disk. Starting download for: {}", cloudflare_id);

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
            }
        }
    }

    async fn execute_download(&self, url: String, id: Uuid) -> Result<PathBuf> {
        let _permit = self.network_gate.acquire().await?;

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

        debug_log!("📦 [Cache] Downloaded bytes, content-type: {:?}", content_type);

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

        // 3. Clean up user-specific serialized indexes sitting inside the cache directory
        // FIXED: Binding the owned PathBuf to assets_dir keeps the underlying data alive long enough!
        let assets_dir = Self::get_assets_dir();
        let cache_base = assets_dir.parent().expect("Should have a parent directory");

        let files_to_purge = ["songs.ron", "playlists.ron", "my_playlists.ron"];
        for file_name in files_to_purge {
            let path = cache_base.join(file_name);
            if metadata(&path).await.is_ok() {
                let _ = remove_file(path).await;
            }
        }

        debug_log!(
            "🧹 [Cache System] Global asset storage and serialized playlist state permanently cleared."
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
            }
        }
    }

    pub async fn cache_pass(&self, _client: Client, _config: &crate::config::CacheConfig) {
        let _ = Self::cleanup_stale_downloads().await;
    }

    pub fn is_online(&self) -> bool {
        true
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
        if abs.starts_with("/WxURxyML82UkE7gY-PiBKw/") || abs.starts_with("WxURxyML82UkE7gY-PiBKw/") {
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
