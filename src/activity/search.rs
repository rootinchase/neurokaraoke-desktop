use eframe::egui;
use eframe::egui::Ui;
use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

use crate::api::LoadingState;
use crate::audio::types::Player;


pub struct SearchActivity {
    search: String,
    current_search_sort: SearchSortOption,
    current_search_sort_desc: bool,
    current_song_uuid: Option<Uuid>,
}

#[derive(Debug)]
pub struct SearchCriteria {
    pub(crate) original: Option<String>,
    pub(crate) cover: Option<String>,
    pub(crate) creator: Option<String>,
    pub(crate) title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchSortOption {
    Name,
    OriginalArtist,
    CoverArtist,
    Plays,
    Length,
}

impl SearchActivity {
    pub fn new() -> Self {
        Self {
            search: "".to_string(),
            current_search_sort: SearchSortOption::Name,
            current_search_sort_desc: false,
            current_song_uuid: None,
        }
    }


    pub fn render(
        &mut self,
        ui: &mut Ui,
        songs: &crate::api::LazySongDatabase,
        player: &mut Player,
        current_song_uuid: &Option<Uuid>,
    ) {
        ui.add(egui::TextEdit::singleline(&mut self.search));

        ui.add_space(10.0);

        let criteria = parse_query(&self.search, false);
        let mut matching_songs: Vec<Uuid> = songs
            .get_map()
            .iter()
            .filter(|x| {
                x.value().if_loaded_or_else(
                    |s| {
                        let title_match = s.title.to_lowercase().contains(&criteria.title.to_lowercase());
                        let original_match = criteria.original.as_ref().map_or(true, |q| s.original_artists.iter().any(|a| a.to_lowercase().contains(&q.to_lowercase())));
                        let cover_match = criteria.cover.as_ref().map_or(true, |q| s.cover_artists.iter().any(|a| a.to_lowercase().contains(&q.to_lowercase())));

                        title_match && original_match && cover_match
                    },
                    false,
                )
            })
            .map(|x| *x.key())
            .collect();

        matching_songs.sort_by(|a, b| {
            let a_data = songs.get(a, |s| (s.title.to_string(), s.original_artists.join(" "), s.cover_artists.join(" "), s.play_count.unwrap_or(0), s.duration.unwrap_or(0)));
            let b_data = songs.get(b, |s| (s.title.to_string(), s.original_artists.join(" "), s.cover_artists.join(" "), s.play_count.unwrap_or(0), s.duration.unwrap_or(0)));

            let (a_title, a_orig, a_cover, a_plays, a_dur) = match a_data {
                LoadingState::Loaded(d) => d,
                _ => ("".to_string(), "".to_string(), "".to_string(), 0, 0),
            };
            let (b_title, b_orig, b_cover, b_plays, b_dur) = match b_data {
                LoadingState::Loaded(d) => d,
                _ => ("".to_string(), "".to_string(), "".to_string(), 0, 0),
            };

            let cmp = match self.current_search_sort {
                SearchSortOption::Name => a_title.cmp(&b_title),
                SearchSortOption::OriginalArtist => a_orig.cmp(&b_orig),
                SearchSortOption::CoverArtist => a_cover.cmp(&b_cover),
                SearchSortOption::Plays => a_plays.cmp(&b_plays),
                SearchSortOption::Length => a_dur.cmp(&b_dur),
            };

            if self.current_search_sort_desc { cmp.reverse() } else { cmp }
        });


        TableBuilder::new(ui)
            .column(Column::exact(40.0))  // Add Button
            .column(Column::remainder()) // Name
            .column(Column::exact(120.0)) // Orig Artist
            .column(Column::exact(120.0)) // Cover Artist
            .column(Column::exact(60.0))  // Plays
            .column(Column::exact(60.0))  // Duration
            .header(20.0, |mut header| {
                header.col(|ui| { ui.label("Add"); });
                header.col(|ui| {
                    if ui.selectable_label(self.current_search_sort == SearchSortOption::Name, "Song Name").clicked() {
                        if self.current_search_sort == SearchSortOption::Name { self.current_search_sort_desc = !self.current_search_sort_desc; }
                        else { self.current_search_sort = SearchSortOption::Name; self.current_search_sort_desc = false; }
                    }
                });
                header.col(|ui| {
                    if ui.selectable_label(self.current_search_sort == SearchSortOption::OriginalArtist, "Original Artist").clicked() {
                        if self.current_search_sort == SearchSortOption::OriginalArtist { self.current_search_sort_desc = !self.current_search_sort_desc; }
                        else { self.current_search_sort = SearchSortOption::OriginalArtist; self.current_search_sort_desc = false; }
                    }
                });
                header.col(|ui| {
                    if ui.selectable_label(self.current_search_sort == SearchSortOption::CoverArtist, "Cover Artist").clicked() {
                        if self.current_search_sort == SearchSortOption::CoverArtist { self.current_search_sort_desc = !self.current_search_sort_desc; }
                        else { self.current_search_sort = SearchSortOption::CoverArtist; self.current_search_sort_desc = false; }
                    }
                });
                header.col(|ui| {
                    if ui.selectable_label(self.current_search_sort == SearchSortOption::Plays, "Plays").clicked() {
                        if self.current_search_sort == SearchSortOption::Plays { self.current_search_sort_desc = !self.current_search_sort_desc; }
                        else { self.current_search_sort = SearchSortOption::Plays; self.current_search_sort_desc = false; }
                    }
                });
                header.col(|ui| {
                    if ui.selectable_label(self.current_search_sort == SearchSortOption::Length, "Length").clicked() {
                        if self.current_search_sort == SearchSortOption::Length { self.current_search_sort_desc = !self.current_search_sort_desc; }
                        else { self.current_search_sort = SearchSortOption::Length; self.current_search_sort_desc = false; }
                    }
                });
            })
            .body(|body| {
                body.rows(20.0, matching_songs.len(), |mut row| {
                    let idx = row.index();
                    let song_uuid = matching_songs[idx];

                    let song_data = songs.get(&song_uuid, |s| (s.title.to_string(), s.original_artists.clone(), s.cover_artists.clone(), s.play_count, s.duration));

                    let (title, orig_artists, cover_artists, play_count, duration) = match song_data {
                        LoadingState::Loaded((t, oa, ca, pc, d)) => (t, oa.join(" & "), ca.join(" & "), pc.unwrap_or(0), d),
                        _ => ("Loading...".to_string(), "-".to_string(), "-".to_string(), 0, None),
                    };

                    row.col(|ui| {
                        let mut is_in_playlist = false;

                        if let Some(playlist) = player.get_playlist() {
                            is_in_playlist = playlist.contains(&song_uuid);
                        }

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
                        if ui.selectable_label(current_song_uuid == &Some(song_uuid), title).clicked() {
                            player.play_song(song_uuid);
                        }
                    });
                    row.col(|ui| { ui.label(orig_artists); });
                    row.col(|ui| { ui.label(cover_artists); });
                    row.col(|ui| { ui.label(play_count.to_string()); });
                    row.col(|ui| {
                        if let Some(dur) = duration {
                            ui.label(format!("{}:{:02}", dur / 60, dur % 60));
                        } else { ui.label("-"); }
                    });
                });
            });
    }

}

pub(crate) fn parse_query(search: &str, is_playlist: bool) -> SearchCriteria {
    let mut original = None;
    let mut cover = None;
    let mut creator = None;
    let mut title_parts = Vec::new();

    let tokens = crate::utilities::util::split_by_space_respecting_quotes(search);

    for token in tokens {
        if !is_playlist {
            if let Some(val) = token.strip_prefix("original:") {
                original = Some(val.to_string());
            } else if let Some(val) = token.strip_prefix("cover:") {
                cover = Some(val.to_string());
            } else {
                title_parts.push(token);
            }
        } else {
            if let Some(val) = token.strip_prefix("creator:") {
                creator = Some(val.to_string());
            } else {
                title_parts.push(token);
            }
        }
    }

    SearchCriteria {
        original,
        cover,
        creator,
        title: title_parts.join(" "),
    }
}