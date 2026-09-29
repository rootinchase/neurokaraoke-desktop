use crate::activity::profile::{ProfileMessage, ProfileState};
use crate::auth::discord::{capture_discord_token, NEURO_KARAOKE_DISCORD};
use crate::auth::AuthService;
use crate::theme::ThemeManager;
use eframe::egui::{Button, Frame, RichText, TextEdit, Ui, Vec2};
use std::sync::Arc;

pub fn render_login_form(
    ui: &mut Ui,
    theme: &ThemeManager,
    state: &mut ProfileState,
    auth_service: &AuthService,
    tx: tokio::sync::mpsc::Sender<ProfileMessage>,
    rt: &Arc<tokio::runtime::Runtime>,
) {
    let ctx = ui.ctx().clone();
    Frame::new()
        .fill(theme.background_elevated)
        .corner_radius(12.0)
        .inner_margin(24.0)
        .show(ui, |ui| {
            ui.set_max_width(400.0);
            ui.vertical_centered(|ui| {
                ui.heading(RichText::new("Sign In to NeuroKaraoke").size(22.0).strong());
                ui.add_space(15.0);

                ui.horizontal(|ui| {
                    ui.label("Username:");
                    ui.add(TextEdit::singleline(&mut state.username_input).desired_width(250.0));
                });
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.add(
                        TextEdit::singleline(&mut state.password_input)
                            .password(true)
                            .desired_width(250.0),
                    );
                });
                ui.add_space(20.0);

                let login_btn = Button::new(RichText::new("Login").size(16.0))
                    .fill(theme.primary)
                    .min_size(Vec2::new(150.0, 36.0));

                if ui.add(login_btn).clicked() {
                    let username = state.username_input.clone();
                    let password = state.password_input.clone();
                    let auth_service = auth_service.clone();
                    let tx = tx.clone();
                    let ctx_clone = ctx.clone();

                    rt.spawn(async move {
                        let req = crate::api::LoginRequest {
                            username: username.into(),
                            password: password.into(),
                        };
                        match auth_service.login(&req).await {
                            Ok(auth_ctx) => {
                                let _ = tx.send(ProfileMessage::LoginSuccess(auth_ctx)).await;
                            }
                            Err(e) => {
                                crate::debug_log!("❌ Login failed: {}", e);
                            }
                        }
                        ctx_clone.request_repaint();
                    });
                }

                ui.add_space(15.0);
                ui.separator();
                ui.add_space(15.0);

                let discord_btn = Button::new(RichText::new("Sign in with Discord").size(15.0))
                    .fill(theme.accent)
                    .min_size(Vec2::new(200.0, 36.0));

                if ui.add(discord_btn).clicked() {
                    let auth_service = auth_service.clone();
                    let tx = tx.clone();
                    let ctx_clone = ctx.clone();

                    rt.spawn(async move {
                        match capture_discord_token(&NEURO_KARAOKE_DISCORD).await {
                            Ok(discord_token) => {
                                match auth_service.login_via_discord(&discord_token).await {
                                    Ok(auth_ctx) => {
                                        let _ = tx.send(ProfileMessage::LoginSuccess(auth_ctx)).await;
                                    }
                                    Err(e) => {
                                        crate::debug_log!("❌ Discord login verification failed: {}", e);
                                    }
                                }
                            }
                            Err(e) => {
                                crate::debug_log!("❌ Discord OAuth capture failed: {}", e);
                            }
                        }
                        ctx_clone.request_repaint();
                    });
                }
            });
        });
}
