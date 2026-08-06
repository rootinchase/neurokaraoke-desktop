use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::AtomicU64;
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};
use dashmap::DashMap;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_with::__private__::DeserializeOwned;
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
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
    pub expires: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Cache {
    pub next_id: AtomicU64,
    pub entries: DashMap<(Uuid, AssetType), CacheEntry>,
}

impl Cache {
    const CACHE_FILE_NAME: &'static str = "cache.ron";

    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    pub fn load_or_default_custom<T: DeserializeOwned + Serialize + Default>(filename: &str) -> T {
        let path = cache_dir().join(filename);
        let data = match std::fs::read(&path) {
            Ok(d) => Some(d),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => panic!("Failed to read cache, delete this file to reset it: {}\n{e}", path.display()),
        };

        if let Some(data) = data {
            match ron::de::from_bytes(data.as_slice()) {
                Ok(c) => c,
                Err(e) => panic!("Failed to deserialize cache, delete this file to reset it: {}\n{e}", path.display()),
            }
        } else {
            T::default()
        }
    }

    pub fn load_or_default() -> Arc<Self> {
        Self::load_or_default_custom(Self::CACHE_FILE_NAME)
    }

    pub fn create_worker(self: Arc<Self>, rt: tokio::runtime::Handle, client: Client, interval: Duration) {
        rt.clone().spawn(async move {
            loop {
                // don't clean cache if you are offline as you won't be able to re-download what gets deleted
                if async { client.get("https://api.neurokaraoke.com/healthz").timeout(Duration::from_secs(5)).send().await?.text().await }.await.map(|s| s == "Healthy").unwrap_or(false) {
                    let now = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("Time went backwards (why is your clock before 12AM UTC January 1st 1970?)")
                        .as_secs();
                    let mut removed = Vec::new();
                    self.entries.retain(|_, entry| if entry.expires == 0 || entry.expires >= now { true } else {
                        removed.push(entry.id);
                        false
                    });
                    for id in removed {
                        let path = cache_dir().join(format!("assets/{:016x}", id));
                        let _ = tokio::fs::remove_file(path).await;
                    }
                } else { println!("[Cache] it appears you are offline, cache will not be cleaned to ensure you get the most out of what you have cached"); }
                let s = self.clone();
                tokio::fs::write(&cache_dir().join(Self::CACHE_FILE_NAME), rt.spawn_blocking(move || ron::ser::to_string_pretty(s.as_ref(), Default::default()).unwrap()).await.unwrap()).await.unwrap();
                tokio::time::sleep(interval).await;
            }
        });
    }

    pub async fn get(&self, key: &(Uuid, AssetType)) -> Option<tokio::fs::File> {
        match match self.entries.get_mut(key) {
            Some(mut entry) => {
                if entry.expires != 0 {
                    entry.expires = SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("Time went backwards (why is your clock before 12AM UTC January 1st 1970?)")
                        .as_secs() + (60 * 60 * 24);
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
                file.write_all(f().await?.as_ref()).await?;
                file.flush().await?;
                drop(file);
                self.entries.insert(*key, CacheEntry {
                    id,
                    expires: SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .expect("Time went backwards (why is your clock before 12AM UTC January 1st 1970?)")
                        .as_secs() + (60 * 60 * 24),
                });
                Ok(tokio::fs::File::open(&path).await?)
            }
        }
    }
}