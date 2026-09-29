use crate::api::internal::deserialize_artists;
use crate::config::SharedConfig;
use dashmap::DashMap;
use reqwest::Client;
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

#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    #[serde_as(as = "DefaultOnNull")]
    pub id: String,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub file_name: Arc<str>,
    //#[serde(default)]
    //#[serde_as(as = "DefaultOnNull")]
    //pub description: Arc<str>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub cloudflare_id: Option<Arc<str>>,
    #[serde(default)]
    #[serde_as(as = "DefaultOnNull")]
    pub absolute_path: Arc<str>,
    pub artist: Option<Artist>,
    pub is_sensitive: bool,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOption {
    Name,
    Songs,
    Plays,
    Date,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSortOption {
    Name,
    OriginalArtist,
    CoverArtist,
    Plays,
    Length,
}