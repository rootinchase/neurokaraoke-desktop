use crate::activity::SortOption;
use crate::api::{LazySongDatabase, LoadingState, PlaylistDetail};
use crate::utilities::cache;
use ron::de::from_bytes;
use ron::ser::to_string_pretty;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{DeserializeAs, SerializeAs};
use std::sync::Arc;
use tokio::fs::{read, write};
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::task::block_in_place;
use uuid::Uuid;

pub fn sort_items<T>(
    items: &mut [T],
    current_sort: &SortOption,
    current_sort_desc: &bool,
    get_name: impl Fn(&T) -> String,
    get_song_count: impl Fn(&T) -> u32,
    get_play_count: impl Fn(&T) -> u32,
    get_date: impl Fn(&T) -> Option<String>,
) {
    match current_sort {
        SortOption::Name => {
            if *current_sort_desc {
                items.sort_by(|a, b| get_name(b).to_lowercase().cmp(&get_name(a).to_lowercase()))
            } else {
                items.sort_by(|a, b| get_name(a).to_lowercase().cmp(&get_name(b).to_lowercase()))
            }
        }
        SortOption::Songs => {
            if *current_sort_desc {
                items.sort_by(|a, b| get_song_count(b).cmp(&get_song_count(a)))
            } else {
                items.sort_by(|a, b| get_song_count(a).cmp(&get_song_count(b)))
            }
        }
        SortOption::Plays => {
            if *current_sort_desc {
                items.sort_by(|a, b| get_play_count(b).cmp(&get_play_count(a)))
            } else {
                items.sort_by(|a, b| get_play_count(a).cmp(&get_play_count(b)))
            }
        }
        SortOption::Date => {
            if *current_sort_desc {
                items.sort_by(|a, b| get_date(b).cmp(&get_date(a)))
            } else {
                items.sort_by(|a, b| get_date(a).cmp(&get_date(b)))
            }
        }
    }
}

pub fn split_by_space_respecting_quotes(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

pub fn format_duration(seconds: u64) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("DEBUG_PLAYBACK").is_ok() {
            eprintln!($($arg)*);
        }
    };
}

#[allow(dead_code)]
pub struct IntoAs<T>(std::marker::PhantomData<T>);
impl<'de, U, T: Deserialize<'de> + Into<U>> DeserializeAs<'de, U> for IntoAs<T> {
    fn deserialize_as<D>(deserializer: D) -> Result<U, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(T::deserialize(deserializer)?.into())
    }
}
impl<U, T: Serialize> SerializeAs<U> for IntoAs<T>
where
    for<'a> T: From<&'a U>,
{
    fn serialize_as<S>(source: &U, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        T::from(source).serialize(serializer)
    }
}

pub struct AsArcMutex<T>(std::marker::PhantomData<T>);
impl<'de, T: Deserialize<'de>> DeserializeAs<'de, Arc<Mutex<T>>> for AsArcMutex<T> {
    fn deserialize_as<D>(deserializer: D) -> Result<Arc<Mutex<T>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Arc::new(Mutex::new(T::deserialize(deserializer)?)))
    }
}
impl<T: Serialize> SerializeAs<Arc<Mutex<T>>> for AsArcMutex<T> {
    fn serialize_as<S>(source: &Arc<Mutex<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let guard = if Handle::try_current().is_ok() {
            block_in_place(|| source.blocking_lock())
        } else {
            source.blocking_lock()
        };
        (*guard).serialize(serializer)
    }
}

pub async fn get_playlist_details_cached(
    id: Uuid,
    songs: &LazySongDatabase,
) -> Result<PlaylistDetail, anyhow::Error> {
    let cache_path = cache::cache_dir().join(format!("playlist_detail_{}.ron", id));

    let mut cached_detail = None;
    if let Ok(data) = read(&cache_path).await {
        if let Ok(detail) = from_bytes::<PlaylistDetail>(&data) {
            cached_detail = Some(detail);
        }
    }

    match songs.get_playlist_details(id).await {
        Ok(data) => {
            let _ = write(
                &cache_path,
                to_string_pretty(&data, Default::default()).unwrap(),
            )
            .await;
            Ok(data)
        }
        Err(err) => {
            if let Some(detail) = cached_detail {
                Ok(detail)
            } else {
                Err(err)
            }
        }
    }
}

pub fn select_playlist_by_id(
    id: Uuid,
    selected: Arc<Mutex<Option<LoadingState<PlaylistDetail>>>>,
    songs: LazySongDatabase,
) {
    tokio::spawn(async move {
        let cache_path = cache::cache_dir().join(format!("playlist_detail_{}.ron", id));
        let mut has_cached = false;
        if let Ok(data) = read(&cache_path).await {
            if let Ok(detail) = from_bytes::<PlaylistDetail>(&data) {
                *selected.lock().await = Some(LoadingState::Loaded(detail));
                has_cached = true;
            }
        }

        match get_playlist_details_cached(id, &songs).await {
            Ok(data) => {
                *selected.lock().await = Some(LoadingState::Loaded(data));
            }
            Err(err) => {
                let mut sel_lock = selected.lock().await;
                if !has_cached
                    && (sel_lock.is_none()
                        || matches!(*sel_lock.as_ref().unwrap(), LoadingState::Loading))
                {
                    *sel_lock = Some(LoadingState::Failed(Arc::new(err)));
                }
            }
        }
    });
}
