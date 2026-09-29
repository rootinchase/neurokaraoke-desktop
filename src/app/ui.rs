use crate::activity::{self, ActivityType, profile::ProfileMessage};
use crate::api::{API_URLS, LazySongDatabase, LoadingState, ProfileResponse, Song, SongDTO};
use crate::app::{player_ui::render_player_controls, state::App};
use crate::audio::Player;
use crate::auth::AuthService;
use crate::debug_log;
use crate::utilities::{cache, persistence};
use std::sync::Arc;

use eframe;
use eframe::egui::{self, CentralPanel, Color32, Ui};
use std::time::{Duration, Instant};
use tokio::spawn;
use tokio::time::sleep;
use uuid::Uuid;

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        if let Some(end) = self.sleep_timer_end {
            if Instant::now() >= end {
                self.player.pause();
                self.sleep_timer_end = None;
            }
        }

        if let Some(msg) = self.profile_activity.poll_messages() {
            match msg {
                ProfileMessage::LoginSuccess(context) => {
                    debug_log!(
                        "🔐 [Auth Sync] Login success captured. Updating app runtime structures..."
                    );

                    self.config.auth = Some(context.clone());
                    let _ = self.config.write_config();

                    if let Ok(mut token_guard) = self.shared_config.auth_token.write() {
                        *token_guard = Some(context.token.clone());
                    }

                    let client_clone = self.client.clone();
                    let ctx_clone = ui.ctx().clone();
                    let stored_token = context.token.clone();
                    let tx_channel = self.profile_activity.get_sender_handle();
                    let shared_config = self.shared_config.clone();
                    let rt = self.rt.clone();

                    rt.spawn(async move {
                        let profile_url = format!("{}/api/badge/profile", API_URLS.api);
                        match client_clone
                            .get(profile_url)
                            .bearer_auth(&stored_token)
                            .send()
                            .await
                        {
                            Ok(prof_res) => {
                                if prof_res.status().is_success() {
                                    if let Ok(raw_prof_text) = prof_res.text().await {
                                        if let Ok(profile_response) =
                                            serde_json::from_str::<ProfileResponse>(&raw_prof_text)
                                        {
                                            debug_log!("🟢 Profile synchronization complete!");
                                            let _ = tx_channel
                                                .send(ProfileMessage::ProfileHeaderLoaded(
                                                    profile_response.profile,
                                                ))
                                                .await;

                                            let limits_client = client_clone.clone();
                                            let limits_tx = tx_channel.clone();
                                            let db = LazySongDatabase::new(
                                                limits_client,
                                                Arc::new(dashmap::DashMap::new()),
                                                "".into(),
                                                shared_config,
                                            );

                                            match db.get_user_limits().await {
                                                Ok(limits) => {
                                                    let _ = limits_tx
                                                        .send(ProfileMessage::UserLimitsLoaded(
                                                            limits,
                                                        ))
                                                        .await;
                                                }
                                                Err(e) => {
                                                    debug_log!(
                                                        "❌ Failed to fetch user limits: {}",
                                                        e
                                                    );
                                                }
                                            }
                                        } else {
                                            debug_log!("❌ Failed to deserialize ProfileResponse");
                                        }
                                    }
                                } else {
                                    debug_log!(
                                        "🔴 Profile fetch failed with status: {}",
                                        prof_res.status()
                                    );
                                }
                            }
                            Err(e) => debug_log!("❌ Profile fetch collapsed: {}", e),
                        }
                        ctx_clone.request_repaint();
                    });
                }

                ProfileMessage::ProfileHeaderLoaded(header_data) => {
                    self.profile_activity.state.profile_data = Some(header_data);
                    self.profile_activity.try_load_avatar(ui.ctx(), &self.rt);
                    self.home_activity.fetch_suggested();
                    self.home_activity.fetch_trending();
                    self.home_activity.fetch_recent_setlist();
                }
                ProfileMessage::Logout => {
                    self.config.auth = None;
                    let _ = self.config.write_config();

                    self.cached_art_paths.clear();
                    self.active_art_downloads.clear();
                    self.cached_avatar_path = None;

                    self.songs.map.clear();

                    let fav_playlists = self.favorites_activity.playlists.clone();
                    let fav_songs = self.favorites_activity.songs.clone();
                    let fav_selected = self.favorites_activity.selected_playlist.clone();
                    let fav_fetching = self.favorites_activity.is_fetching.clone();
                    let fav_hashes = self.favorites_activity.favorite_songs.clone();

                    let public_playlists = self.playlist_activity.playlists.clone();
                    let public_selected = self.playlist_activity.selected_playlist.clone();

                    let private_playlists = self.my_playlist_activity.playlists.clone();
                    let private_selected = self.my_playlist_activity.selected_playlist.clone();

                    let home_suggested = self.home_activity.suggested_songs.clone();
                    let home_trending = self.home_activity.trending_songs.clone();
                    let home_setlist = self.home_activity.setlist_songs.clone();
                    let home_setlist_name = self.home_activity.setlist_name.clone();

                    if let Ok(mut token_guard) = self.shared_config.auth_token.write() {
                        *token_guard = None;
                    }
                    self.profile_activity.state.profile_data = None;

                    self.songs.map.clear();

                    let cache = self.cache.clone();
                    spawn(async move {
                        if let Err(e) = cache.clear_cache().await {
                            eprintln!("❌ Failed to fully wipe cache directory on logout: {}", e);
                        }

                        *fav_playlists.lock().await = LoadingState::Loading;
                        *fav_songs.lock().await = LoadingState::Loading;
                        *fav_selected.lock().await = None;
                        fav_fetching.store(false, std::sync::atomic::Ordering::SeqCst);
                        fav_hashes.write().await.clear();

                        *public_playlists.lock().await = LoadingState::Loading;
                        *public_selected.lock().await = None;

                        *private_playlists.lock().await = LoadingState::Loading;
                        *private_selected.lock().await = None;

                        *home_suggested.lock().await = LoadingState::Loading;
                        *home_trending.lock().await = LoadingState::Loading;
                        *home_setlist.lock().await = LoadingState::Loading;
                        *home_setlist_name.lock().await = None;
                    });
                }
                _ => {}
            }
            ui.ctx().request_repaint();
        }

        if self.theme.animate(ui.input(|i| i.stable_dt)) {
            ui.set_visuals(self.theme.visuals());
            ui.request_repaint();
        }

        // background gradient
        let rect = ui.max_rect();
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(rect.left_top(), self.theme.background_mid);
        mesh.colored_vertex(rect.right_top(), self.theme.background_secondary);
        mesh.colored_vertex(rect.right_bottom(), self.theme.background_mid);
        mesh.colored_vertex(rect.left_bottom(), self.theme.background);

        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);

        ui.painter().add(egui::Shape::mesh(mesh));

        // sidebar
        let mut temp_theme = self.config.theme;
        activity::sidebar::render_sidebar(
            ui,
            &mut self.config,
            &self.theme,
            &mut self.activity,
            &mut self.show_queue,
            &self.profile_activity,
            &mut temp_theme,
        );
        if self.config.theme != temp_theme {
            self.config.theme = temp_theme;
            self.theme.set(self.config.theme.as_theme());
        }

        if let Some(state) = self.player.get_playback_state() {
            if self.current_song_uuid != Some(state.song()) {
                self.current_song_uuid = Some(state.song());
                let mut song = match self.songs.get(&state.song(), |song| song.clone()) {
                    LoadingState::Loaded(s) => Some(s),
                    _ => None,
                };
                if song.is_none() {
                    if let Ok(meta) = self.player.current_url_metadata.lock() {
                        if let Some(meta) = &*meta
                            && meta.id == state.song()
                        {
                            song = Some(Song {
                                id: state.song(),
                                title: meta.title.clone(),
                                absolute_path: meta.audio_url.clone().map(|s| s.to_string().into()),
                                opus: None,
                                cover_artists: meta.cover_artists.clone(),
                                original_artists: meta.original_artists.clone(),
                                cover_art: meta.cover_art.clone(),
                                play_count: None,
                                duration: None,
                            });
                        }
                    }
                }
                if let Some(s) = song {
                    let artwork_url = s
                        .cover_art
                        .as_ref()
                        .and_then(|art| {
                            let key_str: Arc<str> = art
                                .cloudflare_id
                                .clone()
                                .map(|id| id.to_string())
                                .unwrap_or_else(|| art.absolute_path.to_string())
                                .into();
                            let target_uuid = Uuid::parse_str(&key_str).unwrap_or_else(|_| {
                                Uuid::new_v5(&Uuid::NAMESPACE_URL, key_str.as_bytes())
                            });

                            if let Some(path) = self
                                .cache
                                .get_cached_path(target_uuid, cache::AssetType::Image)
                            {
                                Some(path.to_string_lossy().into_owned())
                            } else {
                                self.resolve_artwork_uri(
                                    ui,
                                    art.cloudflare_id.clone(),
                                    art.absolute_path.clone(),
                                )
                            }
                        })
                        .unwrap_or_default();

                    let track = playwire::Track {
                        title: s.title.to_string(),
                        artists: s.original_artists.iter().map(|s| s.to_string()).collect(),
                        url: s.absolute_path.map(|p| p.to_string()).unwrap_or_default(),
                        id: s.id.to_string(),
                        album: self
                            .player
                            .get_playlist_name()
                            .unwrap_or_else(|| "".to_string()),
                        artwork_url,
                    };
                    self.integration.update_metadata(track);
                }
            }
            if self.current_playback_state.as_ref().map(|s| s.song()) != Some(state.song())
                || self.current_playback_state.as_ref().map(|s| s.paused()) != Some(state.paused())
            {
                self.current_playback_state = Some(state);
                self.update_os_playback();
            }
            render_player_controls(self, ui);
        }

        // central panel
        ui.push_id("central_panel", |ui| {
            CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
                .show(ui, |ui| {
                    ui.heading(self.activity.as_str());
                    ui.push_id(self.activity.as_str(), |ui| {
                        self.render_activity(ui);
                    });
                });
        });

        if ui.input(|i| i.focused) {
            ui.request_repaint();
        } else if self.config.framerate_when_not_focused > 0.0 {
            ui.request_repaint_after(Duration::from_micros(
                (1_000_000.0 / self.config.framerate_when_not_focused) as u64,
            ));
        }
    }

    fn on_exit(&mut self) {
        if let Some(state) = self.player.get_playback_state() {
            let app_state = persistence::AppState {
                current_song_uuid: Some(state.song()),
                current_position_secs: state.position().as_secs(),
                playlist: self.player.get_playlist(),
                playlist_name: self.player.get_playlist_name(),
                url_playlist: self.player.get_url_playlist(),
                volume: self.player.get_volume(),
                shuffle: self.player.get_shuffle(),
                loop_mode: self.player.get_loop_mode(),
            };
            let _ = persistence::save_app_state(&app_state);
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if let Err(e) = self.config.write_config() {
            debug_log!("config write failed: {}", e);
        }
    }
}

impl App {
    fn render_activity(&mut self, ui: &mut Ui) {
        if self.show_queue {
            let mut player1 = self.player.clone();
            let mut player2 = self.player.clone();
            let player_ref = self.player.clone();
            self.queue_activity.render(
                ui,
                move |playlist| {
                    let pl: Vec<Uuid> = playlist.songs.iter().map(|s| s.id).collect();
                    player1.clear_playlist();
                    player1.playlists(
                        Some(pl.clone().into()),
                        Some(playlist.songs.clone().into()),
                        Some(playlist.name.to_string()),
                    );
                    if let Some(first) = playlist.songs.first() {
                        player1.play_song(first.id);
                    }
                },
                move |songs, song_id| {
                    let pl: Vec<Uuid> = songs.iter().map(|s| s.id).collect();
                    player2.clear_playlist();
                    player2.playlists(Some(pl.clone().into()), Some(songs.into()), None);
                    player2.play_song(song_id);
                },
                &player_ref,
            );
        } else if self.activity == ActivityType::Home {
            let mut player1 = self.player.clone();
            let mut player2 = self.player.clone();
            let rt = self.rt.clone();
            self.home_activity.render(
                ui,
                &self.songs,
                &self.theme,
                move |state| {
                    let _ = persistence::clear_app_state();
                    player1.volume(state.volume);
                    player1.shuffle(state.shuffle);
                    player1.looping(state.loop_mode);
                    player1.playlists(state.playlist, state.url_playlist, state.playlist_name);
                    if let Some(song_id) = state.current_song_uuid {
                        player1.play_song(song_id);
                        let p = player1.clone();
                        let pos = state.current_position_secs;
                        rt.spawn(async move {
                            sleep(Duration::from_millis(1000)).await;
                            p.seek(Duration::from_secs(pos));
                        });
                    }
                },
                move |songs, song_id| {
                    let pl: Vec<Uuid> = songs.iter().map(|s| s.id).collect();
                    player2.clear_playlist();
                    player2.playlists(Some(pl.clone().into()), Some(songs.into()), None);
                    player2.play_song(song_id);
                },
            );
        } else if self.activity == ActivityType::Search {
            self.search_activity
                .render(ui, &self.songs, &mut self.player, &self.current_song_uuid);
        } else if self.activity == ActivityType::Playlists {
            let mut player1 = self.player.clone();
            let mut player2 = self.player.clone();
            self.playlist_activity.render(
                ui,
                &mut Some(&mut self.playlist_search),
                &mut self.current_playlist_sort,
                &mut self.current_playlist_sort_desc,
                move |detail| {
                    play_playlist(&mut player1, &detail.songs, &detail.name);
                },
                move |songs, song_id| {
                    play_song(&mut player2, songs, song_id, "Favorites");
                },
                false,
                None,
            );
        } else if self.activity == ActivityType::MyPlaylists {
            let mut player1 = self.player.clone();
            let mut player2 = self.player.clone();
            let username = self
                .config
                .auth
                .as_ref()
                .map(|a| a.user.username.to_string());
            self.my_playlist_activity.render(
                ui,
                &mut None,
                &mut self.current_playlist_sort,
                &mut self.current_playlist_sort_desc,
                move |detail| {
                    play_playlist(&mut player1, &detail.songs, &detail.name);
                },
                move |songs, song_id| {
                    play_song(&mut player2, songs, song_id, "Favorites");
                },
                true,
                username,
            );
        } else if self.activity == ActivityType::Favorites {
            let mut player1 = self.player.clone();
            let mut player2 = self.player.clone();
            self.favorites_activity.render(
                ui,
                &self.theme,
                &self.profile_activity,
                move |detail| {
                    play_playlist(&mut player1, &detail.songs, &detail.name);
                },
                move |songs, song_id| {
                    play_song(&mut player2, songs, song_id, "Favorites");
                },
            );
        } else if self.activity == ActivityType::Setlists {
            let mut player1 = self.player.clone();
            let mut player2 = self.player.clone();
            self.setlist_activity.render(
                ui,
                &self.theme,
                move |detail| {
                    play_playlist(&mut player1, &detail.songs, &detail.name);
                },
                move |songs, song_id| {
                    play_song(&mut player2, songs, song_id, "Setlist");
                },
            );
        } else if self.activity == ActivityType::Profile {
            let auth_service = AuthService::new(self.client.clone());
            self.profile_activity.render(
                ui,
                &self.theme,
                &self.config.auth,
                &auth_service,
                &self.rt,
            );
        }
    }
}

fn play_playlist(player: &mut Player, songs: &[SongDTO], name: &str) {
    debug_log!("Playlist '{}' has {} songs.", name, songs.len());
    let pl: Vec<Uuid> = songs.iter().map(|s| s.id).collect();
    player.clear_playlist();
    player.playlists(
        Some(pl.clone().into()),
        Some(songs.into()),
        Some(name.to_string()),
    );
    let _ = persistence::clear_app_state();

    if let Some(first_song) = songs.first() {
        player.url_playback(Some(pl[0]), first_song.clone(), Player::play);
    }
}

fn play_song(player: &mut Player, songs: Vec<SongDTO>, song_id: Uuid, name: &str) {
    let pl: Vec<Uuid> = songs.iter().map(|s| s.id).collect();
    player.clear_playlist();
    player.playlists(
        Some(pl.clone().into()),
        Some(songs.clone().into()),
        Some(name.to_string()),
    );
    let _ = persistence::clear_app_state();

    if let Some(index) = songs.iter().position(|s| s.id == song_id) {
        player.url_playback(Some(songs[index].id), songs[index].clone(), Player::play);
    }
}
