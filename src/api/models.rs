use crate::api::internal::deserialize_artists;
use crate::config::SharedConfig;
use dashmap::DashMap;
use reqwest::Client;
use serde::de::Error;
use std::fmt::Formatter;
use serde::{Deserialize, Serialize};
use serde_with::{DefaultOnNull, serde_as};
use std::sync::Arc;
use uuid::Uuid;

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: Uuid,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub name: Arc<str>,
    #[serde_as(as = "DefaultOnNull")]
    #[serde(default, alias = "createdBy")]
    pub creator: Arc<str>,

    pub media: Option<Artwork>,
    #[serde_as(as = "Option<Vec<serde_with::DefaultOnNull>>")]
    pub mosaic_media: Option<Vec<Artwork>>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,

    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub description: Arc<str>,

    #[serde(default)]
    pub song_count: u64,
    #[serde(default)]
    pub play_count: u64,
    pub favorite_count: Option<u64>,
    pub playlist_type: Option<i32>,

    pub editable: bool,
    pub deletable: bool,
    pub is_public: bool,
    pub is_set_list: bool,
    #[serde(alias = "setListDate")]
    pub set_list_date: Option<String>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistDetail {
    pub name: Arc<str>,
    #[serde(alias = "songListDTOs")]
    pub songs: Vec<SongDTO>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteEntry {
    pub id: Uuid,
    pub song: Option<SongDTO>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SongDTO {
    pub id: Uuid,
    pub title: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub opus: Option<Arc<str>>,
    #[serde(alias = "cnPath")]
    pub audio_url: Option<Arc<str>>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub absolute_path: Option<Arc<str>>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub oss: Option<Arc<str>>,
    #[serde(
        default = "default_cover_art",
        deserialize_with = "deserialize_cover_art"
    )]
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

pub fn default_cover_art() -> Option<Artwork> {
    Some(Artwork::default_art())
}

pub fn deserialize_cover_art<'de, D>(deserializer: D) -> Result<Option<Artwork>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<Artwork>::deserialize(deserializer)?;
    Ok(Some(opt.unwrap_or_else(Artwork::default_art)))
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    #[serde_as(as = "DefaultOnNull")]
    pub id: String,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub file_name: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub cloudflare_id: Option<Arc<str>>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub absolute_path: Arc<str>,
    pub artist: Option<Artist>,
    pub is_sensitive: bool,
}

impl Artwork {
    pub fn default_art() -> Self {
        Self {
            id: "277232b2-e00e-426b-ffb8-bb8664a73600".to_string(),
            file_name: Arc::from(""),
            cloudflare_id: Some(Arc::from("277232b2-e00e-426b-ffb8-bb8664a73600")),
            absolute_path: Arc::from(""),
            artist: None,
            is_sensitive: false,
        }
    }

    pub fn from_url(art_uuid: Uuid, art_url: &str) -> Self {
        Self {
            id: art_uuid.to_string(),
            file_name: art_url.into(),
            cloudflare_id: Some(art_uuid.to_string().into()),
            absolute_path: art_url.into(),
            artist: None,
            is_sensitive: false,
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
    #[serde(
        default = "default_cover_art",
        deserialize_with = "deserialize_cover_art"
    )]
    pub cover_art: Option<Artwork>,
    #[serde(rename = "playCount", default)]
    pub play_count: Option<u64>,
    #[serde(rename = "duration", default)]
    pub duration: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    pub token: Arc<str>,
    pub user: UserClaims,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserClaims {
    pub id: Uuid,
    #[serde(alias = "userName")]
    pub username: Arc<str>,
    #[serde(default)]
    pub email: Option<Arc<str>>,
}

#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub username: Arc<str>,
    pub password: Arc<str>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct RegisterRequest {
    pub username: Arc<str>,
    pub password: Arc<str>,
    pub email: Option<Arc<str>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordTokenRequest {
    pub access_token: Arc<str>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct RedeemCodeRequest {
    pub code: Arc<str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub token: Arc<str>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct JwtPayload {
    #[serde(rename = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/nameidentifier")]
    pub id: String,
    #[serde(rename = "http://schemas.xmlsoap.org/ws/2005/05/identity/claims/name")]
    pub username: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct QrSession {
    pub session_id: Uuid,
    pub qr_code_data: Arc<str>,
    pub is_linked: bool,
    pub token: Option<Arc<str>>,
}

#[serde_as]
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileHeader {
    #[serde(alias = "userID")]
    pub user_id: Uuid,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum TrendingTimes {
    #[default]
    Week = 7,
    Fortnite = 14,
    Month = 30,
}

impl<T> LoadingState<T> {
    pub fn if_loaded_or_else<U>(&self, if_loaded: impl FnOnce(&T) -> U, otherwise: U) -> U {
        if let LoadingState::Loaded(t) = self {
            if_loaded(t)
        } else {
            otherwise
        }
    }
}

#[derive(Clone)]
pub struct LazySongDatabase {
    pub client: Client,
    pub map: Arc<DashMap<Uuid, LoadingState<Song>>>,
    pub guest_id: Arc<str>,
    /// Shared runtime configuration to read the token state dynamically
    pub shared_config: SharedConfig,
}

#[derive(Serialize)]
#[allow(dead_code)]
pub struct UploadSong {
    pub url: String,
    pub playlist_url: String,
}

impl SongDTO {
    pub fn is_valid(&self) -> bool {
        !self.title.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteItem {
    pub playlist: Option<Playlist>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserLimits {
    pub max_songs: u64,
    pub max_storage_bytes: u64,
    pub used_storage_bytes: u64,
    pub current_song_count: u64,
    pub current_playlist_count: u64,
    pub playlist_limit: u64,
    pub song_per_playlist_limit: u64,
}
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetlistStats {
    pub total_count: u64,
    pub years: Vec<u32>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioCurrentStateResponse {
    pub current: Option<SongDTO>,
    #[serde(default)]
    pub upcoming: Vec<SongDTO>,
    #[serde(default)]
    pub history: Vec<SongDTO>,
    #[serde(default)]
    pub listener_count: u64,
    #[serde(default)]
    pub offline: bool,
    pub playlist_name: Option<Arc<str>>,
}

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastNowPlayingResponse {
    pub station: Option<AzuraCastStation>,
    pub listeners: Option<AzuraCastListeners>,
    pub now_playing: Option<AzuraCastTrackInfo>,
    pub playing_next: Option<AzuraCastTrackInfo>,
    #[serde(default)]
    pub song_history: Vec<AzuraCastTrackInfo>,
    pub is_online: bool,
}

impl AzuraCastNowPlayingResponse {
    pub fn effective_now_playing(&self) -> Option<AzuraCastTrackInfo> {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(next) = &self.playing_next {
            if let Some(played_at) = next.played_at {
                if now_secs >= played_at {
                    return Some(next.clone());
                }
            }
        }

        self.now_playing.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastMountListeners {
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub total: Option<u64>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub unique: Option<u64>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub current: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastMount {
    pub id: u64,
    pub name: Option<String>,
    pub url: Option<String>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub bitrate: Option<u64>,
    pub format: Option<String>,
    pub listeners: Option<AzuraCastMountListeners>,
    pub path: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastStation {
    pub id: u64,
    pub name: Option<String>,
    pub shortcode: Option<String>,
    pub listen_url: Option<String>,
    pub url: Option<String>,
    pub public_player_url: Option<String>,
    pub playlist_pls_url: Option<String>,
    pub playlist_m3u_url: Option<String>,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub requests_enabled: bool,
    #[serde(default)]
    pub mounts: Vec<AzuraCastMount>,
}

impl AzuraCastStation {
    pub fn available_streams(&self) -> Vec<(String, String)> {
        let mut streams = Vec::new();
        for mount in &self.mounts {
            if let Some(url) = &mount.url {
                let name = mount
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Mount {}", mount.id));
                streams.push((name, url.clone()));
            }
        }
        if streams.is_empty() {
            if let Some(listen_url) = &self.listen_url {
                streams.push(("Default MP3".to_string(), listen_url.clone()));
            }
        }
        streams
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastListeners {
    pub total: u64,
    pub unique: u64,
    pub current: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastTrackInfo {
    pub sh_id: Option<u64>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub played_at: Option<u64>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub duration: Option<u64>,
    pub playlist: Option<String>,
    pub streamer: Option<String>,
    pub is_request: Option<bool>,
    pub song: Option<AzuraCastSong>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub elapsed: Option<u64>,
    #[serde(deserialize_with = "deserialize_opt_u64_flexible", default)]
    pub remaining: Option<u64>,
}

impl AzuraCastTrackInfo {
    pub fn elapsed_secs(&self) -> u64 {
        let base_elapsed = self.elapsed.unwrap_or(0);
        let duration = self.duration.unwrap_or(0);

        if let (Some(played_at), Some(dur)) = (self.played_at, self.duration) {
            if let Ok(duration_since_epoch) =
                std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH)
            {
                let now_secs = duration_since_epoch.as_secs();
                if now_secs >= played_at {
                    let elapsed = now_secs - played_at;
                    if elapsed <= dur + 30 {
                        return elapsed.min(dur);
                    }
                }
            }
        }

        if duration > 0 {
            base_elapsed.min(duration)
        } else {
            base_elapsed
        }
    }

    pub fn duration_secs(&self) -> u64 {
        self.duration.unwrap_or(0)
    }

    pub fn remaining_secs(&self) -> u64 {
        let dur = self.duration_secs();
        let el = self.elapsed_secs();
        dur.saturating_sub(el)
    }
}

pub fn deserialize_opt_u64_flexible<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptU64Visitor;
    impl<'de> serde::de::Visitor<'de> for OptU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut Formatter) -> std::fmt::Result {
            formatter.write_str("an integer, float, or null")
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(v.max(0) as u64))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(v))
        }

        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(v.round().max(0.0) as u64))
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(self)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_option(OptU64Visitor)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzuraCastSong {
    pub id: Option<String>,
    pub text: Option<String>,
    pub artist: Option<String>,
    pub title: Option<String>,
    pub art: Option<String>,
    #[serde(default)]
    pub custom_fields: Option<std::collections::HashMap<String, String>>,
}

impl AzuraCastSong {
    pub fn song_id_uuid(&self) -> Option<Uuid> {
        self.custom_fields
            .as_ref()
            .and_then(|cf| cf.get("songId"))
            .and_then(|sid| {
                let trimmed = sid.trim();
                if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
                    None
                } else {
                    Uuid::parse_str(trimmed).ok()
                }
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameHubScheduledInfo {
    pub active: bool,
}
