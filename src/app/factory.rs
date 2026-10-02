use crate::activity::{
    ActivityType, SortOption, favorites::FavoritesActivity, home::HomeActivity,
    playlist::PlaylistActivity, profile::ProfileActivity, profile::ProfileMessage,
    queue::QueueActivity, search, setlist::SetlistActivity, settings::SettingsActivity,
};
use chrono::{Timelike, Utc};

use crate::api::LazySongDatabase;
use crate::app::state::App;
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
};
use dashmap::DashMap;
use eframe::egui;
use egui_extras;
use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use tokio::sync::mpsc::unbounded_channel;
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

    let fonts = build_font_defs![
        "noto-sans-jp"        => "../../assets/fonts/NotoSansJP-Regular.ttf",
        "noto-sans-cuneiform" => "../../assets/fonts/NotoSansCuneiform-Regular.ttf",
        "Roboto"              => "../../assets/fonts/Roboto-VariableFont_wdth,wght.ttf",
        "noto-sans-kr"        => "../../assets/fonts/NotoSansKR-Regular.ttf",
        "noto-sans-sc"        => "../../assets/fonts/NotoSansSC-Regular.ttf",
    ];

    ctx.set_fonts(fonts);

    #[cfg(debug_assertions)]
    ctx.global_style_mut(|s| s.debug.warn_if_rect_changes_id = false);

    let config = Config::read().unwrap_or_default();
    let shared_config = config.to_shared();
    let client = Client::new();

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
            if let Ok(data) = tokio::fs::read(&profile_cache_path).await {
                if let Ok(profile_response) =
                    ron::de::from_bytes::<crate::api::ProfileResponse>(&data)
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
            }
            if let Ok(data) = tokio::fs::read(&limits_cache_path).await {
                if let Ok(limits) = ron::de::from_bytes::<crate::api::UserLimits>(&data) {
                    let _ = startup_tx
                        .send(ProfileMessage::UserLimitsLoaded(limits))
                        .await;
                }
            }

            // 2. Try fetching fresh profile and limits from network
            match songs_clone.get_profile(&stored_token).await {
                Ok(profile_response) => {
                    debug_log!("🟢 Startup profile synchronization complete!");
                    let _ = tokio::fs::write(
                        &profile_cache_path,
                        ron::ser::to_string_pretty(&profile_response, Default::default()).unwrap(),
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
                            let _ = tokio::fs::write(
                                &limits_cache_path,
                                ron::ser::to_string_pretty(&limits, Default::default()).unwrap(),
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

    let player = Player::new(rt.clone(), ctx.clone(), songs.clone(), cache.clone());
    player.volume(config.volume);
    player.shuffle(config.shuffle);
    player.looping(config.loop_mode);

    let signal_player = player.clone();
    let signal_config = config.clone();
    rt.spawn(async move {
        handle_signals(signal_player, signal_config).await;
    });

    let s = songs.clone();
    let cc = config.cache.clone();
    let last_update: Arc<Mutex<Option<Instant>>> = Arc::default();

    rt.spawn(async move {
        loop {
            let interval = {
                let guard = cc.lock().await;
                guard.cache_sweep_interval_secs
            };

            let d = {
                let guard = cc.lock().await;
                Duration::from_secs(guard.cache_expiration_secs)
            };

            if last_update
                .lock()
                .unwrap()
                .map(|i| Instant::now() - i >= d)
                .unwrap_or(true)
            {
                last_update.lock().unwrap().replace(Instant::now());
                if let Ok(_) = s.load_all(|_| ()).await {
                    let write_res = tokio::fs::write(
                        cache::cache_dir().join("songs.ron"),
                        ron::ser::to_string_pretty(&s, Default::default()).unwrap(),
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

            tokio::time::sleep(Duration::from_secs(interval)).await;
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
        player,
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
        playlist_activity: PlaylistActivity::new(
            songs.clone(),
            false,
            cache.clone(),
            ctx.clone(),
            rt.clone(),
            client.clone(),
        ),
        my_playlist_activity: PlaylistActivity::new(
            songs.clone(),
            true,
            cache.clone(),
            ctx.clone(),
            rt.clone(),
            client.clone(),
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
                                debug_log!("[Setlist Poller] Polled stats: totalCount={}", stats.total_count);
                                setlist_activity_clone.check_and_update_stats(stats.total_count).await;
                            }
                            Err(e) => {
                                debug_log!("[Setlist Poller] Failed to fetch stats: {}", e);
                            }
                        }
                        tokio::time::sleep(Duration::from_secs(15 * 60)).await;
                    } else {
                        let target = now.date_naive().and_hms_opt(21, 0, 0).unwrap();
                        let target_utc = target.and_utc();
                        let duration = if target_utc > now {
                            (target_utc - now).to_std().unwrap_or(Duration::from_secs(60 * 60))
                        } else {
                            Duration::from_secs(60 * 60)
                        };
                        tokio::time::sleep(duration).await;
                    }
                }
            });

            setlist_activity
        },
        favorites_activity: FavoritesActivity::new(
            ctx.clone(),
            cache.clone(),
            rt.clone(),
            client.clone(),
            songs.clone(),
        ),
        queue_activity: QueueActivity::new(
            songs.clone(),
        ),
        profile_activity,
        settings_activity: SettingsActivity::new(cache.clone()),

        current_playlist_sort: SortOption::Name,
        current_playlist_sort_desc: false,

        search_activity: search::SearchActivity::new(),

        current_song_uuid: None,
        current_playback_state: None,
        rt: rt.clone(),
        client,
        config,
        shared_config,

        cached_art_paths: Arc::new(DashMap::new()),
        active_art_downloads: Arc::new(dashmap::DashSet::new()),

        cached_avatar_path: None,
        sleep_timer_end: None,
        show_timer_menu: false,
        show_queue: false,
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

    app
}
