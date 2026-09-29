pub mod favorites;
pub mod home;
pub mod playlist;
pub mod profile;
pub mod queue;
pub mod search;
pub mod setlist;
pub mod sidebar;

use crate::api::{Artwork, LoadingState, Playlist, PlaylistDetail, SongDTO};
use crate::theme::ThemeManager;
use crate::utilities::cache::{self, PersistentMediaCache};
use crate::utilities::util::format_duration;
use eframe::egui::{include_image, vec2, Align2, Color32, Context, FontId, Frame, Grid, Image, ImageSource, Sense, Ui, Vec2};
use egui_extras::{Column, TableBuilder};
use reqwest::Client;
use std::fs::read;
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use uuid::Uuid;

pub fn render_song_list(
    ui: &mut Ui,
    theme: &ThemeManager,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    id: &str,
    title: &str,
    label: &str,
    songs_state: &Mutex<LoadingState<Vec<SongDTO>>>,
    on_play_suggested: &mut impl FnMut(Vec<SongDTO>, Uuid),
    playlist_art: Option<Artwork>,
) {
    let songs_list = songs_state.blocking_lock();

    let playlist = match &*songs_list {
        LoadingState::Loaded(songs) => Some(PlaylistDetail {
            name: title.to_string().into(),
            songs: songs.clone(),
        }),
        _ => None,
    };

    if let Some(playlist) = playlist {
        render_playlist_box(
            ui,
            theme,
            id,
            title,
            label,
            &playlist,
            on_play_suggested,
            |ui| {
                if let Some(art) = &playlist_art {
                    resolve_and_render_art(
                        ui,
                        cache,
                        ctx,
                        rt,
                        client,
                        art,
                        Vec2::new(150.0, 150.0),
                    );
                } else {
                    render_mosaic(ui, cache, ctx, rt, client, id, &playlist.songs);
                }
            },
        );
    } else {
        // Handle loading/failed states if needed, currently duplicating the old box structure for consistency
        Frame::new()
            .fill(theme.background_elevated)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(170.0);
                        ui.allocate_exact_size(Vec2::new(150.0, 150.0), Sense::hover());
                        ui.heading(truncate_title(title));
                        ui.label(label);
                    });

                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(200.0);

                        // Placeholder table or indicator for loading/failed
                        ui.label(match &*songs_list {
                            LoadingState::Loading => "Loading...",
                            LoadingState::Failed(_) => "Failed to load.",
                            _ => "",
                        });
                    });
                });
            });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityType {
    Home,
    Search,
    Profile,
    Playlists,
    MyPlaylists,
    Setlists,
    Favorites,
    Queue,
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
            Self::Profile => "Profile",
            Self::Playlists => "Public Playlists",
            Self::MyPlaylists => "My Playlists",
            Self::Setlists => "Official Setlists",
            Self::Favorites => "Favorites",
            Self::Queue => "Queue",
        }
    }

    pub fn icon(&self) -> Option<ImageSource<'static>> {
        match self {
            Self::Home => Some(include_image!("../../assets/home.svg")),
            Self::Search => Some(include_image!("../../assets/search.svg")),
            Self::Playlists => Some(include_image!("../../assets/playlist.svg")),
            Self::MyPlaylists => Some(include_image!("../../assets/playlist.svg")),
            Self::Setlists => Some(include_image!("../../assets/setlist.svg")),
            Self::Favorites => Some(include_image!("../../assets/favorite.svg")),
            Self::Profile => Some(include_image!("../../assets/icon.png")),
            Self::Queue => Some(include_image!("../../assets/player-queue.svg"))
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

fn truncate_title(title: &str) -> String {
    if title.chars().count() > 50 {
        format!("{}...", title.chars().take(97).collect::<String>())
    } else {
        title.to_string()
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

    let half_vertical = vec2(size[0] / 2.0, size[1] );
    let half_horizontal = vec2(size[0], size[1] / 2.0 );

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
                    },
                    2 => {
                        Grid::new(format!("{} songs mosaic", playlist.id))
                            .spacing(Vec2::new(1.0, 2.0))
                            .show(ui, |ui| {
                                for i in 0..2 {
                                    let cover = valid_mosaic[i % valid_mosaic.len()];
                                    resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_vertical);
                                }
                            });
                        return
                    }
                    3 =>{
                        Grid::new(format!("{} songs mosaic - p1", playlist.id))
                            .spacing(Vec2::new(1.0, 2.0))
                            .show(ui, |ui| {
                                for i in 0..2 {
                                    let cover = valid_mosaic[i % valid_mosaic.len()];
                                    resolve_and_render_art(ui, cache, ctx, rt, client, cover, size / 2.0);
                                }
                            });

                        Grid::new(format!("{} songs mosaic-p2", playlist.id))
                            .spacing(Vec2::new(1.0, 1.0))
                            .show(ui, |ui| {
                                let cover = valid_mosaic[2];
                                resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_horizontal);

                            });
                        return
                    }
                    _ => {
                        Grid::new(format!("{} songs mosaic", playlist.id))
                            .spacing(Vec2::new(2.0, 2.0))
                            .show(ui, |ui| {
                                for i in 0..4 {
                                    let cover = valid_mosaic[i % valid_mosaic.len()];
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
        }
    }

    let valid_song_covers: Vec<&Artwork> = songs
        .iter()
        .filter_map(|s| s.cover_art.as_ref())
        .collect();
    if !valid_song_covers.is_empty() {
        match valid_song_covers.len() {
            1 => {
                resolve_and_render_art(ui, cache, ctx, rt, client, valid_song_covers[0], size);
                return;
            },
            2 => {
                Grid::new(format!("{} songs mosaic", playlist.id))
                    .spacing(Vec2::new(1.0, 2.0))
                    .show(ui, |ui| {
                        for i in 0..2 {
                            let cover = valid_song_covers[i % valid_song_covers.len()];
                            resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_vertical);
                        }
                    });
                return
            }
            3 =>{
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

                return
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
