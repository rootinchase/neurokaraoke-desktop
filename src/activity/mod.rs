pub mod favorites;
pub mod home;
pub mod playlist;
pub mod profile;
pub mod queue;
pub mod radio;
pub mod search;
pub mod setlist;
pub mod settings;
pub mod sidebar;

use crate::api::{Artwork, Playlist, PlaylistDetail, SongDTO};
use crate::theme::ThemeManager;
use crate::utilities::cache::{self, PersistentMediaCache};
use crate::utilities::util::format_duration;
use eframe::egui::{
    Align2, Color32, Context, FontId, Frame, Grid, Image, ImageSource, Sense, Ui, Vec2,
    include_image, vec2,
};
use egui_extras::{Column, TableBuilder};
use reqwest::Client;
use std::fs::read;
use std::sync::Arc;
use tokio::runtime::Runtime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityType {
    Home,
    Search,
    Radio,
    Profile,
    Playlists,
    MyPlaylists,
    Setlists,
    Favorites,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOption {
    Name,
    Songs,
    Plays,
    Date,
}

impl ActivityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Radio => "Radio",
            Self::Profile => "Profile",
            Self::Playlists => "Public Playlists",
            Self::MyPlaylists => "My Playlists",
            Self::Setlists => "Official Setlists",
            Self::Favorites => "Favorites",
            Self::Settings => "Settings",
        }
    }

    pub fn icon(&self) -> Option<ImageSource<'static>> {
        match self {
            Self::Home => Some(include_image!("../../assets/home.svg")),
            Self::Search => Some(include_image!("../../assets/search.svg")),
            Self::Radio => Some(include_image!("../../assets/radio.svg")),
            Self::Playlists => Some(include_image!("../../assets/playlist.svg")),
            Self::MyPlaylists => Some(include_image!("../../assets/playlist.svg")),
            Self::Setlists => Some(include_image!("../../assets/setlist.svg")),
            Self::Favorites => Some(include_image!("../../assets/favorite.svg")),
            Self::Profile => Some(include_image!("../../assets/icon.png")),
            Self::Settings => Some(include_image!("../../assets/settings.svg")),
        }
    }
}

pub fn resolve_and_render_art(
    ui: &mut Ui,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    cover_art: &Artwork,
    size: Vec2,
) {
    let cloudflare_id = cover_art.cloudflare_id.as_deref().filter(|s| !s.is_empty());
    let artwork_url =
        cache::get_thumbnail_url(cloudflare_id, &cover_art.absolute_path, "crop,gravity=auto");
    if artwork_url.is_empty() {
        let (rect, _response) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, 4.0, Color32::from_gray(50));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "?",
            FontId::proportional(20.0),
            Color32::WHITE,
        );
        return;
    }

    let art_uuid_str = cloudflare_id.map(|s| s).unwrap_or(&cover_art.id);

    let art_uuid = Uuid::parse_str(art_uuid_str).unwrap_or_else(|_| Uuid::new_v4());

    // FIXED: Use the new helper method to safely query the persistent cache setup
    if let Some(path) = cache.get_cached_path(art_uuid, cache::AssetType::Image) {
        if let Ok(bytes) = read(&path) {
            ui.add(
                Image::from_bytes(format!("bytes://{}", path.display()), bytes)
                    .fit_to_exact_size(size),
            );
            return;
        }
    }

    let cache_clone = cache.clone();
    let _client_clone = client.clone();
    let ctx_clone = ctx.clone();
    let artwork_url_clone = artwork_url.clone();
    rt.spawn(async move {
        let _ = cache_clone
            .get_or_download_image(art_uuid, artwork_url_clone)
            .await;
        ctx_clone.request_repaint();
    });

    // Provide a more visible fallback when art is loading or missing
    let (rect, _response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 4.0, Color32::from_gray(50));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "?",
        FontId::proportional(20.0),
        Color32::WHITE,
    );
}

pub fn render_mosaic(
    ui: &mut Ui,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    id: &str,
    songs_list: &[SongDTO],
) {
    let size = Vec2::new(150.0, 150.0);
    let valid_covers: Vec<&Artwork> = songs_list
        .iter()
        .filter_map(|s| s.cover_art.as_ref())
        .collect();

    if valid_covers.is_empty() {
        let (rect, _response) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, 4.0, Color32::from_gray(50));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            "?",
            FontId::proportional(20.0),
            Color32::WHITE,
        );
        return;
    }

    let half_horizontal = vec2(size[0], size[1] / 2.0);

    match valid_covers.len() {
        1 => {
            resolve_and_render_art(ui, cache, ctx, rt, client, valid_covers[0], size);
            return;
        }
        2 => {
            Grid::new(format!("{} songs mosaic", id))
                .spacing(Vec2::new(0.0, 1.0))
                .show(ui, |ui| {
                    for i in 0..2 {
                        let cover = valid_covers[i % valid_covers.len()];
                        resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_horizontal);
                        ui.end_row();
                    }
                });
            return;
        }
        3 => {
            let third_horizontal = vec2(size[0], size[1] / 3.0);
            Grid::new(format!("{} songs mosaic", id))
                .spacing(Vec2::new(0.0, 1.0))
                .show(ui, |ui| {
                    for i in 0..3 {
                        let cover = valid_covers[i % valid_covers.len()];
                        resolve_and_render_art(ui, cache, ctx, rt, client, cover, third_horizontal);
                        ui.end_row();
                    }
                });
            return;
        }
        _ => {
            Grid::new(format!("{} songs mosaic", id))
                .spacing(Vec2::new(2.0, 2.0))
                .show(ui, |ui| {
                    for i in 0..4 {
                        let cover = valid_covers[i % valid_covers.len()];
                        resolve_and_render_art(ui, cache, ctx, rt, client, cover, size / 2.0);
                        if i == 1 {
                            ui.end_row();
                        }
                    }
                });
            return;
        }
    }
}

pub fn render_playlist_table(
    ui: &mut Ui,
    id: &str,
    playlist: &PlaylistDetail,
    on_play_song: &mut impl FnMut(Vec<SongDTO>, Uuid),
) {
    let table = TableBuilder::new(ui)
        .id_salt(format!("{}-songs", id))
        .column(Column::remainder())
        .column(Column::exact(60.0))
        .column(Column::exact(100.0))
        .column(Column::exact(60.0))
        .header(20.0, |mut header| {
            header.col(|ui| {
                ui.label("Song");
            });
            header.col(|ui| {
                ui.label("Plays");
            });
            header.col(|ui| {
                ui.label("Date");
            });
            header.col(|ui| {
                ui.label("Dur");
            });
        });

    let songs_list = &playlist.songs;
    table.body(|body| {
        body.rows(20.0, songs_list.len(), |mut row| {
            let song = &songs_list[row.index()];
            let song_id = song.id;
            let songs_list_clone = songs_list.clone();

            row.col(|ui| {
                if ui
                    .selectable_label(
                        false,
                        format!("{} - {}", song.original_artists.join(" & "), song.title),
                    )
                    .clicked()
                {
                    on_play_song(songs_list_clone, song_id);
                }
            });

            row.col(|ui| {
                ui.label(song.play_count.map(|c| c.to_string()).unwrap_or_default());
            });
            row.col(|ui| {
                ui.label(
                    song.stream_date
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_default(),
                );
            });
            row.col(|ui| {
                if let Some(dur) = song.duration {
                    ui.label(format_duration(dur));
                } else {
                    ui.label("-");
                }
            });
        });
    });
}

fn collapse_word_repetitions(title: &str) -> String {
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.len() < 4 {
        return title.to_string();
    }

    for sub_len in 1..=(words.len() / 2) {
        let mut i = 0;
        let mut repeat_count = 0;
        while i + sub_len <= words.len() {
            let chunk = &words[i..i + sub_len];
            let next_chunk = if i + 2 * sub_len <= words.len() {
                Some(&words[i + sub_len..i + 2 * sub_len])
            } else {
                None
            };

            if let Some(nc) = next_chunk {
                if chunk == nc {
                    repeat_count += 1;
                    i += sub_len;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        if repeat_count >= 1 {
            let unique_words = &words[..sub_len];
            let combined = unique_words.join(" ");
            if combined.len() < title.len() {
                return format!("{}...", combined);
            }
        }
    }

    title.to_string()
}

fn detect_and_collapse_repetition(title: &str) -> String {
    let chars: Vec<char> = title.chars().collect();
    let n = chars.len();
    if n < 6 {
        return title.to_string();
    }

    let mut pi = vec![0; n];
    let mut j = 0;
    for i in 1..n {
        while j > 0 && chars[i] != chars[j] {
            j = pi[j - 1];
        }
        if chars[i] == chars[j] {
            j += 1;
        }
        pi[i] = j;
    }

    let len_pi = pi[n - 1];
    if len_pi > 0 {
        let k = n - len_pi;
        if (n % k == 0 || n % k < k) && n / k >= 2 && k >= 3 {
            let unit: String = chars[..k].iter().collect();
            return format!("{}...", unit.trim());
        }
    }

    title.to_string()
}

fn truncate_title(title: &str) -> String {
    let cleaned = collapse_word_repetitions(title);
    let cleaned = detect_and_collapse_repetition(&cleaned);

    let is_cuneiform = cleaned.chars().any(|c| {
        let code = c as u32;
        (0x12000..=0x12543).contains(&code)
    });

    let max_len = if is_cuneiform { 10 } else { 50 };

    if cleaned.chars().count() > max_len {
        format!("{}...", cleaned.chars().take(max_len).collect::<String>())
    } else {
        cleaned
    }
}

pub fn render_playlist_box(
    ui: &mut Ui,
    theme: &ThemeManager,
    id: &str,
    title: &str,
    label: &str,
    playlist: &PlaylistDetail,
    on_play_playlist: &mut impl FnMut(Vec<SongDTO>, Uuid),
    render_mosaic: impl FnOnce(&mut Ui),
) {
    Frame::new()
        .fill(theme.background_elevated)
        .corner_radius(8.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(170.0);
                    render_mosaic(ui); // Mosaic logic provided by activity
                    ui.heading(truncate_title(title));
                    ui.label(label);
                });

                ui.add_space(20.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.set_height(200.0);
                    render_playlist_table(ui, id, playlist, on_play_playlist);
                });
            });
        });
}

pub fn render_playlist_art_advanced(
    ui: &mut Ui,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    playlist: &Playlist,
    songs: &[SongDTO],
    size: Vec2,
) {
    let media_valid = playlist.media.as_ref().map_or(false, |art| {
        let cloudflare_id = art
            .cloudflare_id
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "null");
        let abs = art.absolute_path.trim();
        let url = cache::get_thumbnail_url(cloudflare_id, abs, "crop,gravity=auto");
        !url.is_empty()
    });

    if media_valid {
        if let Some(art) = &playlist.media {
            resolve_and_render_art(ui, cache, ctx, rt, client, art, size);
            return;
        }
    }

    let half_vertical = vec2(size[0] / 2.0, size[1]);
    let half_horizontal = vec2(size[0], size[1] / 2.0);

    let mosaic_valid = playlist
        .mosaic_media
        .as_ref()
        .map_or(false, |m| !m.is_empty());
    if mosaic_valid {
        if let Some(mosaic) = &playlist.mosaic_media {
            let valid_mosaic: Vec<&Artwork> = mosaic.iter().collect();
            if !valid_mosaic.is_empty() {
                match valid_mosaic.len() {
                    1 => {
                        resolve_and_render_art(ui, cache, ctx, rt, client, valid_mosaic[0], size);
                        return;
                    }
                    2 => {
                        Grid::new(format!("{} songs mosaic", playlist.id))
                            .spacing(Vec2::new(1.0, 2.0))
                            .show(ui, |ui| {
                                for i in 0..2 {
                                    let cover = valid_mosaic[i % valid_mosaic.len()];
                                    resolve_and_render_art(
                                        ui,
                                        cache,
                                        ctx,
                                        rt,
                                        client,
                                        cover,
                                        half_vertical,
                                    );
                                }
                            });
                        return;
                    }
                    3 => {
                        Grid::new(format!("{} songs mosaic - p1", playlist.id))
                            .spacing(Vec2::new(1.0, 2.0))
                            .show(ui, |ui| {
                                for i in 0..2 {
                                    let cover = valid_mosaic[i % valid_mosaic.len()];
                                    resolve_and_render_art(
                                        ui,
                                        cache,
                                        ctx,
                                        rt,
                                        client,
                                        cover,
                                        size / 2.0,
                                    );
                                }
                            });

                        Grid::new(format!("{} songs mosaic-p2", playlist.id))
                            .spacing(Vec2::new(1.0, 1.0))
                            .show(ui, |ui| {
                                let cover = valid_mosaic[2];
                                resolve_and_render_art(
                                    ui,
                                    cache,
                                    ctx,
                                    rt,
                                    client,
                                    cover,
                                    half_horizontal,
                                );
                            });
                        return;
                    }
                    _ => {
                        Grid::new(format!("{} songs mosaic", playlist.id))
                            .spacing(Vec2::new(2.0, 2.0))
                            .show(ui, |ui| {
                                for i in 0..4 {
                                    let cover = valid_mosaic[i % valid_mosaic.len()];
                                    resolve_and_render_art(
                                        ui,
                                        cache,
                                        ctx,
                                        rt,
                                        client,
                                        cover,
                                        size / 2.0,
                                    );
                                    if i == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                        return;
                    }
                }
            }
        }
    }

    let valid_song_covers: Vec<&Artwork> =
        songs.iter().filter_map(|s| s.cover_art.as_ref()).collect();
    if !valid_song_covers.is_empty() {
        match valid_song_covers.len() {
            1 => {
                resolve_and_render_art(ui, cache, ctx, rt, client, valid_song_covers[0], size);
                return;
            }
            2 => {
                Grid::new(format!("{} songs mosaic", playlist.id))
                    .spacing(Vec2::new(1.0, 2.0))
                    .show(ui, |ui| {
                        for i in 0..2 {
                            let cover = valid_song_covers[i % valid_song_covers.len()];
                            resolve_and_render_art(
                                ui,
                                cache,
                                ctx,
                                rt,
                                client,
                                cover,
                                half_vertical,
                            );
                        }
                    });
                return;
            }
            3 => {
                Grid::new(format!("{} songs mosaic-p1", playlist.id))
                    .spacing(Vec2::new(1.0, 2.0))
                    .show(ui, |ui| {
                        for i in 0..2 {
                            let cover = valid_song_covers[i % valid_song_covers.len()];
                            resolve_and_render_art(ui, cache, ctx, rt, client, cover, size / 2.0);
                        }
                    });
                Grid::new(format!("{} songs mosaic-p2", playlist.id))
                    .spacing(Vec2::new(1.0, 1.0))
                    .show(ui, |ui| {
                        let cover = valid_song_covers[2];
                        resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_horizontal);
                    });

                return;
            }
            _ => {
                Grid::new(format!("{} songs mosaic", playlist.id))
                    .spacing(Vec2::new(2.0, 2.0))
                    .show(ui, |ui| {
                        for i in 0..4 {
                            let cover = valid_song_covers[i % valid_song_covers.len()];
                            resolve_and_render_art(ui, cache, ctx, rt, client, cover, size / 2.0);
                            if i == 1 {
                                ui.end_row();
                            }
                        }
                    });
                return;
            }
        }
    }

    let (rect, _response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 4.0, Color32::from_gray(50));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        "?",
        FontId::proportional(20.0),
        Color32::WHITE,
    );
}

pub fn render_playlist_art(
    ui: &mut Ui,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    playlist: &Playlist,
    size: Vec2,
) {
    render_playlist_art_advanced(ui, cache, ctx, rt, client, playlist, &[], size);
}
