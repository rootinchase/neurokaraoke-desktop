use crate::api::AuthContext;
use crate::auth::AuthService;
use crate::auth::discord::{NEURO_KARAOKE_DISCORD, capture_discord_token};
use crate::cache::Cache;
use crate::debug_log;
use crate::theme::ThemeManager;
use eframe::egui::{self, Color32, Frame, RichText, Ui, Vec2, include_image};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub enum ProfileMessage {
    LoginSuccess(AuthContext),
    Logout,
    // FIX: Add this state feedback variant
    ProfileHeaderLoaded(crate::api::ProfileHeader),
    AvatarLoaded(String),
    UserLimitsLoaded(crate::api::UserLimits),
    BadgesLoaded(Vec<crate::api::Badge>),
    BadgeImageLoaded(String, String),
}

pub struct ProfileActivity {
    ctx: egui::Context,
    tx: tokio::sync::mpsc::Sender<ProfileMessage>,
    rx: tokio::sync::mpsc::Receiver<ProfileMessage>,
    pub state: ProfileState,
    cache: Arc<Cache>,
}

pub enum AvatarState {
    None,
    Downloading,
    Ready { bytes: Vec<u8> },
}

pub struct ProfileState {
    pub profile_data: Option<crate::api::ProfileHeader>,
    pub avatar_state: AvatarState,
    pub user_limits: Option<crate::api::UserLimits>,
    pub badges: Vec<crate::api::Badge>,
    pub badge_images: HashMap<String, AvatarState>,
}

impl ProfileActivity {
    pub fn new(ctx: egui::Context, cache: Arc<Cache>) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        Self {
            ctx,
            tx,
            rx,
            state: ProfileState {
                profile_data: None,
                avatar_state: AvatarState::None,
                user_limits: None,
                badges: Vec::new(),
                badge_images: HashMap::new(),
            },
            cache,
        }
    }

    /// Pulls pending messages from background authentication threads.
    /// Returns an action token to safely apply adjustments to global configuration files.
    pub fn poll_messages(&mut self) -> Option<ProfileMessage> {
        match self.rx.try_recv() {
            Ok(msg) => {
                match &msg {
                    ProfileMessage::ProfileHeaderLoaded(data) => {
                        self.state.profile_data = Some(data.clone());
                    }
                    ProfileMessage::AvatarLoaded(path) => {
                        if let Ok(bytes) = std::fs::read(path) {
                            self.state.avatar_state = AvatarState::Ready { bytes };
                        }
                    }
                    ProfileMessage::UserLimitsLoaded(limits) => {
                        debug_log!(
                            "Received UserLimits: max_songs={}, max_storage_bytes={}, max_playlists={}, song_per_playlist_limit={}",
                            limits.max_songs,
                            limits.max_storage_bytes,
                            limits.playlist_limit,
                            limits.song_per_playlist_limit
                        );
                        self.state.user_limits = Some(limits.clone());
                    }
                    ProfileMessage::BadgesLoaded(badges) => {
                        self.state.badges = badges.clone();
                    }
                    ProfileMessage::BadgeImageLoaded(badge_id, path) => {
                        if let Ok(bytes) = std::fs::read(path) {
                            self.state
                                .badge_images
                                .insert(badge_id.clone(), AvatarState::Ready { bytes });
                        }
                    }
                    _ => {}
                }
                Some(msg)
            }
            _ => None,
        }
    }

    pub fn get_sender_handle(&self) -> tokio::sync::mpsc::Sender<ProfileMessage> {
        self.tx.clone()
    }

    pub fn resolve_badge_image(
        &mut self,
        ctx: &egui::Context,
        rt: &tokio::runtime::Runtime,
        client: &reqwest::Client,
        badge_id: &str,
        image_path: &str,
    ) {
        if self.state.badge_images.contains_key(badge_id) {
            return;
        }

        self.state
            .badge_images
            .insert(badge_id.to_string(), AvatarState::Downloading);

        let final_url = format!("https://images.neurokaraoke.com/{}/public", image_path);

        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let cache = self.cache.clone();
        let client_clone = client.clone();
        let badge_id_owned = badge_id.to_string();

        rt.spawn(async move {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&badge_id_owned, &mut hasher);
            let hash_val = std::hash::Hasher::finish(&hasher);
            let badge_uuid = Uuid::from_u128(hash_val as u128);

            match cache
                .get_or_download_image(&client_clone, badge_uuid, final_url)
                .await
            {
                Ok(path) => {
                    let path_str = path.to_string_lossy().into_owned();
                    let _ = tx
                        .send(ProfileMessage::BadgeImageLoaded(badge_id_owned, path_str))
                        .await;
                }
                Err(e) => {
                    debug_log!("❌ Failed to download badge image: {}", e);
                }
            }
            ctx_clone.request_repaint();
        });
    }

    // Resolution logic matching your existing Cloudflare Image variant criteria
    pub fn resolve_avatar_uri(
        &mut self,
        ctx: &egui::Context,
        rt: &tokio::runtime::Runtime,
        client: &reqwest::Client,
        avatar_url: &str,
    ) {
        if matches!(self.state.avatar_state, AvatarState::Downloading) {
            return;
        }

        self.state.avatar_state = AvatarState::Downloading;

        // 1. Determine if the path is fully qualified or needs a Cloudflare public variant suffix
        let final_url = if avatar_url.starts_with("http://") || avatar_url.starts_with("https://") {
            avatar_url.to_string()
        } else {
            format!("https://neurokaraoke.com{}/public", avatar_url)
        };
        debug_log!("Fetching User avatar from : {}", avatar_url);

        // Generate a stable key for the avatar
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&avatar_url, &mut hasher);
        let hash_val = std::hash::Hasher::finish(&hasher);
        let avatar_uuid = Uuid::from_u128(hash_val as u128);

        // Use your application cache / temporary state paths to spawn a clean task
        // and call `ctx.request_repaint()` inside the runtime closure when the download finishes.
        let tx = self.tx.clone();
        let ctx_clone = ctx.clone();
        let cache = self.cache.clone();
        let client_clone = client.clone();

        rt.spawn(async move {
            match cache
                .get_or_download_image(&client_clone, avatar_uuid, final_url)
                .await
            {
                Ok(path) => {
                    let path_str = path.to_string_lossy().into_owned();
                    let _ = tx.send(ProfileMessage::AvatarLoaded(path_str)).await;
                }
                Err(e) => {
                    debug_log!("❌ Failed to download avatar: {}", e);
                }
            }
            ctx_clone.request_repaint();
        });
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        theme: &ThemeManager,
        current_auth: &Option<AuthContext>,
        auth_service: &AuthService,
        rt: &Arc<tokio::runtime::Runtime>,
        client: &reqwest::Client,
    ) {
        ui.add_space(20.0);

        match current_auth {
            Some(auth_ctx) => {
                // 🟢 STATE: User is Logged In
                Frame::new()
                    .fill(theme.background_elevated)
                    .corner_radius(12.0)
                    .inner_margin(20.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let avatar_url = self.state.profile_data.as_ref().and_then(|p| p.avatar_url.clone());
                            
                            if let Some(avatar_url) = avatar_url {
                                match &self.state.avatar_state {
                                    AvatarState::Ready { bytes } => {
                                        let uri = format!("bytes://avatar_{}.jpeg", avatar_url);
                                        ui.add(
                                            egui::Image::from_bytes(uri, bytes.clone())
                                                .fit_to_exact_size(Vec2::new(64.0, 64.0))
                                                .corner_radius(32.0)
                                                .texture_options(egui::TextureOptions::LINEAR),
                                        );
                                    }
                                    AvatarState::Downloading => {
                                        let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 64.0), egui::Sense::hover());
                                        ui.painter().rect_filled(rect, 32.0, theme.background_elevated);
                                    }
                                    AvatarState::None => {
                                        self.resolve_avatar_uri(ui.ctx(), rt, client, &avatar_url);
                                        let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 64.0), egui::Sense::hover());
                                        ui.painter().rect_filled(rect, 32.0, theme.background_elevated);
                                    }
                                }
                            } else {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new("👤").size(64.0)
                                    )
                                );
                            }

                            ui.add_space(10.0);

                            ui.vertical(|ui| {
                                ui.heading(
                                    RichText::new(format!("Welcome, {}!", auth_ctx.user.username))
                                        .color(theme.primary),
                                );
                                ui.label(
                                    RichText::new(format!("User ID: {}", auth_ctx.user.id))
                                        .color(theme.text_muted)
                                        .size(11.0),
                                );
                            });
                        });

                        ui.add_space(15.0);
                        ui.separator();
                        ui.add_space(15.0);

                        if let Some(profile_data) = &self.state.profile_data {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("Level {}", profile_data.level)).strong().size(14.0));
                                if let Some(level_title) = &profile_data.level_title {
                                    ui.label(RichText::new(level_title).color(theme.text_muted).size(12.0));
                                }
                            });

                            ui.add_space(4.0);
                            let progress = profile_data.level_progress.unwrap_or(0.0) as f32;
                            let xp_text = format!("{}/{}", profile_data.total_xp, profile_data.total_xp + profile_data.xp_to_next_level);
                            let bar = egui::ProgressBar::new(progress)
                                .text(xp_text)
                                .fill(theme.primary);
                            ui.add(bar);
                            
                            //debug_log!("Rendering coin balances. Data: Neuro={}, Evil={}, Twins={}", profile_data.neuro_coin, profile_data.evil_coin, profile_data.twins_coin);
                            ui.add_space(15.0);
                            ui.horizontal(|ui| {
                                let coin_size = Vec2::new(24.0, 24.0);

                                ui.add(egui::Image::new(include_image!("../../assets/coin-neuros.png")).fit_to_exact_size(coin_size));
                                ui.label(RichText::new(format!("{}", profile_data.neuro_coin)).size(14.0));

                                ui.add_space(10.0);
                                ui.add(egui::Image::new(include_image!("../../assets/coin-evil.png")).fit_to_exact_size(coin_size));
                                ui.label(RichText::new(format!("{}", profile_data.evil_coin)).size(14.0));

                                ui.add_space(10.0);
                                ui.add(egui::Image::new(include_image!("../../assets/coin-twins.png")).fit_to_exact_size(coin_size));
                                ui.label(RichText::new(format!("{}", profile_data.twins_coin)).size(14.0));
                            });

                            if let Some(limits) = &self.state.user_limits {
                                ui.add_space(15.0);
                                ui.separator();
                                ui.add_space(10.0);
                                ui.label(RichText::new("Account Limits").strong().size(14.0));
                                ui.label(format!("Songs: {} / {}", limits.current_song_count, limits.max_songs));
                                ui.label(format!("Storage: {:.2} MB / {:.2} MB", limits.used_storage_bytes as f64 / 1024.0 / 1024.0, limits.max_storage_bytes as f64 / 1024.0 / 1024.0));
                                ui.label(format!("Playlists: {} / {}", limits.current_playlist_count, limits.playlist_limit));
                                ui.label(format!("Songs per Playlist: {}", limits.song_per_playlist_limit));
                                }

                                ui.add_space(15.0);
                                ui.separator();
                                ui.add_space(10.0);
                                ui.label(RichText::new("Badges").strong().size(14.0));
                                ui.add_space(5.0);

                                let column_width = ui.available_width() / 4.0;

                                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                                    egui::Grid::new("badges_grid")
                                        .num_columns(4)
                                        .min_col_width(column_width - 10.0) // Subtract spacing
                                        .spacing(Vec2::new(10.0, 10.0))
                                        .show(ui, |ui| {
                                            let mut badges_to_download = Vec::new();
                                            for badge in &self.state.badges {
                                                if badge.unlocked {
                                                    if !self.state.badge_images.contains_key(&badge.id) {
                                                        if let Some(media) = &badge.media {
                                                            badges_to_download.push((badge.id.clone(), media.cloudflare_id.clone()));
                                                        }
                                                    }
                                                }
                                            }

                                            for (badge_id, cloudflare_id) in badges_to_download {
                                                self.resolve_badge_image(ui.ctx(), rt, client, &badge_id, &cloudflare_id);
                                            }

                                            let mut count = 0;
                                            for badge in &self.state.badges {
                                                if badge.unlocked {
                                                    let badge_id = &badge.id;
                                                    let badge_state = self.state.badge_images.get(badge_id);

                                                    let border_color = match badge.rarity {
                                                        0 => theme.text_secondary,
                                                        1 => theme.primary,
                                                        2 => theme.accent,
                                                        3 => theme.accent_light,
                                                        _ => theme.text_muted,
                                                    };
                                                    
                                                    let badge_size = column_width * 0.6;
                                                    let radius = badge_size / 2.0;

                                                    let frame = Frame::new()
                                                        .fill(theme.background_elevated)
                                                        .corner_radius(8.0)
                                                        .inner_margin(5.0);

                                                    frame.show(ui, |ui| {
                                                        ui.vertical_centered(|ui| {
                                                            match badge_state {
                                                                Some(AvatarState::Ready { bytes }) => {
                                                                    let uri = format!("bytes://badge_{}.png", badge_id);
                                                                    
                                                                    Frame::new()
                                                                        .corner_radius(radius)
                                                                        .stroke(egui::Stroke::new(2.0, border_color))
                                                                        .show(ui, |ui| {
                                                                            ui.add_sized(Vec2::splat(badge_size), 
                                                                                egui::Image::from_bytes(uri, bytes.clone())
                                                                                    .fit_to_exact_size(Vec2::splat(badge_size))
                                                                                    .corner_radius(radius)
                                                                                    .texture_options(egui::TextureOptions::LINEAR)
                                                                            );
                                                                        });
                                                                }
                                                                _ => {
                                                                    Frame::new()
                                                                        .corner_radius(radius)
                                                                        .stroke(egui::Stroke::new(2.0, border_color))
                                                                        .show(ui, |ui| {
                                                                            ui.add_sized(Vec2::splat(badge_size), egui::Label::new(RichText::new("🏅").size(32.0)));
                                                                        });
                                                                }
                                                            }
                                                            ui.add_space(5.0); // Spacing between icon and label
                                                            ui.label(RichText::new(&badge.name).size(10.0));
                                                        });
                                                    });
                                                    count += 1;
                                                    if count % 4 == 0 {
                                                        ui.end_row();
                                                    }
                                                }
                                            }
                                        });
                                });
                                } else {
                                    debug_log!("profile_data is None, not rendering level/coin info.");
                                }

                                ui.add_space(15.0);
                                ui.separator();
                                ui.add_space(15.0);

                                let logout_btn = egui::Button::new(RichText::new("Log Out").size(14.0))
                                .fill(theme.error)
                                .min_size(Vec2::new(120.0, 32.0));

                                if ui.add(logout_btn).clicked() {
                                let _ = self.tx.try_send(ProfileMessage::Logout);
                                }
                                });
            }
            None => {
                // 🔴 STATE: User is Logged Out
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.label(RichText::new("Sign in to access your custom user library, playlists, and badges.").color(theme.text_secondary).size(16.0));
                    ui.add_space(20.0);

                    let login_btn = egui::Button::new(RichText::new("Log In with Discord").size(16.0).color(Color32::WHITE))
                        .fill(Color32::from_rgb(0x58, 0x65, 0xF2)) // Discord Blurple
                        .min_size(Vec2::new(260.0, 48.0))
                        .corner_radius(8.0);

                    if ui.add(login_btn).clicked() {
                        debug_log!("Initiating local OAuth loop thread...");

                        let rt_handle = rt.clone();
                        let auth_service_worker = auth_service.clone();
                        let tx_worker = self.tx.clone();
                        let ctx_clone = self.ctx.clone();

                        rt_handle.spawn(async move {
                            match capture_discord_token(&NEURO_KARAOKE_DISCORD).await {
                                Ok(discord_access_token) => {
                                    debug_log!("Successfully captured raw access token. Exchanging for Neuro internal JWT...");

                                    match auth_service_worker.login_via_discord(&discord_access_token).await {
                                        Ok(auth_context) => {
                                            debug_log!("Successfully logged in! Welcome, {}", auth_context.user.username);
                                            let _ = tx_worker.send(ProfileMessage::LoginSuccess(auth_context)).await;
                                        }
                                        Err(err) => {
                                            eprintln!("Failed to trade token via Neuro Karaoke provider gate: {}", err);
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!("OAuth transaction loop collapsed: {}", e);
                                }
                            }
                            ctx_clone.request_repaint();
                        });
                    }
                });
            }
        }
    }
}
