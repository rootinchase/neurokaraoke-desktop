use crate::activity::SortOption;
use crate::api::sources::{SourceKind, classify_source_url};
use crate::api::{FileUpload, LazySongDatabase, LoadingState, SongDTO, UploadSong, UserLimits};
use crate::app::fonts;
use crate::audio::types::Player;
use crate::debug_log;
use crate::theme::ThemeManager;
use crate::utilities::cache::cache_dir;
use crate::utilities::util::{load_cached_song_list, sort_items};

use eframe::egui::{
    Align, Button, Context, Frame, Image, Layout, Margin, ProgressBar, RichText, ScrollArea, Sense,
    Stroke, StrokeKind, TextEdit, Ui, Vec2, include_image,
};
use egui_extras::{Column, TableBuilder};
use rfd::FileDialog;
use ron::ser::to_string_pretty;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::fs::write;
use tokio::sync::Mutex;
use uuid::Uuid;

/// Extensions the file picker accepts, aligned with `audio_content_type`.
const AUDIO_EXTENSIONS: [&str; 11] = [
    "mp3", "m4a", "mp4", "m4b", "aac", "ogg", "oga", "opus", "flac", "wav", "webm",
];

/// What the most recent upload or delete reported to the user.
#[derive(Debug, Clone)]
pub enum UploadFeedback {
    Idle,
    Working(String),
    Done(String),
    Failed(String),
}

pub struct UploadsActivity {
    pub ctx: Context,
    pub db: LazySongDatabase,
    pub songs: Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
    pub limits: Arc<Mutex<Option<UserLimits>>>,
    pub is_fetching: Arc<AtomicBool>,
    pub current_sort: SortOption,
    pub current_sort_desc: bool,
    /// Whether the "DOWNLOAD FROM YOUTUBE" panel is open.
    pub show_url_panel: bool,
    /// The source URL typed into that panel.
    pub url_input: String,
    /// Files picked from the drop zone, waiting for a title/artist and an upload.
    pub pending: Vec<FileUpload>,
    /// Progress/result of the most recent upload or delete.
    pub feedback: Arc<Mutex<UploadFeedback>>,
    pub is_uploading: Arc<AtomicBool>,
    /// Set by a finished URL download; the panel closes itself on the next frame.
    pub url_done: Arc<AtomicBool>,
}

impl UploadsActivity {
    pub fn new(ctx: Context, db: LazySongDatabase) -> Self {
        Self {
            ctx,
            db,
            songs: Arc::new(Mutex::new(LoadingState::Loading)),
            limits: Arc::new(Mutex::new(None)),
            is_fetching: Arc::new(AtomicBool::new(false)),
            current_sort: SortOption::Date,
            current_sort_desc: true,
            show_url_panel: false,
            url_input: String::new(),
            pending: Vec::new(),
            feedback: Arc::new(Mutex::new(UploadFeedback::Idle)),
            is_uploading: Arc::new(AtomicBool::new(false)),
            url_done: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Loads the uploads list (cache-first) and the user's quota, then repaints.
    pub fn fetch_uploads(&self) {
        if self.is_fetching.swap(true, Ordering::SeqCst) {
            return;
        }

        let s = self.songs.clone();
        let l = self.limits.clone();
        let db = self.db.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            let cache_path = cache_dir().join("uploads.ron");
            let has_cached = load_cached_song_list(&cache_path, &s).await;

            match db.get_user_uploads().await {
                Ok(list) => {
                    *s.lock().await = LoadingState::Loaded(list.clone());
                    if let Some(parent) = cache_path.parent() {
                        let _ = tokio::fs::create_dir_all(parent).await;
                    }
                    if let Ok(serialized) = to_string_pretty(&list, Default::default()) {
                        let _ = write(&cache_path, serialized).await;
                    }
                }
                Err(err) => {
                    let mut lock = s.lock().await;
                    if !has_cached && matches!(*lock, LoadingState::Loading) {
                        *lock = LoadingState::Failed(Arc::new(err));
                    }
                }
            }

            // Best-effort quota fetch; ignore failures so the list still renders.
            if let Ok(limits) = db.get_user_limits().await {
                *l.lock().await = Some(limits);
            }

            ctx.request_repaint();
        });
    }

    /// Reports a progress/result message to the UI and asks for a repaint.
    fn set_feedback(&self, feedback: UploadFeedback) {
        *self.feedback.blocking_lock() = feedback;
        self.ctx.request_repaint();
    }

    /// Re-reads the uploads list and the quota after a successful upload or delete.
    async fn refresh_state(
        db: &LazySongDatabase,
        songs: &Arc<Mutex<LoadingState<Vec<SongDTO>>>>,
        limits: &Arc<Mutex<Option<UserLimits>>>,
    ) {
        if let Ok(list) = db.get_user_uploads().await {
            *songs.lock().await = LoadingState::Loaded(list);
        }
        if let Ok(new_limits) = db.get_user_limits().await {
            *limits.lock().await = Some(new_limits);
        }
    }

    /// Sends `self.url_input` to `POST /api/user/song/download-from-url`. The URL is
    /// validated offline by `classify_source_url` first, so the server's fast
    /// rejections show up before a request leaves the machine.
    pub fn start_url_upload(&mut self) {
        if self.is_uploading.swap(true, Ordering::SeqCst) {
            return;
        }
        // Quota gate first: the server rejects an upload over the song cap, so fail
        // with the same message the web app shows instead of sending a doomed request.
        if let Some(limits) = self.limits.blocking_lock().as_ref()
            && !limits.has_song_slot()
        {
            self.is_uploading.store(false, Ordering::SeqCst);
            self.set_feedback(UploadFeedback::Failed(format!(
                "Upload limit reached — {} of {} songs used.",
                limits.current_song_count, limits.max_songs
            )));
            return;
        }

        let url = self.url_input.trim().to_string();
        self.set_feedback(UploadFeedback::Working(format!("Downloading {}…", url)));

        let db = self.db.clone();
        let songs = self.songs.clone();
        let limits = self.limits.clone();
        let feedback = self.feedback.clone();
        let uploading = self.is_uploading.clone();
        let ctx = self.ctx.clone();
        let url_done = self.url_done.clone();

        tokio::spawn(async move {
            match db.upload_song_from_url(UploadSong::new(url)).await {
                Ok(result) => {
                    debug_log!("Upload accepted: {} ({})", result.title, result.song_id);
                    *feedback.lock().await =
                        UploadFeedback::Done(format!("Added {}", result.title));
                    Self::refresh_state(&db, &songs, &limits).await;
                    url_done.store(true, Ordering::SeqCst);
                }
                Err(err) => {
                    debug_log!("Upload rejected: {}", err);
                    *feedback.lock().await = UploadFeedback::Failed(err.to_string());
                }
            }
            uploading.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    /// Uploads one picked local file to `POST /api/user/song/upload`.
    pub fn start_file_upload(&mut self, upload: FileUpload) {
        if self.is_uploading.swap(true, Ordering::SeqCst) {
            return;
        }
        // Quota gate first: a file that cannot fit under the storage cap (or a full
        // song slot) is refused locally, before any bytes are sent.
        let size = std::fs::metadata(&upload.path)
            .map(|meta| meta.len())
            .unwrap_or(0);
        if let Some(limits) = self.limits.blocking_lock().as_ref()
            && !limits.can_fit(size)
        {
            self.is_uploading.store(false, Ordering::SeqCst);
            self.set_feedback(UploadFeedback::Failed(format!(
                "Upload limit reached — {} of {} songs used, {} of {} storage used.",
                limits.current_song_count,
                limits.max_songs,
                format_bytes(limits.used_storage_bytes),
                format_bytes(limits.max_storage_bytes)
            )));
            return;
        }

        // The row is consumed by the upload, so drop it from the pending list.
        self.pending.retain(|item| item.path != upload.path);

        let title = upload.title.clone();
        self.set_feedback(UploadFeedback::Working(format!("Uploading {}…", title)));

        let db = self.db.clone();
        let songs = self.songs.clone();
        let limits = self.limits.clone();
        let feedback = self.feedback.clone();
        let uploading = self.is_uploading.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            match db.upload_song_file(upload).await {
                Ok(()) => {
                    debug_log!("File upload accepted: {}", title);
                    *feedback.lock().await = UploadFeedback::Done(format!("Uploaded {}", title));
                    Self::refresh_state(&db, &songs, &limits).await;
                }
                Err(err) => {
                    debug_log!("File upload rejected: {}", err);
                    *feedback.lock().await = UploadFeedback::Failed(err.to_string());
                }
            }
            uploading.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    /// Removes one of the user's own uploads, `DELETE /api/user/song/{id}`.
    pub fn start_delete(&mut self, song_id: Uuid, title: String) {
        if self.is_uploading.swap(true, Ordering::SeqCst) {
            return;
        }
        self.set_feedback(UploadFeedback::Working(format!("Removing {}…", title)));

        let db = self.db.clone();
        let songs = self.songs.clone();
        let limits = self.limits.clone();
        let feedback = self.feedback.clone();
        let uploading = self.is_uploading.clone();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            match db.delete_uploaded_song(song_id).await {
                Ok(()) => {
                    debug_log!("Deleted uploaded song {}", song_id);
                    *feedback.lock().await = UploadFeedback::Done(format!("Removed {}", title));
                    Self::refresh_state(&db, &songs, &limits).await;
                }
                Err(err) => {
                    debug_log!("Delete rejected: {}", err);
                    *feedback.lock().await = UploadFeedback::Failed(err.to_string());
                }
            }
            uploading.store(false, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        theme: &ThemeManager,
        player: &mut Player,
        current_song_uuid: &Option<Uuid>,
        is_logged_in: bool,
    ) {
        // Header: the title on the left, the accent upload button on the right,
        // exactly where the web app puts "DOWNLOAD FROM YOUTUBE".
        ui.horizontal(|ui| {
            ui.heading("Uploads");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let frame = Frame::new()
                    .fill(theme.accent)
                    .corner_radius(6.0)
                    .inner_margin(Margin::symmetric(10, 6));
                let show = frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            Image::new(include_image!("../../assets/yt.svg"))
                                .fit_to_exact_size(Vec2::new(18.0, 18.0))
                                .tint(theme.text),
                        );
                        ui.label(
                            RichText::new("DOWNLOAD FROM YOUTUBE")
                                .strong()
                                .color(theme.text),
                        );
                    });
                });
                let button = ui.interact(
                    show.response.rect,
                    ui.id().with("yt_upload_button"),
                    Sense::click(),
                );
                if button.clicked() {
                    self.show_url_panel = !self.show_url_panel;
                }
            });
        });
        ui.add_space(10.0);

        // A finished download closes the URL panel and clears the field, so the
        // section vanishes once the song is in the library.
        if self.url_done.swap(false, Ordering::SeqCst) {
            self.show_url_panel = false;
            self.url_input.clear();
        }

        if !is_logged_in {
            ui.label("Sign in to view your uploads.");
            return;
        }

        // URL panel, opened by the accent button. The URL is validated offline with the
        // server's own wording, so the Download button only unlocks for usable links.
        if self.show_url_panel {
            fonts::ensure_str(&self.url_input);
            let busy = self.is_uploading.load(Ordering::SeqCst);
            let quota_full = self
                .limits
                .blocking_lock()
                .as_ref()
                .is_some_and(|limits| !limits.has_song_slot());
            let frame = Frame::new()
                .fill(theme.background_elevated)
                .corner_radius(8.0)
                .inner_margin(Margin::symmetric(10, 8));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Source URL");
                    ui.add_sized(
                        [ui.available_width(), 24.0],
                        TextEdit::singleline(&mut self.url_input)
                            .hint_text("https://www.youtube.com/watch?v=…"),
                    );
                });
                let url = self.url_input.trim().to_string();
                ui.horizontal(|ui| {
                    if !url.is_empty() {
                        match classify_source_url(&url) {
                            Ok(kind) => {
                                ui.label(
                                    RichText::new(describe_source(&kind)).color(theme.success),
                                );
                            }
                            Err(err) => {
                                ui.label(RichText::new(err.to_string()).color(theme.error));
                            }
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let valid = classify_source_url(&url).is_ok();
                        let button =
                            ui.add_enabled(!busy && valid && !quota_full, Button::new("Download"));
                        if button.clicked() {
                            self.start_url_upload();
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }

        // Quota header from UserLimits — Songs and Storage bars side by side
        if let Some(limits) = self.limits.blocking_lock().as_ref() {
            let song_fraction = if limits.max_songs > 0 {
                limits.current_song_count as f32 / limits.max_songs as f32
            } else {
                0.0
            };
            let storage_fraction = if limits.max_storage_bytes > 0 {
                limits.used_storage_bytes as f32 / limits.max_storage_bytes as f32
            } else {
                0.0
            };

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label("Songs");
                    let bar = ProgressBar::new(song_fraction)
                        .text(format!(
                            "{} / {}",
                            limits.current_song_count, limits.max_songs
                        ))
                        .fill(theme.primary);
                    ui.add_sized([200.0, 20.0], bar);
                });
                ui.add_space(16.0);
                ui.vertical(|ui| {
                    ui.label("Storage");
                    let bar = ProgressBar::new(storage_fraction)
                        .text(format!(
                            "{} / {}",
                            format_bytes(limits.used_storage_bytes),
                            format_bytes(limits.max_storage_bytes)
                        ))
                        .fill(theme.primary);
                    ui.add_sized([200.0, 20.0], bar);
                });
            });
            ui.add_space(10.0);
        }

        // Drop zone — icon, heading, and browse hint, mirroring the web app. Clicking
        // it opens the system file picker, filtered to the audio extensions the
        // server accepts. Files dragged from the OS arrive in `dropped_files`, and the
        // zone lights up while a drag hovers the window.
        let dragging = ui.input(|i| !i.raw.hovered_files.is_empty());
        let frame = Frame::new()
            .fill(theme.background_secondary)
            .corner_radius(10.0)
            .inner_margin(Margin::symmetric(14, 14));
        let show = frame.show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(4.0);
                ui.add(
                    Image::new(include_image!("../../assets/drop-zone.svg"))
                        .fit_to_exact_size(Vec2::new(48.0, 48.0))
                        .tint(if dragging {
                            theme.accent
                        } else {
                            theme.text_muted
                        }),
                );
                ui.label(RichText::new("Upload Your Songs").strong().size(16.0));
                ui.label(
                    RichText::new("Drag and drop audio files here, or click to browse")
                        .color(theme.text_secondary),
                );
                ui.add_space(4.0);
            });
        });
        let zone = ui.interact(
            show.response.rect,
            ui.id().with("drop_zone"),
            Sense::click(),
        );
        let border = if dragging || zone.contains_pointer() {
            theme.accent
        } else {
            theme.border
        };
        ui.painter().rect_stroke(
            show.response.rect,
            10.0,
            Stroke::new(1.0, border),
            StrokeKind::Inside,
        );
        if zone.clicked() {
            for path in pick_audio_files() {
                debug_log!("Selected audio file for upload: {}", path.display());
                self.pending.push(FileUpload::from_path(path));
            }
        }

        // Files dragged in from the OS, filtered to the accepted audio extensions.
        // winit 0.30.13 delivers drops on Windows and the web; X11 drops start
        // arriving as soon as the project moves to winit 0.31.
        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .filter(|path| {
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| {
                            let ext = ext.to_lowercase();
                            AUDIO_EXTENSIONS.iter().any(|known| *known == ext.as_str())
                        })
                })
                .collect()
        });
        for path in dropped {
            debug_log!("Dropped audio file: {}", path.display());
            self.pending.push(FileUpload::from_path(path));
        }

        // Files waiting for a title/artist, then an upload.
        let busy = self.is_uploading.load(Ordering::SeqCst);
        let quota_full = self
            .limits
            .blocking_lock()
            .as_ref()
            .is_some_and(|limits| !limits.has_song_slot());
        let mut upload_now: Option<FileUpload> = None;
        let mut remove_at: Option<usize> = None;
        if !self.pending.is_empty() {
            ui.add_space(8.0);
            for (index, item) in self.pending.iter_mut().enumerate() {
                fonts::ensure_str(&item.title);
                fonts::ensure_str(&item.artist);
                let file_name = item
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                let frame = Frame::new()
                    .fill(theme.background_elevated)
                    .corner_radius(6.0)
                    .inner_margin(Margin::symmetric(8, 6));
                frame.show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(file_name).color(theme.text_secondary));
                        let discard = Image::new(include_image!("../../assets/trash.svg"))
                            .fit_to_exact_size(Vec2::new(16.0, 16.0))
                            .tint(theme.error)
                            .sense(Sense::click());
                        if ui.add(discard).clicked() {
                            remove_at = Some(index);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Title");
                        ui.add_sized([170.0, 24.0], TextEdit::singleline(&mut item.title));
                        ui.label("Artist");
                        ui.add_sized([130.0, 24.0], TextEdit::singleline(&mut item.artist));
                        let upload = ui.add_enabled(!busy && !quota_full, Button::new("Upload"));
                        if upload.clicked() {
                            upload_now = Some(item.clone());
                        }
                    });
                });
            }
        }
        if let Some(upload) = upload_now {
            self.start_file_upload(upload);
        }
        if let Some(index) = remove_at {
            self.pending.remove(index);
        }

        // Feedback from the most recent upload or delete.
        match self.feedback.blocking_lock().clone() {
            UploadFeedback::Idle => {}
            UploadFeedback::Working(text) => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(text).color(theme.text_secondary));
                });
            }
            UploadFeedback::Done(text) => {
                ui.add_space(6.0);
                ui.label(RichText::new(text).color(theme.success));
            }
            UploadFeedback::Failed(text) => {
                ui.add_space(6.0);
                ui.label(RichText::new(text).color(theme.error));
            }
        }
        ui.add_space(10.0);

        let guard = self.songs.blocking_lock();
        let mut delete_id: Option<Uuid> = None;
        match &*guard {
            LoadingState::Loaded(loaded) => {
                let mut songs = loaded.clone();
                sort_items(
                    &mut songs,
                    &self.current_sort,
                    &self.current_sort_desc,
                    |s| s.title.to_string(),
                    |_| 0,
                    |s| s.play_count.unwrap_or(0) as u32,
                    |s| s.stream_date.as_ref().map(|d| d.to_string()),
                );

                if songs.is_empty() {
                    ui.label("You haven't uploaded any songs yet.");
                    return;
                }

                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        TableBuilder::new(ui)
                            .column(Column::exact(24.0))
                            .column(Column::remainder())
                            .column(Column::exact(120.0))
                            .column(Column::exact(32.0))
                            .header(20.0, |mut header| {
                                header.col(|ui| {
                                    ui.label("");
                                });
                                header.col(|ui| {
                                    ui.label("Title");
                                });
                                header.col(|ui| {
                                    ui.label("Original");
                                });
                                header.col(|ui| {
                                    ui.label("");
                                });
                            })
                            .body(|body| {
                                body.rows(20.0, songs.len(), |mut row| {
                                    let song = &songs[row.index()];
                                    let song_uuid = song.id;

                                    row.col(|ui| {
                                        let mut is_in_playlist = player
                                            .get_playlist()
                                            .is_some_and(|pl| pl.contains(&song_uuid));
                                        if ui.checkbox(&mut is_in_playlist, "").changed() {
                                            if is_in_playlist {
                                                player.append_to_playlist(song_uuid);
                                                if player.get_playback_state().is_none() {
                                                    player.play_song(song_uuid);
                                                } else {
                                                    player.play();
                                                }
                                            } else {
                                                player.remove_from_playlist(song_uuid);
                                            }
                                        }
                                    });
                                    row.col(|ui| {
                                        if ui
                                            .selectable_label(
                                                current_song_uuid == &Some(song_uuid),
                                                song.title.to_string(),
                                            )
                                            .clicked()
                                        {
                                            player.play_song(song_uuid);
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(song.original_artists.join(" & "));
                                    });
                                    row.col(|ui| {
                                        let trash =
                                            Image::new(include_image!("../../assets/trash.svg"))
                                                .fit_to_exact_size(Vec2::new(16.0, 16.0))
                                                .tint(theme.accent)
                                                .sense(Sense::click());
                                        if ui.add(trash).clicked() {
                                            delete_id = Some(song_uuid);
                                        }
                                    });
                                });
                            });
                    });
            }

            LoadingState::Loading => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading uploads...");
                });
            }
            LoadingState::Failed(err) => {
                debug_log!("Failed to load uploads: {}", err);
                ui.label(format!("Error loading uploads: {}", err));
            }
        }

        // The delete runs once the list lock is released; the title comes from the
        // snapshot taken above.
        if let Some(song_id) = delete_id {
            let title = match &*guard {
                LoadingState::Loaded(songs) => songs
                    .iter()
                    .find(|s| s.id == song_id)
                    .map(|s| s.title.to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            };
            drop(guard);
            self.start_delete(song_id, title);
        }
    }
}

/// Human-readable label for a validated source, shown next to the URL field.
fn describe_source(kind: &SourceKind) -> String {
    match kind {
        SourceKind::YouTubeVideo { video_id } => format!("YouTube video {}", video_id),
        SourceKind::YouTubePlaylist { list_id } => format!("YouTube playlist {}", list_id),
        SourceKind::BilibiliVideo { video_id } => format!("Bilibili video {}", video_id),
        SourceKind::DiscordAttachment { .. } => "Discord attachment".to_string(),
    }
}

/// Opens the system file picker, filtered to the audio extensions the server accepts
/// (see `audio_content_type`). Returns the selected paths; empty if the dialog was
/// cancelled.
fn pick_audio_files() -> Vec<PathBuf> {
    FileDialog::new()
        .set_title("Select audio files")
        .add_filter("Audio", &AUDIO_EXTENSIONS[..])
        .pick_files()
        .unwrap_or_default()
}

/// Human-readable byte count (KB / MB / GB).
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{} KB", bytes / KB)
    } else {
        format!("{} B", bytes)
    }
}
