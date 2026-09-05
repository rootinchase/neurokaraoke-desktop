use crate::config::CacheConfig;
use dashmap::DashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_with::__private__::DeserializeOwned;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn init_cache_dir() {
    let dir = dirs::cache_dir().expect("cache directory should exist").join("neurokaraoke-desktop");
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    CACHE_DIR.set(dir).expect("CACHE_DIR already initialized");
}

pub fn cache_dir() -> &'static PathBuf {
    CACHE_DIR.get().expect("CACHE_DIR not initialized")
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetType {
    Audio,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub id: u64,
    /// if 0, this item never expires
    pub last_touched: u64,
    pub size: u64,
    #[serde(default)]
    pub extension: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    #[serde(skip)]
    pub online: AtomicBool,
    pub next_id: AtomicU64,
    pub entries: DashMap<(Uuid, AssetType), CacheEntry>,
}

impl Cache {
    const CACHE_FILE_NAME: &'static str = "cache.ron";

    pub fn load_or_default_custom<T: DeserializeOwned + Default>(filename: &str) -> T {
        let path = cache_dir().join(filename);
        crate::debug_log!("Loading cache from: {:?}", path);
        let data = match std::fs::read(&path) {
            Ok(d) => Some(d),
            Err(e) => {
                crate::debug_log!("cache load failed: {}", e);
                None
            },
        };

        if let Some(data) = data {
            let res = ron::de::from_bytes(data.as_slice());
            match res {
                Ok(val) => {
                    crate::debug_log!("Cache loaded successfully");
                    val
                },
                Err(e) => {
                    crate::debug_log!("cache deserialization failed: {}", e);
                    T::default()
                }
            }
        } else {
            T::default()
        }
    }

    pub fn load_or_default() -> Arc<Self> {
        Self::load_or_default_custom(Self::CACHE_FILE_NAME)
    }

    pub async fn cache_pass(self: &Arc<Self>, client: Client, config: &CacheConfig) {
        // don't clean cache if you are offline as you won't be able to re-download what gets deleted
        if async { client.get("https://api.neurokaraoke.com/healthz").timeout(Duration::from_secs(5)).send().await?.text().await }.await.map(|s| s == "Healthy").unwrap_or(false) {
            self.online.store(true, Ordering::Release);

            let mut total = 0u64;
            let expires = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("Time went backwards (why is your clock before 12AM UTC January 1st 1970?)")
                .as_secs() - config.cache_expiration_secs;
            let mut removed = Vec::new();
            let mut keys = Vec::new();
            self.entries.retain(|key, entry| {
                keys.push((*key, (entry.last_touched, entry.size)));
                total += entry.size;

                if entry.last_touched == 0 || entry.last_touched >= expires { true } else {
                    removed.push(entry.id);
                    false
                }
            });
            for id in removed {
                let path = cache_dir().join(format!("assets/{:016x}", id));
                let _ = tokio::fs::remove_file(path).await;
            }

            let max = config.cache_size_limit_mb * 1024 * 1024;
            if total > max {
                keys.sort_by_key(|x| x.1.0);
                let mut iter = keys.iter();
                while total > max {
                    let value = iter.next().expect("how did we run out of items to grab if the total size > 0?");
                    if value.1.0 == 0 { continue; }
                    total -= value.1.1;

                    let id = self.entries.remove(&value.0)
                        .unwrap()
                        .1.id;
                    let path = cache_dir().join(format!("assets/{:016x}", id));
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
        } else { self.online.store(false, Ordering::Release); }
        tokio::fs::write(&cache_dir().join(Self::CACHE_FILE_NAME), ron::ser::to_string_pretty(&self, Default::default()).unwrap()).await.unwrap();
    }

    pub fn create_worker<F: Future + Send + Sync + 'static>(self: Arc<Self>, mut extra_pass: impl FnMut() -> F + Send + 'static, rt: tokio::runtime::Handle, client: Client, config: Arc<Mutex<CacheConfig>>) {
        rt.clone().spawn(async move {
            loop {
                let config = { config.lock().await.clone() };
                self.cache_pass(client.clone(), &config).await;
                extra_pass().await;
                tokio::time::sleep(Duration::from_secs(config.cache_sweep_interval_secs)).await;
            }
        });
    }

    pub async fn get(&self, key: &(Uuid, AssetType)) -> Option<tokio::fs::File> {
        match match self.entries.get_mut(key) {
            Some(mut entry) => {
                if entry.last_touched != 0 {
                    entry.last_touched = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("Time went backwards (why is your clock before 12AM UTC January 1st 1970?)")
                        .as_secs();
                }
                Some(entry.id)
            },
            None => None,
        } {
            Some(id) => {
                let path = cache_dir().join(format!("assets/{:016x}", id));
                match tokio::fs::File::open(&path).await {
                    Ok(f) => Some(f),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        self.entries.remove(key);
                        None
                    },
                    Err(e) => panic!("Failed to read this cache file: {}\n{e}", path.display()),
                }
            },
            None => None,
        }
    }

    pub async fn get_or_download_audio(
        &self,
        client: &Client,
        song_uuid: Uuid,
        url: String,
    ) -> anyhow::Result<tokio::fs::File> {
        let key = (song_uuid, AssetType::Audio);

        // 1. Try to fetch from existing cache entries
        if let Some(file) = self.get(&key).await {
            crate::debug_log!("⚡ [Cache API] CACHE HIT: Playing local copy for UUID: {}", song_uuid);
            return Ok(file);
        }

        crate::debug_log!("🌍 [Cache API] CACHE MISS: Downloading from remote endpoint: {}", url);

        // 2. Cache miss: download the byte payload safely
        let response = client.get(url).send().await?;
        if !response.status().is_success() {
            return Err(anyhow::anyhow!("Request failed with status: {}", response.status()));
        }
        let body_bytes = response.bytes().await?;
        let size = body_bytes.len() as u64;

        // 3. Reserve a clean file increment slot
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let path = cache_dir().join(format!("assets/{:016x}", id));

        // 4. Flush raw binary payload to asset index
        let mut file = tokio::fs::File::create(&path).await?;
        file.write_all(&body_bytes).await?;
        file.flush().await?;
        drop(file);

        // 5. Track inside DashMap structure matching your background sweeps
        self.entries.insert(key, CacheEntry {
            id,
            last_touched: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            size,
            extension: None,
        });

        Ok(tokio::fs::File::open(&path).await?)
    }


    pub async fn get_or_download_image(
        &self,
        client: &Client,
        cloudflare_id: Uuid,
        url: String,
    ) -> anyhow::Result<PathBuf> {
        let key = (cloudflare_id, AssetType::Image);

        // 1. Check if already tracked in the cache index
        if let Some(entry) = self.entries.get(&key) {
            let path = if let Some(ref ext) = entry.extension {
                cache_dir().join(format!("assets/{:016x}.{}", entry.id, ext))
            } else {
                cache_dir().join(format!("assets/{:016x}", entry.id))
            };
            if tokio::fs::metadata(&path).await.is_ok() {
                return Ok(path);
            }
        }

        let response = client.get(&url).send().await?;
        let status = response.status();
        let content_type = response.headers().get("content-type")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string()); // Capture as owned String
        
        let body_bytes = response.bytes().await?;
        
        if content_type.as_deref().map_or(true, |ct| !ct.starts_with("image/")) {
             let body_text = String::from_utf8_lossy(&body_bytes);
             crate::debug_log!("🔴 [Cache Core HTTP] UNEXPECTED CONTENT-TYPE. Status: {}, Content-Type: {:?}, Body: {}", status, content_type, body_text);
        }

        let extension = match content_type.as_deref() {
            Some("image/jpeg") | Some("image/jpg") => Some("jpeg".to_string()),
            Some("image/png") => Some("png".to_string()),
            Some("image/webp") => Some("webp".to_string()),
            _ => None,
        };
        let size = body_bytes.len() as u64;

        crate::debug_log!("💾 [Cache Core HTTP] FLUSHING ASSET DATA. Size: {} bytes, Extension: {:?}", size, extension);

        // 3. Reserve a clean file slot
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let path = if let Some(ref ext) = extension {
            cache_dir().join(format!("assets/{:016x}.{}", id, ext))
        } else {
            cache_dir().join(format!("assets/{:016x}", id))
        };

        // 4. Flush image binary data to the asset index
        let mut file = tokio::fs::File::create(&path).await?;
        file.write_all(&body_bytes).await?;
        file.flush().await?;

        // 5. Track inside the DashMap structure for background cleanup compatibility
        self.entries.insert(key, CacheEntry {
            id,
            last_touched: SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            size,
            extension,
        });

        Ok(path)
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }
}