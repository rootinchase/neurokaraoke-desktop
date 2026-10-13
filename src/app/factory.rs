use crate::activity::{
    ActivityType, SortOption, favorites::FavoritesActivity, home::HomeActivity,
    playlist::PlaylistActivity, profile::ProfileActivity, profile::ProfileMessage,
    queue::QueueActivity, radio, search, setlist::SetlistActivity, settings::SettingsActivity,
    uploads::UploadsActivity,
};
use chrono::{Timelike, Utc};

use crate::api::{LazySongDatabase, ProfileResponse, UserLimits};
use crate::app::state::{App, evict_art_paths};
use crate::app::system_tray::TrayIconMenu;
use crate::audio::Player;
use crate::config::Config;
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::{
    cache::{self, PersistentMediaCache},
    discord,
    discord::DiscordPresencePayload,
    integration,
    persistence::handle_signals,
    util::create_reqwest_client,
    util::log_memory,
};
use dashmap::DashMap;
use eframe::egui;
use eframe::egui::{Vec2, ViewportCommand};
use egui_extras;
use ron::ser::to_string_pretty;
use std::collections::HashSet;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::fs::{read, write};
use tokio::runtime::Runtime;
use tokio::sync::Mutex as TokioMutex;
use tokio::sync::mpsc::unbounded_channel;
use tokio::time::sleep;
use uuid::Uuid;

macro_rules! build_font_defs {
    ( $( $name:literal => $path:literal ),* $(,)? ) => {{
        let mut fonts = egui::FontDefinitions::default();

        $(
            let bytes = include_bytes!($path);

            fonts.font_data.insert(
                $name.to_string(),
                Arc::new(egui::FontData::from_static(bytes)),
            );

            fonts.families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, $name.to_string());
        )*

        fonts
    }};
}

pub fn create_app(creation_ctx: &eframe::CreationContext, rt: Arc<Runtime>) -> App {
    let ctx = &creation_ctx.egui_ctx;
    egui_extras::install_image_loaders(ctx);

    // Roboto is the only font loaded eagerly. The Noto CJK/cuniform fallbacks are
    // decompressed and registered lazily when text containing their scripts shows up
    // (see `crate::app::fonts`).
    let fonts = build_font_defs![
        "Roboto" => "../../assets/fonts/Roboto-VariableFont_wdth,wght.ttf",
    ];

    ctx.set_fonts(fonts);
    crate::app::fonts::init_lazy_fonts(ctx.clone(), rt.clone());

    #[cfg(debug_assertions)]
    ctx.global_style_mut(|s| s.debug.warn_if_rect_changes_id = false);

    let config = Config::read().unwrap_or_default();
    let initial_min_width = if config.compact_sidebar {
        852.0
    } else {
        1040.0
    };
    ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(Vec2::new(
        initial_min_width,
        610.0,
    )));
    let shared_config = config.to_shared();
    let client = create_reqwest_client();

    let cache = Arc::new(PersistentMediaCache::new(client.clone(), 12));
    rt.spawn(async {
        let _ = PersistentMediaCache::cleanup_stale_downloads().await;
    });

    let profile_activity = ProfileActivity::new(cache.clone());
    let guest_id: Arc<str> = Uuid::new_v4().to_string().into();

    let songs = LazySongDatabase::new(
        client.clone(),
        Arc::new(DashMap::new()),
        guest_id,
        shared_config.clone(),
    );

    let current_auth = { config.auth.clone() };
    if let Some(auth_ctx) = &current_auth {
        let stored_token = auth_ctx.token.clone();

        let startup_tx = profile_activity.get_sender_handle();
        let ctx_clone = ctx.clone();
        let songs_clone = songs.clone();
        rt.spawn(async move {
            let profile_cache_path = cache::cache_dir().join("profile.ron");
            let limits_cache_path = cache::cache_dir().join("limits.ron");

            // 1. Try loading cached profile and limits immediately as offline fallback
            if let Ok(data) = read(&profile_cache_path).await
                && let Ok(profile_response) = ron::de::from_bytes::<ProfileResponse>(&data)
            {
                let _ = startup_tx
                    .send(ProfileMessage::ProfileHeaderLoaded(
                        profile_response.profile,
                    ))
                    .await;
                let _ = startup_tx
                    .send(ProfileMessage::BadgesLoaded(profile_response.badges))
                    .await;
            }
            if let Ok(data) = read(&limits_cache_path).await
                && let Ok(limits) = ron::de::from_bytes::<UserLimits>(&data)
            {
                let _ = startup_tx
                    .send(ProfileMessage::UserLimitsLoaded(limits))
                    .await;
            }

            // 2. Try fetching fresh profile and limits from network
            match songs_clone.get_profile(&stored_token).await {
                Ok(profile_response) => {
                    debug_log!("🟢 Startup profile synchronization complete!");
                    let _ = write(
                        &profile_cache_path,
                        to_string_pretty(&profile_response, Default::default()).unwrap(),
                    )
                    .await;

                    let _ = startup_tx
                        .send(ProfileMessage::ProfileHeaderLoaded(
                            profile_response.profile,
                        ))
                        .await;

                    let _ = startup_tx
                        .send(ProfileMessage::BadgesLoaded(profile_response.badges))
                        .await;

                    match songs_clone.get_user_limits().await {
                        Ok(limits) => {
                            let _ = write(
                                &limits_cache_path,
                                to_string_pretty(&limits, Default::default()).unwrap(),
                            )
                            .await;
                            let _ = startup_tx
                                .send(ProfileMessage::UserLimitsLoaded(limits))
                                .await;
                        }
                        Err(e) => {
                            debug_log!("❌ Failed to fetch user limits on startup: {}", e);
                        }
                    }
                }
                Err(e) => debug_log!("❌ Startup profile fetch collapsed: {}", e),
            }
            ctx_clone.request_repaint();
        });
    }

    let player = Player::new(
        rt.clone(),
        ctx.clone(),
        songs.clone(),
        cache.clone(),
        client.clone(),
    );
    player.volume(config.volume);
    player.shuffle(config.shuffle);
    player.looping(config.loop_mode);

    let signal_player = player.clone();
    let signal_config = config.clone();
    rt.spawn(async move {
        handle_signals(signal_player, signal_config).await;
    });

    // Shared set of favorited playlist ids, populated by `FavoritesActivity` and
    // read by the playlist detail view to show/toggle the ♡ button.
    let favorite_playlists: Arc<TokioMutex<HashSet<Uuid>>> =
        Arc::new(TokioMutex::new(HashSet::new()));

    let favorites_activity = FavoritesActivity::new(
        ctx.clone(),
        cache.clone(),
        rt.clone(),
        client.clone(),
        songs.clone(),
        favorite_playlists.clone(),
    );

    // In-memory artwork reference cache and its LRU access-order tracker, shared
    // between the App and the background eviction loop.
    let cached_art_paths: Arc<DashMap<Arc<str>, String>> = Arc::new(DashMap::new());
    let art_last_used: Arc<DashMap<Arc<str>, u64>> = Arc::new(DashMap::new());
    let art_counter: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));

    let s = songs.clone();
    let cc = config.cache.clone();
    let cache_clone = cache.clone();
    let player_for_evict = player.clone();
    let favs_for_evict = favorites_activity.clone();
    let cached_art_paths_for_evict = cached_art_paths.clone();
    let art_last_used_for_evict = art_last_used.clone();
    let last_update: Arc<Mutex<Option<Instant>>> = Arc::default();
    // Track the last logged memory counts so the background sweep only writes
    // to the debug log when the counts actually change.
    let mut last_counts = (usize::MAX, usize::MAX, usize::MAX);

    rt.spawn(async move {
        loop {
            let (interval, expiration, size_limit_bytes) = {
                let guard = cc.lock().await;
                (
                    guard.cache_sweep_interval_secs,
                    guard.cache_expiration_secs,
                    guard.cache_size_limit_mb * 1024 * 1024,
                )
            };

            // Run cache eviction sweep
            let _ = cache_clone
                .evict(Duration::from_secs(expiration), size_limit_bytes)
                .await;

            // In-memory LRU eviction for the song database and artwork cache.
            let (song_map_max, art_path_max, in_flight_max) = {
                let guard = cc.lock().await;
                (
                    guard.song_map_max_entries,
                    guard.art_path_max_entries,
                    guard.in_flight_max_entries,
                )
            };

            let mut pinned: Vec<Uuid> = Vec::new();
            if let Some(state) = player_for_evict.get_playback_state() {
                pinned.push(state.song());
            }
            if let Some(pl) = player_for_evict.get_playlist() {
                pinned.extend(pl.iter().copied());
            }
            if let Some(upl) = player_for_evict.get_url_playlist() {
                pinned.extend(upl.iter().map(|d| d.id));
            }
            let favs = favs_for_evict.favorite_songs.read().await;
            pinned.extend(favs.iter().copied());
            drop(favs);

            s.evict_to_fit(song_map_max, &pinned);
            evict_art_paths(
                &cached_art_paths_for_evict,
                &art_last_used_for_evict,
                art_path_max,
            );
            cache_clone.evict_in_flight(in_flight_max);
            let counts = (
                s.map.len(),
                cached_art_paths_for_evict.len(),
                cache_clone.in_flight_len(),
            );
            // Gate memory logging behind DEBUG_MEMORY and only emit when the
            // counts changed, so the sweep does not write to the debug log on
            // every tick.
            if std::env::var("DEBUG_MEMORY").is_ok() && counts != last_counts {
                debug_log!(
                    "[mem] song_map={} art_paths={} in_flight={}",
                    counts.0,
                    counts.1,
                    counts.2
                );
                log_memory("background eviction");
                last_counts = counts;
            }

            let d = Duration::from_secs(expiration);

            if last_update
                .lock()
                .unwrap()
                .map(|i| Instant::now() - i >= d)
                .unwrap_or(true)
            {
                last_update.lock().unwrap().replace(Instant::now());
                if s.load_all(|_| ()).await.is_ok() {
                    let write_res = write(
                        cache::cache_dir().join("songs.ron"),
                        to_string_pretty(&s, Default::default()).unwrap(),
                    )
                    .await;

                    if let Err(e) = write_res {
                        debug_log!(
                            "⚠️ [Factory Background] Failed to save songs database: {}",
                            e
                        );
                    }
                }
            }

            sleep(Duration::from_secs(interval)).await;
        }
    });

    let (discord_tx, discord_rx) = unbounded_channel::<DiscordPresencePayload>();
    rt.spawn(async move {
        discord::spawn_discord_worker(discord_rx).await;
    });

    let integration = integration::PlaybackIntegrationManager::new(
        player.clone(),
        shared_config.clone(),
        discord_tx,
    );

    let tray_icon = TrayIconMenu::new(ctx);

    let app = App {
        cache: cache.clone(),
        songs: songs.clone(),
        player: player.clone(),
        integration,
        playlist_search: "".to_string(),
        dragging_seeker: false,

        theme: ThemeManager::new(config.theme.as_theme()),

        activity: ActivityType::Home,
        home_activity: {
            let ha = HomeActivity::new(
                ctx.clone(),
                cache.clone(),
                rt.clone(),
                client.clone(),
                songs.clone(),
            );
            if profile_activity.is_logged_in() {
                ha.fetch_suggested();
                ha.fetch_trending();
                ha.fetch_recent_setlist();
            }
            ha
        },
        radio_activity: radio::RadioActivity::new(
            ctx.clone(),
            cache.clone(),
            rt.clone(),
            client.clone(),
            songs.clone(),
            player.clone(),
        ),
        playlist_activity: PlaylistActivity::new(
            songs.clone(),
            false,
            cache.clone(),
            ctx.clone(),
            rt.clone(),
            client.clone(),
            favorite_playlists.clone(),
        ),
        my_playlist_activity: PlaylistActivity::new(
            songs.clone(),
            true,
            cache.clone(),
            ctx.clone(),
            rt.clone(),
            client.clone(),
            favorite_playlists.clone(),
        ),
        setlist_activity: {
            let setlist_activity = SetlistActivity::new(
                songs.clone(),
                cache.clone(),
                ctx.clone(),
                rt.clone(),
                client.clone(),
            );

            let setlist_activity_clone = setlist_activity.clone();
            let songs_clone = songs.clone();
            rt.spawn(async move {
                loop {
                    let now = Utc::now();
                    let hour = now.hour();

                    if hour >= 21 {
                        match songs_clone.get_setlist_stats().await {
                            Ok(stats) => {
                                debug_log!(
                                    "[Setlist Poller] Polled stats: totalCount={}",
                                    stats.total_count
                                );
                                setlist_activity_clone
                                    .check_and_update_stats(stats.total_count)
                                    .await;
                            }
                            Err(e) => {
                                debug_log!("[Setlist Poller] Failed to fetch stats: {}", e);
                            }
                        }
                        sleep(Duration::from_secs(15 * 60)).await;
                    } else {
                        let target = now.date_naive().and_hms_opt(21, 0, 0).unwrap();
                        let target_utc = target.and_utc();
                        let duration = if target_utc > now {
                            (target_utc - now)
                                .to_std()
                                .unwrap_or(Duration::from_secs(60 * 60))
                        } else {
                            Duration::from_secs(60 * 60)
                        };
                        sleep(duration).await;
                    }
                }
            });

            setlist_activity
        },
        favorites_activity,
        uploads_activity: UploadsActivity::new(ctx.clone(), songs.clone()),
        queue_activity: QueueActivity::new(songs.clone()),
        profile_activity,
        settings_activity: SettingsActivity::new(cache.clone()),

        current_playlist_sort: SortOption::Name,
        current_playlist_sort_desc: false,

        search_activity: search::SearchActivity::new(),

        current_song_uuid: None,
        current_playback_state: None,
        logged_lyrics_missing: HashSet::new(),
        rt: rt.clone(),
        client,
        config,
        shared_config,

        cached_art_paths: cached_art_paths.clone(),
        active_art_downloads: Arc::new(dashmap::DashSet::new()),
        art_last_used: art_last_used.clone(),
        art_counter: art_counter.clone(),

        cached_avatar_path: None,
        sleep_timer_end: None,
        show_timer_menu: false,
        show_queue: false,
        show_fullscreen: false,
        _tray_icon: tray_icon,
    };

    let songs_clone = app.songs.clone();
    let favs_clone = app.favorites_activity.favorite_songs.clone();
    let ctx_clone = ctx.clone();
    let rt_clone = rt.clone();
    rt_clone.spawn(async move {
        if let Ok(favs) = songs_clone.get_favorite_songs().await {
            let mut lock = favs_clone.write().await;
            for song in favs {
                lock.insert(song.id);
            }
            ctx_clone.request_repaint();
        }
    });

    if app.config.auth.is_some() {
        app.uploads_activity.fetch_uploads();
    }

    log_memory("startup");
    app
}
