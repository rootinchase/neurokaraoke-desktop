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
use crate::config::{SharedConfig};

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
            
            // Object with name (should match ID)
            let j2 = json!({"name": "Artist 1"});
            let r2: Result<PossiblyWithId, _> = serde_json::from_value(j2);
            assert!(r2.is_ok(), "Failed to deserialize name-only object: {:?}", r2.err());
            
            // Object with id and name (should match ID)
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
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub cloudflare_id: Option<Arc<str>>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
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

/// Active authentication context containing the issued session token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    /// JWT bearer token issued by NeuroKaraoke (not raw third-party tokens).
    pub token: Arc<str>,
    /// Metadata of the authenticated account context.
    pub user: UserClaims,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserClaims {
    pub id: Uuid,

    #[serde(alias = "userName")]
    pub username: Arc<str>,

    #[serde(default)] // Prevents decoding failure if missing entirely on some payloads
    pub email: Option<Arc<str>>,
}

// --- Request Payloads ---

/*
#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub username: Arc<str>,
    pub password: Arc<str>,
}
 */

/*
#[derive(Debug, Serialize)]
pub struct RegisterRequest {
    pub username: Arc<str>,
    pub password: Arc<str>,
    pub email: Option<Arc<str>>,
}
 */

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordTokenRequest {
    /// The access token obtained from Discord's OAuth2 authorization flow.
    pub access_token: Arc<str>,
}

/*
#[derive(Debug, Serialize)]
pub struct RedeemCodeRequest {
    pub code: Arc<str>,
}
 */

// --- Response Payloads ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub token: Arc<str>,
}

/// The inner structure inside the decrypted JWT string payload.
/// Maps the Microsoft XML Soap claims formats used by ASP.NET Core back to your egui workspace layouts.
#[derive(Debug, Deserialize)]
pub(crate) struct JwtPayload {
    #[serde(rename = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/nameidentifier")]
    pub id: String,
    #[serde(rename = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name")]

    pub username: String,
}

/*
#[derive(Clone)]
pub struct AuthService {
    client: Client,
    auth_host: Arc<str>, // https://idk.neurokaraoke.com
    api_host: Arc<str>,  // https://api.neurokaraoke.com
}

 */

/*
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrSession {
    pub session_id: Uuid,
    pub qr_code_data: Arc<str>,
    pub is_linked: bool,
    pub token: Option<Arc<str>>,
}
 */

#[serde_with::serde_as]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileHeader {
    #[serde(alias = "userID")]
    pub user_id: uuid::Uuid,
    #[serde(alias = "displayName")]
    pub display_name: String,
    #[serde(alias = "avatarUrl")]
    pub avatar_url: Option<Arc<str>>,
    pub level: i32,
    #[serde(alias = "levelTitle")]
    pub level_title: Option<String>,
    #[serde(alias = "totalXP")]
    pub total_xp: i32,
    #[serde(alias = "totalBadges")]
    pub total_badges: i32,
    #[serde(alias = "unlockedBadges")]
    pub unlocked_badges: i32,
    #[serde(alias = "collectionProgress")]
    pub collection_progress: Option<f64>,
    #[serde(alias = "xpToNextLevel")]
    pub xp_to_next_level: i32,
    #[serde(alias = "levelProgress")]
    pub level_progress: Option<f64>,
    #[serde(alias = "neuroCoin")]
    pub neuro_coin: i32,
    #[serde(alias = "evilCoin")]
    pub evil_coin: i32,
    #[serde(alias = "twinsCoin")]
    pub twins_coin: i32,
    #[serde(alias = "cardArtUrl")]
    pub card_art_url: Option<String>,
    #[serde(alias = "frameTheme")]
    pub frame_theme: i32,
    #[serde(alias = "displayItemIds")]
    pub display_item_ids: Vec<String>,
    #[serde(alias = "rankScore")]
    pub rank_score: i32,
    #[serde(alias = "rankTier")]
    pub rank_tier: i32,
    #[serde(alias = "unlockedTiers")]
    pub unlocked_tiers: Vec<i32>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BadgeMedia {
    pub id: String,
    #[serde(alias = "fileName")]
    pub file_name: String,
    #[serde(alias = "contentType")]
    pub content_type: String,
    pub description: Option<String>,
    #[serde(alias = "isAnimated")]
    pub is_animated: bool,
    pub credit: Option<String>,
    #[serde(alias = "cloudflareId")]
    pub cloudflare_id: String,
    #[serde(alias = "absolutePath")]
    pub absolute_path: Option<Arc<str>>,
    pub upvotes: i32,
    #[serde(alias = "isSensitive")]
    pub is_sensitive: Option<bool>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Badge {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub rarity: i32,
    pub category: Option<String>,
    pub unlocked: bool,
    pub requirement: Option<String>,
    pub media: Option<BadgeMedia>,
    #[serde(alias = "unlockedAt")]
    pub unlocked_at: Option<String>,
    #[serde(alias = "currentProgress")]
    pub current_progress: i32,
    #[serde(alias = "conditionValue")]
    pub condition_value: i32,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ProfileResponse {
    pub profile: ProfileHeader,
    pub badges: Vec<Badge>,
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
    /// Shared runtime configuration to read the token state dynamically
    pub shared_config: SharedConfig,
}

impl LazySongDatabase {
    const SONGS_API_URL: &str = "https://api.neurokaraoke.com/api/songs";

    pub fn new(
        client: Client,
        map: Arc<DashMap<Uuid, LoadingState<Song>>>,
        guest_id: Arc<str>,
        shared_config: SharedConfig, // <-- Add config parameter here
    ) -> Self {
        Self {
            client,
            map,
            guest_id,
            shared_config,
        }
    }

    async fn apply_auth(&self, mut req_builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        // 1. Safely acquire a thread-safe read guard snapshot from the lock-free config mirror
        let token_lock = self.shared_config.auth_token.read().unwrap();

        // 2. Unpack the cloned value to minimize hold time on the guard
        if let Some(token) = token_lock.as_ref() {
            // User is signed in: append the verified JWT token directly
            req_builder = req_builder.bearer_auth(token);
        } else {
            // User is anonymous: fall back to the guest identifier header
            req_builder = req_builder.header("x-guest-id", self.guest_id.to_string());
        }

        req_builder
    }

    pub async fn get_public_playlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let mut request = self.client.get("https://api.neurokaraoke.com/api/playlist/public");
        request = self.apply_auth(request).await; // <-- Inject headers

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_user_playlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let mut request = self.client.get("https://api.neurokaraoke.com/api/user/playlists");
        request = self.apply_auth(request).await; // <-- Inject headers

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_official_setlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let mut request = self.client.get("https://api.neurokaraoke.com/api/playlists?isSetlist=True");
        request = self.apply_auth(request).await; // <-- Inject headers

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_playlist_details(&self, id: Uuid) -> anyhow::Result<PlaylistDetail> {
        let mut request = self.client.get(format!("https://api.neurokaraoke.com/api/playlist/{}", id));
        request = self.apply_auth(request).await; // <-- Inject headers

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let detail: PlaylistDetail = serde_json::from_value(json)?;
        crate::debug_log !("Deserialized PlaylistDetail: {:?}", detail);

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
            let db_self = self.clone(); // Clone database handle to share within the task context

            tokio::spawn(async move {
                let url = format!("{}/{}", Self::SONGS_API_URL, id.to_string());
                let mut req = client.get(url);
                req = db_self.apply_auth(req).await; // <-- Inject token directly into the thread loop

                map.insert(id, match async { Ok(serde_json::from_slice(req.send().await?.bytes().await?.as_ref())?) }.await.map_err(Arc::new) {
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
