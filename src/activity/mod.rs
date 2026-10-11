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
pub mod uploads;

use crate::api::{Artwork, Playlist, PlaylistDetail, SongDTO};
use crate::theme::ThemeManager;
use crate::utilities::cache::{self, PersistentMediaCache};
use crate::utilities::util::format_duration;
use eframe::egui::{
    Align2, Color32, Context, FontId, Frame, Grid, Image, ImageSource, Sense, Ui, Vec2,
    include_image, vec2,
};
use egui_extras::{Column, Table, TableBuilder};
use reqwest::Client;
use std::fs::read;
use std::sync::Arc;
use tokio::runtime::Runtime;
use uuid::Uuid;

/// Computes the egui `bytes://` texture URI that `resolve_and_render_art`
/// registers for a given artwork. Returns `None` when the artwork has no cached
/// image file on disk (in which case no texture was ever registered for it), so
/// callers can release the matching texture with `Context::forget_image`.
pub fn cover_art_uri(cache: &PersistentMediaCache, art: &Artwork) -> Option<String> {
    let cloudflare_id = art.cloudflare_id.as_deref().filter(|s| !s.is_empty());
    let art_uuid_str = cloudflare_id.unwrap_or(&art.id);
    let art_uuid = Uuid::parse_str(art_uuid_str)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, art_uuid_str.as_bytes()));
    let path = cache.get_cached_path(art_uuid, cache::AssetType::Image)?;
    Some(format!("bytes://{}", path.display()))
}

/// Computes the virtualized grid slice `[start..end]` for a scroll offset,
/// shared by the playlist/setlist grid renderers.
///
/// `start` is derived from the persisted scroll offset and is clamped to `len`:
/// the list can shrink (search filter, sort, load-more reset) while a stale
/// deep offset from rapid scrolling is still stored in `last_scroll_y`. Without
/// the clamp, `start > len` makes `end = min(start + visible, len) < start`
/// and slicing panics with "range start index N out of range for slice of
/// length M".
pub fn visible_range(
    offset_y: f64,
    viewport_height: f32,
    box_height: f32,
    len: usize,
) -> (usize, usize) {
    let start = (offset_y / box_height as f64) as usize;
    let start = start.min(len);
    let visible = (viewport_height / box_height).ceil() as usize + 2;
    let end = (start + visible).min(len);
    (start, end)
}

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
    Uploads,
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
            Self::Uploads => "My Uploads",
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
            Self::Uploads => Some(include_image!("../../assets/uploaded.svg")),
            Self::Profile => Some(include_image!("../../assets/icon.png")),
            Self::Settings => Some(include_image!("../../assets/settings.svg")),
        }
    }
}

pub fn render_fallback_art(ui: &mut Ui, size: Vec2) {
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
        render_fallback_art(ui, size);
        return;
    }

    let art_uuid_str = cloudflare_id.unwrap_or(&cover_art.id);

    let art_uuid = Uuid::parse_str(art_uuid_str)
        .unwrap_or_else(|_| Uuid::new_v5(&Uuid::NAMESPACE_URL, art_uuid_str.as_bytes()));

    // FIXED: Use the new helper method to safely query the persistent cache setup
    if let Some(path) = cache.get_cached_path(art_uuid, cache::AssetType::Image)
        && let Ok(bytes) = read(&path) {
            ui.add(
                Image::from_bytes(format!("bytes://{}", path.display()), bytes)
                    .fit_to_exact_size(size),
            );
            return;
        }

    // Skip spawning a new download task when one is already in flight for
    // this artwork. Without this guard, every frame spawns a task for
    // missing/stalled art, and tasks blocked on a stalled download's Notify
    // accumulate unboundedly.
    if cache.has_in_flight(art_uuid, cache::AssetType::Image) {
        render_fallback_art(ui, size);
        return;
    }

    let cache_clone = cache.clone();
    let _client_clone = client.clone();
    let ctx_clone = ctx.clone();
    let artwork_url_clone = artwork_url.clone();
    rt.spawn(async move {
        let _ = cache_clone
            .get_or_download_image(art_uuid, artwork_url_clone, cache::AssetType::Image)
            .await;
        ctx_clone.request_repaint();
    });

    // Provide a more visible fallback when art is loading or missing
    render_fallback_art(ui, size);
}

pub fn render_grid_2x2(
    ui: &mut Ui,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    id: &str,
    covers: &[&Artwork],
    size: Vec2,
) {
    Grid::new(format!("{} songs mosaic", id))
        .spacing(Vec2::new(2.0, 2.0))
        .show(ui, |ui| {
            for i in 0..4 {
                let cover = covers[i % covers.len()];
                resolve_and_render_art(ui, cache, ctx, rt, client, cover, size / 2.0);
                if i == 1 {
                    ui.end_row();
                }
            }
        });
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
        render_fallback_art(ui, size);
        return;
    }

    let half_horizontal = vec2(size[0], size[1] / 2.0);

    match valid_covers.len() {
        1 => {
            resolve_and_render_art(ui, cache, ctx, rt, client, valid_covers[0], size);
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
        }
        _ => {
            render_grid_2x2(ui, cache, ctx, rt, client, id, &valid_covers, size);
        }
    }
}

pub fn build_playlist_table<'a>(ui: &'a mut Ui, id: &str) -> Table<'a> {
    TableBuilder::new(ui)
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
        })
}

pub fn render_playlist_table(
    ui: &mut Ui,
    id: &str,
    playlist: &PlaylistDetail,
    on_play_song: &mut impl FnMut(Vec<SongDTO>, Uuid),
) {
    let table = build_playlist_table(ui, id);

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
                render_duration_label(ui, song.duration);
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
        if (n.is_multiple_of(k) || n % k < k) && n / k >= 2 && k >= 3 {
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

pub fn render_mosaic_covers(
    ui: &mut Ui,
    cache: &Arc<PersistentMediaCache>,
    ctx: &Context,
    rt: &Runtime,
    client: &Client,
    id: &str,
    covers: &[&Artwork],
    size: Vec2,
) {
    if covers.is_empty() {
        return;
    }
    let half_vertical = vec2(size[0] / 2.0, size[1]);
    let half_horizontal = vec2(size[0], size[1] / 2.0);

    match covers.len() {
        1 => {
            resolve_and_render_art(ui, cache, ctx, rt, client, covers[0], size);
        }
        2 => {
            Grid::new(format!("{} songs mosaic", id))
                .spacing(Vec2::new(1.0, 2.0))
                .show(ui, |ui| {
                    for i in 0..2 {
                        let cover = covers[i % covers.len()];
                        resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_vertical);
                    }
                });
        }
        3 => {
            Grid::new(format!("{} songs mosaic - p1", id))
                .spacing(Vec2::new(1.0, 2.0))
                .show(ui, |ui| {
                    for i in 0..2 {
                        let cover = covers[i % covers.len()];
                        resolve_and_render_art(ui, cache, ctx, rt, client, cover, size / 2.0);
                    }
                });
            Grid::new(format!("{} songs mosaic-p2", id))
                .spacing(Vec2::new(1.0, 1.0))
                .show(ui, |ui| {
                    let cover = covers[2];
                    resolve_and_render_art(ui, cache, ctx, rt, client, cover, half_horizontal);
                });
        }
        _ => {
            render_grid_2x2(ui, cache, ctx, rt, client, id, covers, size);
        }
    }
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
    let media_valid = playlist.media.as_ref().is_some_and(|art| {
        let cloudflare_id = art
            .cloudflare_id
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "null");
        let abs = art.absolute_path.trim();
        let url = cache::get_thumbnail_url(cloudflare_id, abs, "crop,gravity=auto");
        !url.is_empty()
    });

    if media_valid
        && let Some(art) = &playlist.media {
            resolve_and_render_art(ui, cache, ctx, rt, client, art, size);
            return;
        }

    let mosaic_valid = playlist
        .mosaic_media
        .as_ref()
        .is_some_and(|m| !m.is_empty());
    if mosaic_valid
        && let Some(mosaic) = &playlist.mosaic_media {
            let valid_mosaic: Vec<&Artwork> = mosaic.iter().collect();
            if !valid_mosaic.is_empty() {
                render_mosaic_covers(
                    ui,
                    cache,
                    ctx,
                    rt,
                    client,
                    &playlist.id.to_string(),
                    &valid_mosaic,
                    size,
                );
                return;
            }
        }

    let valid_song_covers: Vec<&Artwork> =
        songs.iter().filter_map(|s| s.cover_art.as_ref()).collect();
    if !valid_song_covers.is_empty() {
        render_mosaic_covers(
            ui,
            cache,
            ctx,
            rt,
            client,
            &playlist.id.to_string(),
            &valid_song_covers,
            size,
        );
        return;
    }

    render_fallback_art(ui, size);
}

pub fn render_duration_label(ui: &mut Ui, duration: Option<u64>) {
    if let Some(dur) = duration {
        ui.label(format_duration(dur));
    } else {
        ui.label("-");
    }
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

#[cfg(test)]
mod tests {
    use super::visible_range;

    const BOX: f32 = 260.0;

    #[test]
    fn test_visible_range_top_of_list() {
        // Offset 0 with a viewport fitting ~3 boxes -> start 0, end 3 + 2 = 5.
        assert_eq!(visible_range(0.0, 780.0, BOX, 20), (0, 5));
    }

    #[test]
    fn test_visible_range_mid_list() {
        // Scrolled to box 10 -> start 10, end min(10 + 5, 20) = 15.
        assert_eq!(visible_range(2600.0, 780.0, BOX, 20), (10, 15));
    }

    #[test]
    fn test_visible_range_deep_offset_shrunk_list() {
        // Regression for the "range start index 42 out of range for slice of
        // length 20" crash: rapid scrolling stores a deep offset, then the
        // list shrinks (search filter/sort). start must clamp to len so the
        // slice is empty, not a panic.
        assert_eq!(visible_range(10920.0, 780.0, BOX, 20), (20, 20));
    }

    #[test]
    fn test_visible_range_empty_list() {
        assert_eq!(visible_range(5000.0, 780.0, BOX, 0), (0, 0));
    }

    #[test]
    fn test_visible_range_end_never_exceeds_len() {
        // start + visible beyond len clamps end to len.
        let (start, end) = visible_range(4940.0, 780.0, BOX, 20);
        assert!(start <= end && end <= 20);
        assert_eq!((start, end), (19, 20));
    }
}

