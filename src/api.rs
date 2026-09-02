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
    #[derive(Deserialize, Debug)]
    #[serde(untagged)]
    pub enum PossiblyWithId {
        NoId(Arc<str>),
        Id {
            id: Option<Uuid>,
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
        // The API might send a single string or an array
        Single(Arc<str>),
        List(Vec<PossiblyWithId>),
        Optional(Option<Vec<PossiblyWithId>>),
    }

    #[cfg(test)]
    mod tests {
        use super::MaybeArtists;
        use super::PossiblyWithId;
        use serde_json::json;

        #[test]
        fn test_maybe_artists_deserialization() {
            // Case 1: Simple list of artists (now List variant)
            let j1 = json!([{"name": "Artist 1"}]);
            let r1: Result<MaybeArtists, _> = serde_json::from_value(j1);
            assert!(r1.is_ok(), "Failed to deserialize list: {:?}", r1.err());

            // Case 2: Null
            let j2 = json!(null);
            let r2: Result<MaybeArtists, _> = serde_json::from_value(j2);
            assert!(r2.is_ok(), "Failed to deserialize null: {:?}", r2.err());

            // Case 3: Empty list
            let j3 = json!([]);
            let r3: Result<MaybeArtists, _> = serde_json::from_value(j3);
            assert!(r3.is_ok(), "Failed to deserialize empty list: {:?}", r3.err());

            // Case 4: Single String
            let j4 = json!("Artist 1");
            let r4: Result<MaybeArtists, _> = serde_json::from_value(j4);
            assert!(r4.is_ok(), "Failed to deserialize single string: {:?}", r4.err());
        }
        
        #[test]
        fn test_possibly_with_id_deserialization() {
            // String (should match NoId)
            let j1 = json!("Artist 1");
            let r1: Result<PossiblyWithId, _> = serde_json::from_value(j1);
            assert!(r1.is_ok(), "Failed to deserialize string: {:?}", r1.err());
            
            // Object with name (should match Id)
            let j2 = json!({"name": "Artist 1"});
            let r2: Result<PossiblyWithId, _> = serde_json::from_value(j2);
            assert!(r2.is_ok(), "Failed to deserialize name-only object: {:?}", r2.err());
            
            // Object with id and name (should match Id)
            let j3 = json!({"id": "550e8400-e29b-41d4-a716-446655440000", "name": "Artist 1"});
            let r3: Result<PossiblyWithId, _> = serde_json::from_value(j3);
            assert!(r3.is_ok(), "Failed to deserialize full object: {:?}", r3.err());
            
            // Empty object (should fail?)
            let j4 = json!({});
            let r4: Result<PossiblyWithId, _> = serde_json::from_value(j4);
            assert!(r4.is_err(), "Should have failed to deserialize empty object: {:?}", r4);
        }
    }

    pub fn deserialize_artists<'de, D>(d: D) -> Result<Arc<[Arc<str>]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let artists = match MaybeArtists::deserialize(d)? {
            MaybeArtists::Single(name) => vec![PossiblyWithId::NoId(name)],
            MaybeArtists::List(artists) => artists,
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
pub struct Playlist {
    pub id: Uuid,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub name: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub creator: Arc<str>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct PlaylistDetail {
    pub name: Arc<str>,
    #[serde(alias = "songListDTOs")]
    pub songs: Vec<SongDTO>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct SongDTO {
    pub title: Arc<str>,
    #[serde(alias = "cnPath")]
    pub audio_url: Option<Arc<str>>,
    pub cover_art: Option<Artwork>,
    #[serde(default, deserialize_with = "deserialize_artists")]
    pub original_artists: Arc<[Arc<str>]>,
    #[serde(default, deserialize_with = "deserialize_artists")]
    pub cover_artists: Arc<[Arc<str>]>,
    #[serde(rename = "playCount", default)]
    pub play_count: Option<u64>,
    #[serde(rename = "streamDate")]
    pub stream_date: Option<Arc<str>>,
    #[serde(rename = "duration", default)]
    pub duration: Option<u64>,
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
    pub file_name: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub description: Arc<str>,
    pub cloudflare_id: Arc<str>,
    pub absolute_path: Arc<str>,
    pub artist: Option<Artist>,
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
    pub guest_id: Arc<str>,
}

impl LazySongDatabase {
    const SONGS_API_URL: &str = "https://api.neurokaraoke.com/api/songs";

    pub fn new(client: Client, map: Arc<DashMap<Uuid, LoadingState<Song>>>, guest_id: Arc<str>) -> Self {
        Self {
            client,
            map,
            guest_id,
        }
    }

    pub async fn get_public_playlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let response = self.client
            .get("https://api.neurokaraoke.com/api/playlist/public")
            .header("x-guest-id", self.guest_id.to_string())
            .send()
            .await?;
        
        if !response.status().is_success() {
             return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        
        // The API returns an Array directly, not an object containing "items"
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_official_setlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let response = self.client
            .get("https://api.neurokaraoke.com/api/playlists?isSetlist=True")
            .header("x-guest-id", self.guest_id.to_string())
            .send()
            .await?;
        
        if !response.status().is_success() {
             return Err(anyhow!("Failed to fetch: {}", response.status()));
        }
        
        let json: Value = response.json().await?;
        
        // The API returns an Array directly, not an object containing "items"
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_playlist_details(&self, id: Uuid) -> anyhow::Result<PlaylistDetail> {
        let response = self.client
            .get(format!("https://api.neurokaraoke.com/api/playlist/{}", id))
            .header("x-guest-id", self.guest_id.to_string())
            .send()
            .await?;
        
        if !response.status().is_success() {
             return Err(anyhow!("Failed to fetch: {}", response.status()));
        }
        
        let json: Value = response.json().await?;
        
        let detail: PlaylistDetail = serde_json::from_value(json)?;
        
        // Log the deserialized detail
        //crate::debug_log !("Deserialized PlaylistDetail: {:?}", detail);
        
        Ok(detail)
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
