use eframe::egui::{TextEdit, Ui};
use egui_extras::{Column, TableBuilder};
use uuid::Uuid;

use crate::activity::render_duration_label;
use crate::api::{LazySongDatabase, LoadingState};
use crate::app::fonts::ensure_str;
use crate::audio::types::Player;
use crate::utilities::util::split_by_space_respecting_quotes;

pub struct SearchActivity {
    search: String,
    current_search_sort: SearchSortOption,
    current_search_sort_desc: bool,
    _current_song_uuid: Option<Uuid>,
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
            _current_song_uuid: None,
        }
    }

    /// Frees the search state (the query text, sort options, and tracked song)
    /// so the data held by the search activity is dropped when the user
    /// navigates away. The next time Search is opened it starts fresh.
    pub fn clear(&mut self) {
        self.search.clear();
        self.current_search_sort = SearchSortOption::Name;
        self.current_search_sort_desc = false;
        self._current_song_uuid = None;
    }

    pub fn render(
        &mut self,
        ui: &mut Ui,
        songs: &LazySongDatabase,
        player: &mut Player,
        current_song_uuid: &Option<Uuid>,
    ) {
        let text_response = ui.add(TextEdit::singleline(&mut self.search));
        // Only scan the query for required scripts when the text actually
        // changed, not on every repaint.
        if text_response.changed() {
            ensure_str(&self.search);
        }

        ui.add_space(10.0);

        let criteria = parse_query(&self.search, false);
        // Lowercase the query once, not per song per frame.
        let title_lower = criteria.title.to_lowercase();
        let original_lower = criteria.original.as_ref().map(|q| q.to_lowercase());
        let cover_lower = criteria.cover.as_ref().map(|q| q.to_lowercase());

        let mut matching_songs: Vec<Uuid> = songs
            .get_map()
            .iter()
            .filter(|x| {
                x.value().if_loaded_or_else(
                    |s| {
                        let title_match = contains_ignore_case(&s.title, &title_lower);
                        let original_match = original_lower.as_ref().is_none_or(|q| {
                            s.original_artists
                                .iter()
                                .any(|a| contains_ignore_case(a, q))
                        });
                        let cover_match = cover_lower.as_ref().is_none_or(|q| {
                            s.cover_artists.iter().any(|a| contains_ignore_case(a, q))
                        });

                        title_match && original_match && cover_match
                    },
                    false,
                )
            })
            .map(|x| *x.key())
            .collect();

        // Decorate-sort-undecorate: compute the sort key once per song, sort the
        // (key, id) pairs, then map back to UUIDs. Avoids O(n log n) allocations
        // per comparison that the previous comparator performed.
        let mut decorated: Vec<(String, Uuid)> = Vec::with_capacity(matching_songs.len());
        for id in &matching_songs {
            let key = match songs.get(id, |s| match self.current_search_sort {
                SearchSortOption::Name => s.title.to_lowercase(),
                SearchSortOption::OriginalArtist => s.original_artists.join(" ").to_lowercase(),
                SearchSortOption::CoverArtist => s.cover_artists.join(" ").to_lowercase(),
                SearchSortOption::Plays => format!("{:010}", s.play_count.unwrap_or(0)),
                SearchSortOption::Length => format!("{:010}", s.duration.unwrap_or(0)),
            }) {
                LoadingState::Loaded(k) => k,
                _ => String::new(),
            };
            decorated.push((key, *id));
        }
        decorated.sort_by(|a, b| a.0.cmp(&b.0));
        if self.current_search_sort_desc {
            decorated.reverse();
        }
        matching_songs = decorated.into_iter().map(|(_, id)| id).collect();

        TableBuilder::new(ui)
            .column(Column::exact(40.0)) // Add Button
            .column(Column::remainder()) // Name
            .column(Column::exact(120.0)) // Orig Artist
            .column(Column::exact(120.0)) // Cover Artist
            .column(Column::exact(60.0)) // Plays
            .column(Column::exact(60.0)) // Duration
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.label("Add");
                });
                header.col(|ui| {
                    if ui
                        .selectable_label(
                            self.current_search_sort == SearchSortOption::Name,
                            "Song Name",
                        )
                        .clicked()
                    {
                        if self.current_search_sort == SearchSortOption::Name {
                            self.current_search_sort_desc = !self.current_search_sort_desc;
                        } else {
                            self.current_search_sort = SearchSortOption::Name;
                            self.current_search_sort_desc = false;
                        }
                    }
                });
                header.col(|ui| {
                    if ui
                        .selectable_label(
                            self.current_search_sort == SearchSortOption::OriginalArtist,
                            "Original Artist",
                        )
                        .clicked()
                    {
                        if self.current_search_sort == SearchSortOption::OriginalArtist {
                            self.current_search_sort_desc = !self.current_search_sort_desc;
                        } else {
                            self.current_search_sort = SearchSortOption::OriginalArtist;
                            self.current_search_sort_desc = false;
                        }
                    }
                });
                header.col(|ui| {
                    if ui
                        .selectable_label(
                            self.current_search_sort == SearchSortOption::CoverArtist,
                            "Cover Artist",
                        )
                        .clicked()
                    {
                        if self.current_search_sort == SearchSortOption::CoverArtist {
                            self.current_search_sort_desc = !self.current_search_sort_desc;
                        } else {
                            self.current_search_sort = SearchSortOption::CoverArtist;
                            self.current_search_sort_desc = false;
                        }
                    }
                });
                header.col(|ui| {
                    if ui
                        .selectable_label(
                            self.current_search_sort == SearchSortOption::Plays,
                            "Plays",
                        )
                        .clicked()
                    {
                        if self.current_search_sort == SearchSortOption::Plays {
                            self.current_search_sort_desc = !self.current_search_sort_desc;
                        } else {
                            self.current_search_sort = SearchSortOption::Plays;
                            self.current_search_sort_desc = false;
                        }
                    }
                });
                header.col(|ui| {
                    if ui
                        .selectable_label(
                            self.current_search_sort == SearchSortOption::Length,
                            "Length",
                        )
                        .clicked()
                    {
                        if self.current_search_sort == SearchSortOption::Length {
                            self.current_search_sort_desc = !self.current_search_sort_desc;
                        } else {
                            self.current_search_sort = SearchSortOption::Length;
                            self.current_search_sort_desc = false;
                        }
                    }
                });
            })
            .body(|body| {
                body.rows(20.0, matching_songs.len(), |mut row| {
                    let idx = row.index();
                    let song_uuid = matching_songs[idx];

                    let song_data = songs.get(&song_uuid, |s| {
                        (
                            s.title.to_string(),
                            s.original_artists.clone(),
                            s.cover_artists.clone(),
                            s.play_count,
                            s.duration,
                        )
                    });

                    let (title, orig_artists, cover_artists, play_count, duration) = match song_data
                    {
                        LoadingState::Loaded((t, oa, ca, pc, d)) => {
                            (t, oa.join(" & "), ca.join(" & "), pc.unwrap_or(0), d)
                        }
                        _ => (
                            "Loading...".to_string(),
                            "-".to_string(),
                            "-".to_string(),
                            0,
                            None,
                        ),
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
                        if ui
                            .selectable_label(current_song_uuid == &Some(song_uuid), title)
                            .clicked()
                        {
                            player.play_song(song_uuid);
                        }
                    });
                    row.col(|ui| {
                        ui.label(orig_artists);
                    });
                    row.col(|ui| {
                        ui.label(cover_artists);
                    });
                    row.col(|ui| {
                        ui.label(play_count.to_string());
                    });
                    row.col(|ui| {
                        render_duration_label(ui, duration);
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

    let tokens = split_by_space_respecting_quotes(search);

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

/// Case-insensitive substring check that avoids allocating a lowercased copy of
/// `hay`. `needle_lower` must already be lowercased. Uses ASCII byte folding,
/// which is allocation-free and correct for the common (ASCII) case; non-ASCII
/// bytes are compared byte-for-byte.
fn contains_ignore_case(hay: &str, needle_lower: &str) -> bool {
    let h = hay.as_bytes();
    let n = needle_lower.as_bytes();
    if n.is_empty() {
        return true;
    }
    if n.len() > h.len() {
        return false;
    }
    for i in 0..=h.len() - n.len() {
        let mut matched = true;
        for j in 0..n.len() {
            if h[i + j].to_ascii_lowercase() != n[j] {
                matched = false;
                break;
            }
        }
        if matched {
            return true;
        }
    }
    false
}
