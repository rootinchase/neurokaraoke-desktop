use crate::app::system_tray::TrayIconMenu;
use dashmap::DashMap;
use eframe::egui;
use reqwest::Client;
use std::sync::Arc;
use std::time::Instant;
use tokio::runtime::Runtime;
use uuid::Uuid;

use crate::activity::uploads::UploadsActivity;
use crate::activity::{
    ActivityType, SortOption, favorites::FavoritesActivity, home::HomeActivity,
    playlist::PlaylistActivity, profile::ProfileActivity, queue::QueueActivity,
    radio::RadioActivity, search, setlist::SetlistActivity, settings::SettingsActivity,
};
use crate::api::{LazySongDatabase, LoadingState};
use crate::audio::{LoopMode, PlaybackState, Player};
use crate::config::{Config, SharedConfig};
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::{
    cache::{self, PersistentMediaCache},
    discord::DiscordPresencePayload,
    integration, playwire,
};

pub struct App {
    pub cache: Arc<PersistentMediaCache>,
    pub songs: LazySongDatabase,
    pub player: Player,
    pub integration: integration::PlaybackIntegrationManager,

    pub search_activity: search::SearchActivity,
    pub playlist_search: String,
    pub dragging_seeker: bool,

    // theme stuff
    pub theme: ThemeManager,

    // activity stuff
    pub activity: ActivityType,
    pub home_activity: HomeActivity,
    pub radio_activity: RadioActivity,
    pub playlist_activity: PlaylistActivity,
    pub my_playlist_activity: PlaylistActivity,
    pub setlist_activity: SetlistActivity,
    pub favorites_activity: FavoritesActivity,
    pub uploads_activity: UploadsActivity,
    pub queue_activity: QueueActivity,
    pub profile_activity: ProfileActivity,
    pub settings_activity: SettingsActivity,

    pub current_playlist_sort: SortOption,
    pub current_playlist_sort_desc: bool,

    pub current_song_uuid: Option<Uuid>,
    pub current_playback_state: Option<PlaybackState>,

    pub rt: Arc<Runtime>,
    pub client: Client,

    // Image caching
    pub cached_art_paths: Arc<DashMap<Arc<str>, String>>,
    pub active_art_downloads: Arc<dashmap::DashSet<Arc<str>>>,

    pub config: Config,
    pub shared_config: SharedConfig,

    pub cached_avatar_path: Option<String>,
    pub sleep_timer_end: Option<Instant>,
    pub show_timer_menu: bool,
    pub show_queue: bool,
    pub show_fullscreen: bool,
    pub(crate) _tray_icon: TrayIconMenu,
}

impl App {
    pub fn resolve_artwork_uri(
        &self,
        ctx: &egui::Context,
        cloudflare_id: Option<Arc<str>>,
        absolute_path: Arc<str>,
    ) -> Option<String> {
        let key_str: Arc<str> = cloudflare_id
            .clone()
            .map(|id| id.to_string())
            .unwrap_or_else(|| absolute_path.to_string())
            .into();

        // 1. Check hot in-memory UI reference cache
        if let Some(path_str) = self.cached_art_paths.get::<Arc<str>>(&key_str) {
            return Some(path_str.value().clone());
        }

        // 3. Extract or synthesize a stable, unique internal UUID for the asset file name
        let target_uuid = Uuid::parse_str(&key_str)
            .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, key_str.as_bytes()));

        // 2. Check persistent disk cache before downloading
        if let Some(path) = self
            .cache
            .get_cached_path(target_uuid, cache::AssetType::Image)
        {
            let path_str = path.to_string_lossy().into_owned();
            self.cached_art_paths
                .insert(key_str.clone(), path_str.clone());
            return Some(path_str);
        }

        // 4. Prevent UI dispatch duplication locks
        if self.active_art_downloads.insert(key_str.clone()) {
            let cache = self.cache.clone();
            let cached_paths = self.cached_art_paths.clone();
            let active_downloads = self.active_art_downloads.clone();
            let ctx_clone = ctx.clone();

            let url = cache::get_thumbnail_url(
                cloudflare_id.as_deref(),
                &absolute_path,
                "crop,gravity=auto",
            );

            if url.is_empty() {
                active_downloads.remove::<Arc<str>>(&key_str);
                return None;
            }

            debug_log!("⚡ [Image Pipeline] Checking/Downloading artwork: {}", url);

            // 5. Dispatch tasks safely onto the background executor thread pool
            self.rt.spawn(async move {
                match cache
                    .get_or_download_image(target_uuid, url, cache::AssetType::Image)
                    .await
                {
                    Ok(path) => {
                        let path_str = path.to_string_lossy().into_owned();
                        cached_paths.insert(key_str.clone(), path_str.clone());
                    }
                    Err(e) => {
                        debug_log!(
                            "❌ [Image Pipeline] Download failure for {}: {}",
                            key_str,
                            e
                        );
                    }
                }

                active_downloads.remove::<Arc<str>>(&key_str);
                ctx_clone.request_repaint();
            });
        }

        None
    }

    /// Resolves the full-resolution artwork asset (Cloudflare `public` variant),
    /// cached under a namespaced key so it never collides with the 512x512 thumbnail.
    pub fn resolve_full_artwork_uri(
        &self,
        ctx: &egui::Context,
        cloudflare_id: Option<Arc<str>>,
        absolute_path: Arc<str>,
    ) -> Option<String> {
        let key_str: Arc<str> = format!(
            "full:{}",
            cloudflare_id.as_deref().unwrap_or(absolute_path.as_ref())
        )
        .into();

        // 1. Check hot in-memory UI reference cache
        if let Some(path_str) = self.cached_art_paths.get::<Arc<str>>(&key_str) {
            return Some(path_str.value().clone());
        }

        // 2. Derive a stable UUID namespaced away from the thumbnail key
        let target_uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, key_str.as_bytes());

        // 3. Check persistent disk cache before downloading
        if let Some(path) = self
            .cache
            .get_cached_path(target_uuid, cache::AssetType::FullImage)
        {
            let path_str = path.to_string_lossy().into_owned();
            self.cached_art_paths
                .insert(key_str.clone(), path_str.clone());
            return Some(path_str);
        }

        // 4. Dispatch a background download if one is not already in flight
        if self.active_art_downloads.insert(key_str.clone()) {
            let cache = self.cache.clone();
            let cached_paths = self.cached_art_paths.clone();
            let active_downloads = self.active_art_downloads.clone();
            let ctx_clone = ctx.clone();

            let url = cache::get_full_image_url(cloudflare_id.as_deref(), &absolute_path);

            if url.is_empty() {
                active_downloads.remove::<Arc<str>>(&key_str);
                return None;
            }

            debug_log!(
                "⚡ [Image Pipeline] Checking/Downloading full artwork: {}",
                url
            );

            self.rt.spawn(async move {
                match cache
                    .get_or_download_image(target_uuid, url, cache::AssetType::FullImage)
                    .await
                {
                    Ok(path) => {
                        let path_str = path.to_string_lossy().into_owned();
                        cached_paths.insert(key_str.clone(), path_str.clone());
                    }
                    Err(e) => {
                        debug_log!(
                            "❌ [Image Pipeline] Full artwork download failure for {}: {}",
                            key_str,
                            e
                        );
                    }
                }

                active_downloads.remove::<Arc<str>>(&key_str);
                ctx_clone.request_repaint();
            });
        }

        None
    }

    pub fn update_os_playback(&mut self) {
        if let Some(state) = self.player.get_playback_state() {
            let repeat = match self.player.get_loop_mode() {
                LoopMode::None => playwire::Repeat::Off,
                LoopMode::One => playwire::Repeat::One,
                LoopMode::All => playwire::Repeat::All,
            };
            let shuffle = self.player.get_shuffle();
            let volume = self.player.get_volume() as f64;

            let discord_payload = if let Some(_controls) = &self.integration.media_controls
                && let Some(track) = self.integration.get_track()
            {
                let cloudflare_id = if let Some(id) = self.current_song_uuid {
                    if let LoadingState::Loaded(song) = self.songs.get(&id, |s| s.cover_art.clone())
                    {
                        song.and_then(|art| art.cloudflare_id.map(|cid| cid.to_string()))
                    } else {
                        None
                    }
                } else {
                    None
                }
                .or_else(|| {
                    let guard = self
                        .player
                        .current_url_metadata
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    guard
                        .as_ref()
                        .and_then(|m| m.cover_art.as_ref())
                        .and_then(|a| a.cloudflare_id.as_ref())
                        .map(|id| id.to_string())
                });

                let external_cover_url = cloudflare_id
                    .as_deref()
                    .map(|id| cache::get_thumbnail_url(Some(id), "", "crop,gravity=auto"));

                Some(DiscordPresencePayload {
                    title: track.title.clone(),
                    artist_line: track.artists.join(""),
                    is_playing: !state.paused(),
                    position_secs: state.position().as_secs(),
                    duration_secs: Some(state.duration().as_secs()),
                    cover_url: external_cover_url,
                })
            } else {
                None
            };

            self.integration
                .update_playback(&state, repeat, shuffle, volume, discord_payload);
        }
    }
}
