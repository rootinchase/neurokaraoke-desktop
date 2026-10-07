use crate::activity::SortOption;
use crate::api::{LazySongDatabase, PlaylistDetail};
use crate::utilities::cache;
use ron::de::from_bytes;
use ron::ser::to_string_pretty;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{DeserializeAs, SerializeAs};
use std::io::Write;
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

pub fn write_debug_log(msg: &str) {
    let log_path = match std::panic::catch_unwind(|| crate::config::config_dir().join("debug.log")) {
        Ok(path) => path,
        Err(_) => {
            if let Some(dir) = dirs::config_dir() {
                let fallback = dir.join("neurokaraoke-desktop");
                let _ = std::fs::create_dir_all(&fallback);
                fallback.join("debug.log")
            } else {
                std::path::PathBuf::from("debug.log")
            }
        }
    };

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S%.3f UTC");
        let _ = writeln!(file, "[{}] {}", timestamp, msg);
    }
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        {
            let msg = format!($($arg)*);
            if std::env::var("DEBUG_PLAYBACK").is_ok() {
                eprintln!("{}", msg);
            }
            $crate::utilities::util::write_debug_log(&msg);
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
                to_string_pretty(&data, Default::default())?,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::SortOption;

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "0:00");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_duration(600), "10:00");
    }

    #[test]
    fn test_split_by_space_respecting_quotes() {
        let query = r#"hello "world test" foo bar"#;
        let parts = split_by_space_respecting_quotes(query);
        assert_eq!(parts, vec!["hello", "world test", "foo", "bar"]);
    }

    #[test]
    fn test_sort_items() {
        struct Item {
            name: String,
            songs: u32,
            plays: u32,
            date: Option<String>,
        }

        let mut items = vec![
            Item { name: "B".into(), songs: 10, plays: 100, date: Some("2026-01-01".into()) },
            Item { name: "A".into(), songs: 5, plays: 200, date: Some("2026-02-01".into()) },
        ];

        sort_items(
            &mut items,
            &SortOption::Name,
            &false,
            |i| i.name.clone(),
            |i| i.songs,
            |i| i.plays,
            |i| i.date.clone(),
        );
        assert_eq!(items[0].name, "A");
        assert_eq!(items[1].name, "B");

        sort_items(
            &mut items,
            &SortOption::Songs,
            &true,
            |i| i.name.clone(),
            |i| i.songs,
            |i| i.plays,
            |i| i.date.clone(),
        );
        assert_eq!(items[0].songs, 10);
        assert_eq!(items[1].songs, 5);
    }

    #[test]
    fn test_debug_log_writing() {
        debug_log!("Test debug log entry {}", 123);
        let log_path = match std::panic::catch_unwind(|| crate::config::config_dir().join("debug.log")) {
            Ok(path) => path,
            Err(_) => {
                if let Some(dir) = dirs::config_dir() {
                    dir.join("neurokaraoke-desktop").join("debug.log")
                } else {
                    std::path::PathBuf::from("debug.log")
                }
            }
        };
        if log_path.exists() {
            let content = std::fs::read_to_string(&log_path).unwrap_or_default();
            assert!(content.contains("Test debug log entry 123"));
        }
    }
}
