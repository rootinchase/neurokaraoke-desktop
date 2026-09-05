use eframe::egui::{self, Ui, RichText, Color32, Vec2, Frame};
use std::sync::Arc;
use crate::api::{AuthContext};
use crate::auth::{ discord::{capture_discord_token, NEURO_KARAOKE_DISCORD}};
use crate::auth::AuthService;
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::cache::{Cache};
use uuid::Uuid;

pub enum ProfileMessage {
    LoginSuccess(AuthContext),
    Logout,
    // FIX: Add this state feedback variant
    ProfileHeaderLoaded(crate::api::ProfileHeader),
    AvatarLoaded(String),
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
            },
            cache
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
                    },
                    ProfileMessage::AvatarLoaded(path) => {
                        if let Ok(bytes) = std::fs::read(path) {
                            self.state.avatar_state = AvatarState::Ready { bytes };
                        }
                    },
                    _ => {}
                }
                Some(msg)
            },
            _ => None,
        }
    }

    pub fn get_sender_handle(&self) -> tokio::sync::mpsc::Sender<ProfileMessage> {
        self.tx.clone()
    }

    // Resolution logic matching your existing Cloudflare Image variant criteria
    pub fn resolve_avatar_uri(&mut self, ctx: &egui::Context, rt: &tokio::runtime::Runtime, client: &reqwest::Client, avatar_url: &str) {
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
            match cache.get_or_download_image(&client_clone, avatar_uuid, final_url).await {
                Ok(path) => {
                    let path_str = path.to_string_lossy().into_owned();
                    let _ = tx.send(ProfileMessage::AvatarLoaded(path_str)).await;
                },
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
        rt: &Arc<tokio::runtime::Runtime>
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
                        ui.vertical(|ui| {
                            ui.heading(RichText::new(format!("Welcome, {}!", auth_ctx.user.username)).color(theme.primary));
                            ui.label(RichText::new(format!("User ID: {}", auth_ctx.user.id)).color(theme.text_muted).size(11.0));

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
                        crate::debug_log!("Initiating local OAuth loop thread...");

                        let rt_handle = rt.clone();
                        let auth_service_worker = auth_service.clone();
                        let tx_worker = self.tx.clone();
                        let ctx_clone = self.ctx.clone();

                        rt_handle.spawn(async move {
                            match capture_discord_token(&NEURO_KARAOKE_DISCORD).await {
                                Ok(discord_access_token) => {
                                    crate::debug_log!("Successfully captured raw access token. Exchanging for Neuro internal JWT...");

                                    match auth_service_worker.login_via_discord(&discord_access_token).await {
                                        Ok(auth_context) => {
                                            crate::debug_log!("Successfully logged in! Welcome, {}", auth_context.user.username);
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
