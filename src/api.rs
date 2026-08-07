use anyhow::anyhow;
use dashmap::DashMap;
use internal::*;
use reqwest::Client;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{json, Value};
use serde_with::{serde_as, DefaultOnNull};
use std::string::ToString;
use std::sync::Arc;
use serde::ser::SerializeMap;
use uuid::Uuid;

mod internal {
    use serde::{Deserialize, Deserializer};
    use std::sync::Arc;
    use uuid::Uuid;

    // WHY????? I DON'T KNOW??????
    #[derive(Deserialize)]
    #[serde(untagged)]
    pub enum PossiblyWithId {
        NoId(Arc<str>),
        Id {
            id: Uuid,
            name: Arc<str>,
        }
    }

    impl From<PossiblyWithId> for Arc<str> {
        fn from(value: PossiblyWithId) -> Self {
            match value {
                PossiblyWithId::NoId(name) => name,
                PossiblyWithId::Id { name, .. } => name
            }
        }
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeArtists {
        Artists(Vec<PossiblyWithId>),
        Optional(Option<Vec<PossiblyWithId>>),
    }

    pub fn deserialize_artists<'de, D>(d: D) -> Result<Arc<[Arc<str>]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let artists = match MaybeArtists::deserialize(d)? {
            MaybeArtists::Artists(artists) => artists,
            MaybeArtists::Optional(artists) => artists.unwrap_or_default(),
        };

        Ok(artists
            .into_iter()
            .map(Into::into)
            .collect::<Vec<_>>()
            .into())
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Artist {
    pub id: Uuid,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub name: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub social_link: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub user_id: Option<Uuid>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Artwork {
    pub id: Uuid,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub description: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub cloudflare_id: Arc<str>,
    pub artist: Option<Artist>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub upvotes: u64
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct Song {
    pub id: Uuid,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub title: Arc<str>,
    pub absolute_path: Option<Arc<str>>,
    pub opus: Option<Arc<str>>,
    #[serde(default, deserialize_with = "deserialize_artists")]
    pub cover_artists: Arc<[Arc<str>]>,
    #[serde(default, deserialize_with = "deserialize_artists")]
    pub original_artists: Arc<[Arc<str>]>,
    pub cover_art: Option<Artwork>,
}

pub enum LoadingState<T> {
    Failed(Arc<anyhow::Error>),
    Loading,
    Loaded(T),
}

impl<T> LoadingState<T> {
    pub fn if_loaded_or_else<U>(&self, if_loaded: impl FnOnce(&T) -> U, otherwise: U) -> U {
        if let LoadingState::Loaded(t) = self { if_loaded(t) } else { otherwise }
    }
}

/// This database is cheap to clone, clone it all you need!
#[derive(Clone)]
pub struct LazySongDatabase {
    pub client: Client,
    pub map: Arc<DashMap<Uuid, LoadingState<Song>>>,
}

impl LazySongDatabase {
    const SONGS_API_URL: &str = "https://api.neurokaraoke.com/api/songs";

    pub fn new(client: Client, map: Arc<DashMap<Uuid, LoadingState<Song>>>) -> Self {
        Self {
            client,
            map,
        }
    }

    pub fn get<T>(&self, id: &Uuid, f: impl FnOnce(&Song) -> T) -> LoadingState<T> {
        if let Some(r) = self.map.get(id) {
            match &*r {
                LoadingState::Loaded(song) => LoadingState::Loaded(f(song)),
                LoadingState::Failed(err) => LoadingState::Failed(err.clone()),
                LoadingState::Loading => LoadingState::Loading,
            }
        } else {
            self.map.insert(id.clone(), LoadingState::Loading);
            let id = *id;
            let client = self.client.clone();
            let map = self.map.clone();
            tokio::spawn(async move {
                map.insert(id, match async { Ok(serde_json::from_slice(client.get(format!("{}/{}", Self::SONGS_API_URL, id.to_string())).send().await?.bytes().await?.as_ref())?) }.await.map_err(Arc::new) {
                    Ok(song) => LoadingState::Loaded(song),
                    Err(err) => LoadingState::Failed(err),
                });
            });
            LoadingState::Loading
        }
    }

    pub async fn load_all<T>(&self, f: impl FnMut(&Song) -> T) -> anyhow::Result<Arc<[LoadingState<T>]>> {
        let json = self.client.post(Self::SONGS_API_URL).json(&json!({"page": 1, "pageSize": 0})).send().await?.json::<Value>().await?;
        let total_count = json.get("totalCount").ok_or_else(|| anyhow!("missing total count"))?.as_u64().ok_or_else(|| anyhow!("missing total count"))?;
        if let Value::Object(ref mut obj) = self.client.post(Self::SONGS_API_URL).json(&json!({"page": 1, "pageSize": total_count})).send().await?.json::<Value>().await? {
            let songs: Vec<Value> = serde_json::from_value(obj.remove("items").ok_or_else(|| anyhow!("missing items"))?)?;
            self.load(songs, f)
        } else {
            Err(anyhow!("invalid response type"))
        }
    }

    fn load<T>(&self, values: Vec<Value>, mut f: impl FnMut(&Song) -> T) -> anyhow::Result<Arc<[LoadingState<T>]>> {
        let mut result = Vec::with_capacity(values.len());
        for value in values {
            let id = Uuid::parse_str(value.get("id").ok_or_else(|| anyhow!("song missing id???"))?.as_str().ok_or_else(|| anyhow!("song missing id???"))?)?;
            match serde_json::from_value::<Song>(value) {
                Ok(song) => {
                    result.push(LoadingState::Loaded(f(&song)));
                    self.map.insert(id, LoadingState::Loaded(song));
                }
                Err(err) => {
                    let e = Arc::new(anyhow!(err));
                    result.push(LoadingState::Failed(e.clone()));
                    self.map.insert(id, LoadingState::Failed(e));
                }
            }
        }
        Ok(result.into())
    }

    pub fn get_map(&self) -> &Arc<DashMap<Uuid, LoadingState<Song>>> { &self.map }
}

impl Serialize for LazySongDatabase {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;

        for entry in self.map.iter() {
            if let LoadingState::Loaded(song) = entry.value() {
                map.serialize_entry(entry.key(), song)
                    .map_err(serde::ser::Error::custom)?;
            }
        }

        map.end()
    }
}