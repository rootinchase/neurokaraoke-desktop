use crate::api::{LazySongDatabase, LoadingState, PlaylistDetail, SongDTO};
use crate::audio::types::Player;
use eframe::egui::Ui;
use egui_extras::{Column, TableBuilder};
use uuid::Uuid;
use crate::utilities::util::format_duration;

pub struct QueueActivity {
    pub songs: LazySongDatabase,
    pub show_queue: bool,
    pub activity: String,
    pub current_song_uuid: Option<Uuid>,
}

impl QueueActivity {
    pub fn new(songs: LazySongDatabase) -> Self {
        Self {
            songs,
            show_queue: true,
            activity: "Queue".to_string(),
            current_song_uuid: None,
        }
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        _play_playlist: impl FnMut(&PlaylistDetail),
        _play_song: impl FnMut(Vec<SongDTO>, Uuid) + 'static,
        player: &Player,
    ) {
        ui.heading(&self.activity);
        ui.push_id(&self.activity, |ui| {
            if self.show_queue {
                if let Some(pl) = player.get_playlist() {
                    let url_pl = player.get_url_playlist();
                    self.current_song_uuid = player.get_playback_state().map(|s| s.song());

                    TableBuilder::new(ui)
                        .column(Column::remainder())
                        .column(Column::exact(150.0))
                        .column(Column::exact(60.0))
                        .header(20.0, |mut header| {
                            header.col(|ui| {
                                ui.label("Song");
                            });
                            header.col(|ui| {
                                ui.label("Cover Artist");
                            });
                            header.col(|ui| {
                                ui.label("Duration");
                            });
                        })
                        .body(|body| {
                            body.rows(20.0, pl.len(), |mut row| {
                                let idx = row.index();
                                let song_uuid = pl[idx];

                                let song_data = url_pl
                                    .as_ref()
                                    .and_then(|list| list.iter().find(|s| s.id == song_uuid));

                                let (orig_artists, title, cover_artists, duration) =
                                    if let Some(song) = song_data {
                                        (
                                            song.original_artists.join(" & "),
                                            song.title.to_string(),
                                            song.cover_artists.join(" & "),
                                            song.duration,
                                        )
                                    } else {
                                        match self.songs.get(&song_uuid, |s| {
                                            (
                                                s.original_artists.clone(),
                                                s.title.to_string(),
                                                s.cover_artists.clone(),
                                                None,
                                            )
                                        }) {
                                            LoadingState::Loaded((oa, t, ca, d)) => {
                                                (oa.join(" & "), t, ca.join(" & "), d)
                                            }
                                            _ => (
                                                "-".to_string(),
                                                "Loading...".to_string(),
                                                "-".to_string(),
                                                None,
                                            ),
                                        }
                                    };

                                row.col(|ui| {
                                    let display_text = format!("{} - {}", orig_artists, title);
                                    if ui
                                        .selectable_label(
                                            self.current_song_uuid == Some(song_uuid),
                                            display_text,
                                        )
                                        .clicked()
                                    {
                                        player.play_song(song_uuid);
                                    }
                                });
                                row.col(|ui| {
                                    ui.label(cover_artists);
                                });
                                row.col(|ui| {
                                    if let Some(dur) = duration {
                                        ui.label(format!("{}:{:02}", dur / 60, dur % 60));
                                    } else {
                                        ui.label("-");
                                    }
                                });
                            });
                        });
                }
            }
        });
    }
}
