use crate::activity::SortOption;
use crate::api::LoadingState;
use eframe::egui::Ui;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_with::{DeserializeAs, SerializeAs};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

pub fn sort_items<T>(
    items: &mut [T],
    current_sort: &SortOption,
    current_sort_desc: &bool,
    get_name: impl Fn(&T) -> String,
    get_song_count: impl Fn(&T) -> u32,
    get_play_count: impl Fn(&T) -> u32,
    get_date: impl Fn(&T) -> Option<String>,
) {
    match current_sort {
        SortOption::Name => if *current_sort_desc {
            items.sort_by(|a, b| get_name(b).to_lowercase().cmp(&get_name(a).to_lowercase()))
        } else {
            items.sort_by(|a, b| get_name(a).to_lowercase().cmp(&get_name(b).to_lowercase()))
        },
        SortOption::Songs => if *current_sort_desc {
            items.sort_by(|a, b| get_song_count(b).cmp(&get_song_count(a)))
        } else {
            items.sort_by(|a, b| get_song_count(a).cmp(&get_song_count(b)))
        },
        SortOption::Plays => if *current_sort_desc {
            items.sort_by(|a, b| get_play_count(b).cmp(&get_play_count(a)))
        } else {
            items.sort_by(|a, b| get_play_count(a).cmp(&get_play_count(b)))
        },
        SortOption::Date => {
            if *current_sort_desc {
                items.sort_by(|a, b| get_date(b).cmp(&get_date(a)))
            } else {
                items.sort_by(|a, b| get_date(a).cmp(&get_date(b)))
            }
        },
    }
}

pub fn split_by_space_respecting_quotes(
    s: &str
) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;

    for c in s.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            ' ' if !in_quotes => {
                if !current.is_empty() {
                    result.push(current.clone());
                    current.clear();
                }
            },
            _ => current.push(c),
        }
    }
    if !current.is_empty() {
        result.push(current);
    }
    result
}

pub fn format_duration(
    seconds: u64
) -> String {
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("DEBUG_PLAYBACK").is_ok() {
            eprintln!($($arg)*);
        }
    };
}

pub fn render_song_table(
    ui: &mut Ui,
    songs: &[crate::api::SongDTO]
) {
    use egui_extras::{Column, TableBuilder};

    TableBuilder::new(ui)
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
                ui.label("Stream Date");
            });
            header.col(|ui| {
                ui.label("Duration");
            });
        })
        .body(|body| {
            body.rows(20.0, songs.len(), |mut row| {
                let row_index = row.index();
                let song = &songs[row_index];
                row.col(|ui| {
                    let text = format!(
                        "{} - {} ({})",
                        song.original_artists.join(" & "),
                        song.title,
                        song.cover_artists.join(" & ")
                    );
                    ui.label(text);
                });
                row.col(|ui| {
                    ui.label(
                        song.play_count
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                    );
                });
                row.col(|ui| {
                    ui.label(song.stream_date.as_deref().unwrap_or("-"));
                });
                row.col(|ui| {
                    if let Some(duration) = song.duration {
                        ui.label(format_duration(duration));
                    } else {
                        ui.label("-");
                    }
                });
            });
        });
}

pub fn render_list_table<T>(
    ui: &mut Ui,
    id: &str,
    items: &[T],
    name: impl Fn(&T) -> String,
    song_count: impl Fn(&T) -> String,
    play_count: impl Fn(&T) -> String,
    metadata: impl Fn(&T) -> String,
    created_at: impl Fn(&T) -> String,
    updated_at: impl Fn(&T) -> String,
    mut on_click: impl FnMut(usize),
    mut on_header_click: impl FnMut(usize),
) {
    use egui_extras::{Column, TableBuilder};

    ui.push_id(id, |ui| {
        TableBuilder::new(ui)
            .column(Column::remainder())
            .column(Column::exact(60.0))
            .column(Column::exact(80.0))
            .column(if is_col_empty(items, &metadata) { Column::exact(0.0) } else { Column::exact(100.0) })
            .column(Column::exact(80.0))
            .column(if is_col_empty(items, &updated_at) { Column::exact(0.0) } else { Column::exact(80.0) })
            .header(20.0, |mut header| {
                header.col(|ui| {
                    if ui.selectable_label(false, "Name").clicked() {
                        on_header_click(0);
                    }
                });
                header.col(|ui| {
                    if ui.selectable_label(false, "Songs").clicked() {
                        on_header_click(1);
                    }
                });
                header.col(|ui| {
                    if ui.selectable_label(false, "Plays").clicked() {
                        on_header_click(2);
                    }
                });
                if !is_col_empty(items, &metadata) {
                    header.col(|ui| {
                        if ui.selectable_label(false, "Creator").clicked() {
                            on_header_click(3);
                        }
                    });
                }
                header.col(|ui| {
                    if ui.selectable_label(false, "Created").clicked() {
                        on_header_click(4);
                    }
                });
                if !is_col_empty(items, &updated_at) {
                    header.col(|ui| {
                        if ui.selectable_label(false, "Updated").clicked() {
                            on_header_click(5);
                        }
                    });
                }
            })
            .body(|body| {
                body.rows(20.0, items.len(), |mut row| {
                    let item = &items[row.index()];
                    let mut clicked = false;
                    row.col(|ui| {
                        if ui.selectable_label(false, name(item)).clicked() {
                            clicked = true;
                        }
                    });
                    if clicked {
                        on_click(row.index());
                    }
                    row.col(|ui| {
                        ui.label(song_count(item));
                    });
                    row.col(|ui| {
                        ui.label(play_count(item));
                    });
                    if !is_col_empty(items, &metadata) {
                        row.col(|ui| {
                            ui.label(metadata(item));
                        });
                    }
                    row.col(|ui| {
                        ui.label(created_at(item));
                    });
                    if !is_col_empty(items, &updated_at) {
                        row.col(|ui| {
                            ui.label(updated_at(item));
                        });
                    }
                });
            });
    });
}

pub fn is_col_empty<T>(
    items: &[T],
    f: &impl Fn(&T) -> String
) -> bool {
    items.iter().all(|item| f(item).is_empty())
}

pub fn render_playlist_details(
    ui: &mut Ui,
    selected: &LoadingState<crate::api::PlaylistDetail>,
    mut on_play: impl FnMut(&crate::api::PlaylistDetail),
) {
    ui.separator();
    match selected {
        LoadingState::Loaded(detail) => {
            ui.label(format!("Playlist: {}", detail.name));
            if ui.button("Play Playlist").clicked() {
                on_play(detail);
            }
            render_song_table(ui, &detail.songs);
        }
        LoadingState::Loading => {
            ui.label("Loading playlist details...");
        }
        LoadingState::Failed(err) => {
            ui.label(format!("Error loading playlist details: {}", err));
        }
    }
}
#[allow(dead_code)]
pub struct IntoAs<T>(std::marker::PhantomData<T>);
impl<'de, U, T: Deserialize<'de> + Into<U>> DeserializeAs<'de, U> for IntoAs<T> {
    fn deserialize_as<D>(deserializer: D) -> Result<U, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(T::deserialize(deserializer)?.into())
    }
}
impl<U, T: Serialize> SerializeAs<U> for IntoAs<T>
where
        for<'a> T: From<&'a U>,
{
    fn serialize_as<S>(source: &U, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        T::from(source).serialize(serializer)
    }
}


pub struct AsArcMutex<T>(std::marker::PhantomData<T>);
impl<'de, T: Deserialize<'de>> DeserializeAs<'de, Arc<Mutex<T>>> for AsArcMutex<T> {
    fn deserialize_as<D>(deserializer: D) -> Result<Arc<Mutex<T>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Arc::new(Mutex::new(T::deserialize(deserializer)?)))
    }
}
impl<T: Serialize> SerializeAs<Arc<Mutex<T>>> for AsArcMutex<T> {
    fn serialize_as<S>(source: &Arc<Mutex<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        (*source.blocking_lock()).serialize(serializer)
    }
}

pub fn select_playlist_by_id(
    id: Uuid,
    selected: Arc<Mutex<Option<LoadingState<crate::api::PlaylistDetail>>>>,
    songs: crate::api::LazySongDatabase,
) {
    tokio::spawn(async move {
        *selected.lock().await = Some(LoadingState::Loading);

        match songs.get_playlist_details(id).await {
            Ok(data) => {
                *selected.lock().await = Some(LoadingState::Loaded(data));
            }
            Err(err) => {
                *selected.lock().await = Some(LoadingState::Failed(Arc::new(err)));
            }
        }
    });
}



