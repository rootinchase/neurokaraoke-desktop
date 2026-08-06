use crate::config::CacheConfig;
use dashmap::DashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_with::__private__::DeserializeOwned;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
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
    /// Standard audio formats (eg. MP3, Wav, Vorbis)
    Audio,
    /// Opus Ogg
    AudioOpus,
    /// Standard image formats
    Image,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CacheEntry {
    pub id: u64,
    /// if 0, this item never expires
    pub last_touched: u64,
    pub size: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    pub next_id: AtomicU64,
    pub entries: DashMap<(Uuid, AssetType), CacheEntry>,
}

impl Cache {
    const CACHE_FILE_NAME: &'static str = "cache.ron";

    pub fn load_or_default_custom<T: DeserializeOwned + Serialize + Default>(filename: &str) -> T {
        let path = cache_dir().join(filename);
        let data = match std::fs::read(&path) {
            Ok(d) => Some(d),
            Err(e) => {
                eprintln!("cache load failed: {}", e);
                None
            },
        };

        if let Some(data) = data {
            ron::de::from_bytes(data.as_slice()).unwrap_or_else(|e| {
                eprintln!("cache load failed: {}", e);
                T::default()
            })
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
        } else { println!("[Cache] it appears you are offline, cache will not be cleaned to ensure you get the most out of what you have cached"); }
        let s = self.clone();
        tokio::fs::write(&cache_dir().join(Self::CACHE_FILE_NAME), ron::ser::to_string_pretty(s.as_ref(), Default::default()).unwrap()).await.unwrap();
    }

    pub fn create_worker(self: Arc<Self>, rt: tokio::runtime::Handle, client: Client, config: Arc<Mutex<CacheConfig>>) {
        rt.clone().spawn(async move {
            loop {
                let config = { config.lock().await.clone() };
                self.cache_pass(client.clone(), &config).await;
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

    pub async fn get_or_else<F: Future<Output = Result<B, E>>, B: AsRef<[u8]>, E: std::error::Error + Send + Sync + 'static>(&self, key: &(Uuid, AssetType), f: impl FnOnce() -> F) -> anyhow::Result<tokio::fs::File> {
        match self.get(key).await {
            Some(f) => Ok(f),
            None => {
                let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let path = cache_dir().join(format!("assets/{:016x}", id));
                let mut file = tokio::fs::File::create(&path).await?;
                let buf = f().await?;
                let buf_ref = buf.as_ref();
                let size = buf_ref.len();
                file.write_all(buf_ref).await?;
                drop(buf);
                file.flush().await?;
                drop(file);
                self.entries.insert(*key, CacheEntry {
                    id,
                    last_touched: SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("Time went backwards (why is your clock before 12AM UTC January 1st 1970?)")
                        .as_secs(),
                    size: size as u64,
                });
                Ok(tokio::fs::File::open(&path).await?)
            }
        }
    }
}