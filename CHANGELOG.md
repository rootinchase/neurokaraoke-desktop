# Changelog

All notable changes to this project will be documented in this file.

## v0.4.4: Synced lyrics

### Added features
* **Synced Lyrics Overlay:** `get_lyrics` (`src/api/client.rs`) fetches `GET /api/songs/{id}/lyrics` lazily, deserializes the JSON array of `{time, text}` entries, parses each .NET `TimeSpan` (`HH:mm:ss.fffffff`) into a `Duration`, sorts by timestamp, and caches a single song slot (`src/utilities/lrc.rs` `parse_time_entries`).
    * A `404` stores an empty sentinel so songs without lyrics are not re-fetched every frame.
    * `has_lyrics` is added to `SongDTO` to gate the fetch.
* **Fullscreen Lyrics Toggle & Overlay:** `render_fullscreen_player` (`src/app/player_ui.rs`) draws a top-left lyrics toggle button (using `assets/lyrics.svg`) that flips the persisted `show_lyrics` config flag.
    * When enabled and the current song has lyrics, the synced lyrics overlay is drawn centered over the cover art: the active line is white at full opacity (2rem), the next line is 0.6 opacity (1.375rem), and past lines are 0.35 opacity (1.125rem).
    * The overlay forces the fullscreen `Area` to the current screen size each frame so it shrinks cleanly when the window is resized smaller.
    * The radio current song's lyrics use the catalog `current.id` (not the nil playback id).

## v0.4.3: Memory improvements

### Added features
* **Fullscreen Playlist View:** Added a full playlist detail page (`render_full_view` in `src/activity/playlist.rs`) that opens when a playlist is selected.
    * A header shows the cover art, title, creator, stats, and action buttons (Play, Shuffle, favorite toggle, kebab "more" menu), followed by a searchable Tracks table with per-song play controls.
    * Wired in `ui.rs` via `selected_playlist_id`.
    * The kebab menu's CopyLink copies the playlist URL to the clipboard, shuffle plays the playlist in random order from a random starting song, and the favorite toggle calls `POST /api/favorites/toggle?itemId={id}&type=3`.
    * The remaining kebab actions (download, save offline, create/add-to playlist, edit, delete) are stubs pending the server endpoints tracked in issue #1.
* **HLS Radio Playback:** `src/audio/hls/` fetches the Shoutcast HLS master and media playlists and resolves relative segment URIs against the playlist URL.
    * MPEG-TS segments are demultiplexed into ADTS AAC frames and decoded to PCM via symphonia.
    * A rodio `HlsSource` fed from an mpsc channel of `HlsChunk` items is appended to the mixer, so HLS radio plays alongside the existing MP3 and Opus paths.
    * `TsDemuxer` is stateful: it caches the resolved audio PID across segments and carries a trailing partial ADTS frame over segment boundaries.
    * This is required because live segments are 188-byte aligned and ADTS frames straddle them.
* **HLS Adaptive Bitrate:** `BandwidthEstimator` (harmonic mean of the last 3 segment download rates, per RFC 8216 §7.2.1).
    * `TierSelector` (highest variant under `estimate × 0.9`, up-switching after 2 consecutive qualifying segments, down-switching immediately).
    * These drive tier choice in `hls_worker` (`src/audio/player.rs`).
    * `PreferredTier` (`Auto`/`Lofi`/`Midfi`/`Hifi`) lets the user lock a tier.
    * The demuxer resets on `#EXT-X-DISCONTINUITY` and on tier switches, and `HlsChunk::Flush` tells the player to drop its buffer so playback resumes cleanly.


### Improvements
* **Repaint Gating:** `ui()` (`src/app/ui.rs`) computes a `needs_continuous` flag (seeker drag, active playback, theme animation, in-flight downloads, open popup) and calls `request_repaint()` only when it is true.
    * Otherwise it uses `request_repaint_after(250ms)`, so idle focused frames no longer run the full egui layout + render pass continuously.
* **Artwork Texture Reuse:** `resolve_and_render_art` (`src/activity/mod.rs`) and `render_fullscreen_player` (`src/app/player_ui.rs`) register each artwork once via egui's `bytes://` URI cache and reuse the cached texture.
    * The disk read runs only on the first frame.
    * The resolved URI is cached in `cached_art_paths` so the `format!` is not repeated, and the fullscreen player reuses the cached texture instead of reading the file every frame.
* **Theme From App State:** `render_playlist_box` and the playlist detail view (`src/activity/playlist.rs`) take the app's `theme` field as a `&ThemeManager` parameter.
    * Instead of calling `Config::read()` (disk read + RON parse) every frame.
* **Search Filter/Sort Allocations:** `src/activity/search.rs` lowercases the query once and uses a non-allocating case-insensitive `contains`.
    * Results are decorate-sorted into a `Vec<(key, Uuid)>` that is cached and invalidated only when the song map, query, or sort option changes, removing the O(n log n) allocations per frame.
* **`sort_items` Key Precompute:** `sort_items` (`src/utilities/util.rs`) computes each item's sort key once into `Vec<(Key, T)>` (decorate-sort-undecorate).
    * `sort_by_key` no longer recomputes `get_name`/`get_date` (which clone/allocate) on every comparison.
* **Cache Lookup Memoization:** `get_cached_path` (`src/utilities/cache.rs`) memorizes the resolved path in a `resolved` DashMap and negative results in a `negative` DashSet.
    * A hit is a single map lookup and a missing artwork is not re-stat-ed every frame.
    * The per-hit `set_modified` syscall is replaced by the in-memory `in_flight_last_used` counter, with mtime flushed to disk only on eviction sweeps.
* **Playlist Display Strings:** the playlist detail loaded state stores precomputed display strings (`Vec<(String, Uuid)>`) and the grid rows use persistent `ui.id().with(playlist.id)` ids.
    * Row labels and `original_artists.join(" & ")` are built once on load, not every frame.
* **Static Sidebar Icons:** `ActivityType::icon()` (`src/activity/mod.rs`) returns references to `LazyLock<ImageSource<'static>>` statics built once per asset.
    * The sidebar no longer rebuilds and re-hashes an `ImageSource` per item per frame.
* **Avatar ImageSource Reuse:** `AvatarState::Ready` stores the `ImageSource<'static>` once when the avatar finishes downloading.
    * The sidebar reuses it with a stable URI and egui's texture cache hits without re-registering, instead of cloning the full JPEG bytes and formatting the URI every frame.
* **4-Worker Runtime:** `src/main.rs` uses `worker_threads(4)` instead of 2, so API calls, downloads, prefetch, Discord presence, and eviction sweeps no longer compete for the same two workers.
* **Shared Reqwest Client:** the audio worker (`src/audio/player.rs`) receives the shared `Arc<Client>` from `create_app` instead of building its own `Client` per loop.
    * Stream reconnects reuse the shared connection pool.
* **Single Logout Clear:** the Logout branch (`src/app/ui.rs`) calls `self.songs.map.clear()` once instead of twice.
* **Mosaic Cover Slice:** the playlist detail loaded state stores the valid mosaic slice (`Vec<Arc<Artwork>>`) computed once on load.
    * `render_mosaic`/`render_playlist_art_advanced` render from the stored slice instead of rebuilding `Vec<&Artwork>` every frame.
* **Audio Worker Arc Hoisting:** The audio worker loop in `src/audio/player.rs` cloned `player`, `rt`, `ctx`, `database`, `cache`, `rx`, and `client` on every iteration.
    * The `catch_unwind` closure captured fresh `Arc` clones each time.
    * The clones are now built once outside the loop and borrowed inside the `AssertUnwindSafe` closure, removing the per-iteration `Arc` increment overhead.
* **Event-Driven Search Font Scanning:** `src/activity/search.rs` now calls `fonts::ensure_str` only when the search `TextEdit` reports `changed()`.
    * Instead of scanning every character of the query string on every repaint.
    * `detect_scripts` runs only when the text actually changes, so idle search frames no longer pay for the script scan.
* **Gated Memory Logging:** The background eviction sweep in `src/app/factory.rs` no longer writes to the debug log on every tick.
    * Memory logging is gated behind the `DEBUG_MEMORY` environment variable and emits only when the `song_map`/`art_paths`/`in_flight` counts change.
    * The sweep stays silent during steady-state operation.
* **Bounded Auth Error Reads:** `handle_error_response` in `src/auth/service.rs` now reads the response body with `bytes()` and truncates it to 4 KiB before formatting the error.
    * A misbehaving endpoint cannot cause an unbounded body read on every auth failure.
* **Debug Log Username Redaction:** `redact_username` (`src/utilities/util.rs`) masks usernames written to `debug.log`.
    * Names of six characters or more keep their first two and last two characters.
    * Someone who already knows the username can confirm a match while outsiders cannot reconstruct it (`rootinchase` → `ro•••••••se`).
    * Names of five characters or fewer are reduced to the first character plus a mask, because keeping a suffix would expose the whole name, and single-character names are masked entirely.
    * The auth login log line in `src/auth/service.rs` now uses it.
* **CodeQL Triage:** all 21 CodeQL alerts reviewed.
    * Of the 10 open alerts, 9 (`src/utilities/cache.rs` lines 188/199/203/218/224 and `src/audio/player.rs` lines 832/870/969/988) log public Cloudflare image and song UUIDs.
    * These match the existing false-positive dismissals for alerts 1-5, 7, 8, 18 and 19.
    * Alert 6 (`src/auth/service.rs:86`) flagged the username and is addressed by the redaction above.
* **Home Memory Release:** `HomeActivity::clear()` (`src/activity/home.rs`) resets the suggested, trending, and setlist loading states back to `Loading` and clears the setlist name.
    * The loaded `Vec<SongDTO>` payloads are dropped from RAM.
    * `ui()` (`src/app/ui.rs`) captures the activity before the sidebar render and, when it changes, calls `clear()` on navigating away from Home and re-fetches suggested/trending/setlist on reopening Home.
    * The home data no longer sits in memory while the user is on another page.
* **Search Memory Release:** `SearchActivity::clear()` (`src/activity/search.rs`) clears the query text, resets the sort option/direction, and drops the tracked song.
    * The search state is released when the user navigates away from Search.
    * The same activity-change detection in `ui()` calls it on leaving Search, and Search starts fresh the next time it is opened.
* **Profile Memory Release:** `ProfileActivity::clear()` (`src/activity/profile.rs`) drops the badges, badge images, user limits, and login form inputs when the user navigates away from Profile.
    * `profile_data`/`avatar_state` are kept because the sidebar renders the avatar on every page.
    * `ProfileActivity::refetch()` re-fetches badges and limits from the server on reopening Profile, and the activity-change detection in `ui()` drives both.
* **Playlists Memory Release:** `PlaylistActivity::clear_sync()` (`src/activity/playlist.rs`) drops the loaded playlist lists, all-playlists cache, selected playlist, the `playlist_details` DashMap, and the search/menu state.
    * This runs when the user navigates away from Playlists or My Playlists (the shared `favorite_playlists` set is kept).
    * `PlaylistActivity::refetch()` re-fetches the playlist list on reopening, mirroring the fetch in `new()`.
    * The activity-change detection in `ui()` drives both for `Playlists` and `MyPlaylists`.
* **Playlist Cover-Art Texture Release:** `clear_sync()` now also frees every cover-art texture egui registered for the rendered playlists.
    * Scrolling a large playlist list registers a `bytes://<cached_path>` texture per cover in egui's texture store, and clearing the playlist data alone does not drop those GPU-side textures.
    * Memory stays high after scrolling.
    * `cover_art_uri()` (`src/activity/mod.rs`) recomputes the exact URI `resolve_and_render_art` uses for each playlist's `media` and `mosaic_media`.
    * `clear_sync()` calls `Context::forget_image(uri)` for each one.
    * `ui()` passes `ui.ctx()` into `clear_sync()` to drive it.
* **Setlists Memory and Cover-Art Texture Release:** `SetlistActivity::clear_sync()` (`src/activity/setlist.rs`) drops the loaded setlist list, the `setlist_details` DashMap, the selected setlist, and the last total count.
    * This runs when the user navigates away from Setlists.
    * It also frees every cover-art texture egui registered for the rendered setlists via `Context::forget_image` — the same texture-release treatment as Playlists.
    * `current_year` and `available_years` are kept so `SetlistActivity::refetch()` reloads the same year on reopening.
    * The activity-change detection in `ui()` drives both for `Setlists`.
    * `cover_art_uri()` was moved to `src/activity/mod.rs` as a shared `pub` helper so both `playlist.rs` and `setlist.rs` reuse the exact URI `resolve_and_render_art` registers.
* **Favorites Memory and Cover-Art Texture Release:** `FavoritesActivity::clear_sync()` (`src/activity/favorites.rs`) drops the loaded favorite playlists and songs lists, the `playlist_details` DashMap, the selected playlist, and the fetching flag.
    * This runs when the user navigates away from Favorites.
    * It also frees every cover-art texture egui registered for the rendered playlists' `media`/`mosaic_media` and the favorite-songs mosaic tiles' `cover_art` via `Context::forget_image`.
    * `favorite_songs`/`favorite_playlists` are kept because the player UI and playlist detail view read them on every page.
    * `FavoritesActivity::refetch()` re-runs `fetch_favorites()` on reopening, and the activity-change detection in `ui()` drives both.
    * The existing async `clear()` (which also flushes `favorite_songs`) remains the logout path.
* **Uploads Memory Release:** `UploadsActivity::clear()` (`src/activity/uploads.rs`) drops the loaded uploads list, the user quota, the sort option/direction, the URL panel and its input, the pending file list, and the upload feedback.
    * This runs when the user navigates away from Uploads.
    * Uploads renders no cover art, so there are no textures to free.
    * `UploadsActivity::refetch()` re-runs `fetch_uploads()` on reopening, and the activity-change detection in `ui()` drives both.
* **Settings Memory Release:** `SettingsActivity::clear()` (`src/activity/settings.rs`) drops the cached storage figures and the clear-cache status when the user navigates away from Settings.
    * `SettingsActivity::refetch()` re-reads the storage figures on reopening.
    * The activity-change detection in `ui()` drives both.
    * Radio is intentionally left as-is: its polling loop repopulates the live now-playing state within seconds regardless of the active page.
    * Clearing it on navigation away yields no memory benefit.
* **Cover-Art Interning:** `Song.cover_art`, `SongDTO.cover_art`, `Playlist.media`, and `Playlist.mosaic_media` are now `Arc<Artwork>`.
    * They are shared through a new `LazySongDatabase::artwork_cache` (`Arc<DashMap<String, Arc<Artwork>>>`) keyed by artwork id.
    * `intern_artwork()` returns the shared `Arc<Artwork>` for a given id (inserting on first sight, cloning the existing on repeat).
    * `intern_song` / `intern_song_dto` / `intern_playlist` / `intern_playlist_detail` apply it.
    * Every fetch site (`get`, `load`, public/user playlists, official setlists, favorite playlists/songs, user uploads, playlist detail, suggested, trending, radio current state) interns its cover art.
    * Every disk-cache deserialization site (`setlist.rs`, `favorites.rs`, `playlist.rs`, `home.rs` `CachedSetlist`, `util.rs` `load_cached_song_list` + `get_playlist_details_cached`) interns its cover art.
    * Songs and playlists that reference the same artwork id share one allocation instead of duplicating the `Artwork` metadata per song/playlist.
    * Render sites use `Arc` deref coercion.
    * Explicit `as_deref()` where code builds `Vec<&Artwork>` (`src/activity/mod.rs`), `Arc::new(...)` for constructed cover art (`src/app/player_ui.rs`), and `.as_ref()` for the `cloudflare_id` lookup (`src/app/state.rs`).

### Bug fixes
* **Removed Raw Auth Response Logging:** the inbound raw auth response body and the outbound JSON payload are no longer written to `debug.log` (`src/auth/service.rs`).
    * The inbound body contains the JWT session token and the outbound payload contains the Discord `access_token`, so both were plaintext credential leaks in a local log file.
    * CodeQL flagged neither.
* **Playlists Scroll Crash:** the virtualized playlist grid (`src/activity/playlist.rs`) computed the slice start index from the persisted `last_scroll_y` without clamping it to the list length.
    * Rapid scrolling stores a deep offset, and when the list shrinks underneath it (search filter, sort, load-more reset), `start > len` made `sorted_playlists[start..end]` panic with "range start index 42 out of range for slice of length 20".
    * A shared `visible_range()` helper (`src/activity/mod.rs`) clamps `start` to `len` so the slice is empty instead of crashing.
    * The setlist grid (`src/activity/setlist.rs`) had the identical latent bug and now uses the same helper.
* **HLS MPEG-TS audio demuxing:** `demux_ts` (`src/audio/hls/client.rs`) parsed the TS header with the wrong bit layout, reading the PID as `((b1 & 0x1f) << 8) | (b2 & 0x3f)` and the adaptation-field control from `b2 >> 6`.
    * In MPEG-TS the PID is the 13 low bits of bytes 1–2, the adaptation-field control is the **high nibble of byte 3**, and the adaptation-field length byte is **byte 4**.
    * The payload offset `4 + packet[3]` was one byte short and every audio payload was sliced one byte early.
    * The ADTS sync scan then landed on shifted bytes and misread the frame-length field, so no frames decoded.
    * The parser now follows ffmpeg's `mpegts.c` (`p = packet + 4; if (has_adaptation) p += p[0] + 1`), a payload exists only when `afc & 1`, and the demuxed ADTS is byte-identical to ffmpeg's extraction.
    * `oxideav-mpegts`'s `TsPacket::parse` cannot be used here: it returns an empty payload when `afc & 1 == 0`.
    * That rejects Shoutcast's non-standard packets (the adaptation-field control and continuity counter share byte 3).
* **HLS first-segment audio dropped:** `find_audio_pid` collected audio packets in a single pass before the audio PID was known, so the first segment's audio was discarded.
    * It now walks the PAT to find the PMT PID, then the PMT to find the audio elementary-stream PID (stream type `0x0f`), and the demuxer caches that PID for later segments.

### Testing
* Added `test_redact_username_keeps_prefix_and_suffix`, `test_redact_username_masks_short_names` and `test_redact_username_is_unicode_safe` (`src/utilities/util.rs`).
    * Bringing the suite to 52 passing tests with 5 ignored
* Added `test_visible_range_top_of_list`, `test_visible_range_mid_list`, `test_visible_range_deep_offset_shrunk_list`, `test_visible_range_empty_list` and `test_visible_range_end_never_exceeds_len` (`src/activity/mod.rs`).
    * Covering the virtualized grid slice helper including the rapid-scroll/shrunk-list crash case, bringing the suite to 68 passing tests with 5 ignored
* Added `intern_artwork_shares_one_arc_per_id`, `intern_artwork_keeps_distinct_ids_separate`, `intern_artwork_none_stays_none`, `intern_song_dto_shares_artwork_across_dtos`, `intern_song_shares_artwork_with_dto`, `intern_playlist_interns_media_and_mosaic`, `intern_playlist_detail_shares_artwork` and `default_cover_art_interns_to_one_arc` (`src/api/client.rs`).
    * Proving cover-art interning: the same artwork id yields one shared `Arc<Artwork>` (`Arc::ptr_eq`) across songs, DTOs, playlist media, mosaic covers, and playlist-detail songs.
    * Distinct ids stay separate; `None` stays `None`; and every default cover art shares one interned `Arc`, bringing the suite to 78 passing tests with 5 ignored
* Added `demuxed_adts_matches_ffmpeg_reference_and_decodes`, `all_tiers_demux_to_ffmpeg_reference`, `tier_switch_re_resolves_audio_pid`, `abr_session_climbs_tiers_and_stays_continuous`, `ts_demuxer_matches_one_shot_demux`, `ts_demuxer_survives_live_segment_split`, `ts_demuxer_carries_partial_frame_across_segments`, `ts_demuxer_reset_leaves_no_state`, `live_split_feeding_decodes_to_identical_pcm`, `segment_without_pat_pmt_yields_no_adts`, `demuxes_real_ts_into_adts`, `decodes_real_ts_segment_full` and `demuxed_adts_excludes_shoutcast_id3_metadata` (`src/audio/hls/client.rs`).
    * Proving the demuxed ADTS is byte-identical to ffmpeg on all three ABR tiers (lofi 25124, midfi 49073, hifi 97111 bytes, 172 AAC-LC frames each at 44100 Hz stereo).
    * Partial ADTS frames survive segment boundaries, a tier switch re-resolves the audio PID without losing samples, and Shoutcast's PID 257 `timed_id3` metadata never leaks into the ADTS stream.
    * Real lofi/midfi/hifi segments are checked in as fixtures under `raw_json/hls/`, bringing the suite to 100 passing tests with 5 ignored

### Security
* **Dropped the GTK tray backend (RUSTSEC-2024-0429 + RUSTSEC-2024-0370):** `tray-icon` is now declared with `default-features = false` (`Cargo.toml`), keeping only the `ksni` feature.
    * The default `libappindicator` backend pulled in `gtk → glib → glib-macros`, which carried two RustSec advisories.
    * The `glib::VariantStrIter` iterator unsoundness (RUSTSEC-2024-0429, patched only in glib ≥ 0.20) and the unmaintained `proc-macro-error` (RUSTSEC-2024-0370, via `glib-macros`).
    * Disabling the default features removes the entire gtk/glib stack, so `cargo audit` no longer reports them.
    * The `ksni` backend (D-Bus StatusNotifierItem) was already enabled and is display-server agnostic, so the tray keeps working on X11 and Wayland.
    * Windows/macOS are unaffected (their tray backends are unconditional and not gated by the dropped feature).

---

## v0.4.2: Uploads and more

### Added features
* **Fullscreen Player:** Added a fullscreen player overlay toggled from the player controls (fullscreen icon, fullscreen-exit icon when active, Esc to exit).
    * Shows the full-resolution cover artwork filling the screen with the embedded play/pause, seek, volume, favorite, timer and queue controls
* **My Uploads:** Added a dedicated view for the user's own uploaded songs, fetched from the server with cache-first loading.
    * Cover-art mosaic, sortable columns (title, plays, date), and per-song play/queue controls
* **Lazy Font Loading:** The Noto JP/KR/SC/Cuneiform fallback fonts are embedded gzip-compressed and only decompressed and registered with egui when text containing their scripts appears in song titles, artists, playlists, or playlist details.
    * Startup stays lightweight.
    * Latin text keeps rendering with Roboto and only missing glyphs come from the Noto fallbacks
* **File-Based Song Upload API:** `LazySongDatabase::upload_song_file(FileUpload)` posts `multipart/form-data` to `POST https://idk.neurokaraoke.com/api/user/song/upload`.
    * The `File` part is typed by extension (`audio/mpeg` for `.mp3`) plus `Title` and `Artist` text parts typed `text/plain; charset=utf-8`.
    * Matching the web app's form exactly.
    * The server transcodes the upload to `user-audio/{guid}.m4a` and answers `200` with an empty body, so success is detected from the status alone.
    * `UserLimits::{remaining_storage_bytes, has_song_slot, can_fit}` cover the quota check.
    * Verified live against the server. UI wiring is intentionally not included yet
* **YouTube URL Validation:** `classify_source_url` (`src/api/sources.rs`) validates source links offline before `LazySongDatabase::upload_song_from_url` posts to `POST https://idk.neurokaraoke.com/api/user/song/download-from-url`.
    * Reproduces the server's `400` wording verbatim (`Invalid URL format`, `Unsupported platform. Only YouTube, Bilibili and Discord attachments are supported.`, `Invalid YouTube URL. Must be a video or playlist.`).
    * Recognised forms cover `watch?v=`, `youtu.be`, `shorts`, `live`, `embed`, `v`, `playlist?list`, `m.`/`music.`/`youtube-nocookie` hosts, Bilibili `BV…`/`av…` and `b23.tv` short links, and Discord attachment URLs.
    * The payload field is `playlistId` (the old `playlist_url` model was wrong) and the new `UploadSongResult` parses `{"success":true,"songId":…,"title":…}`.
    * Verified live against the server
* **Uploaded Song Removal:** `LazySongDatabase::delete_uploaded_song(song_id)` sends `DELETE https://idk.neurokaraoke.com/api/user/song/{id}` and treats `204 No Content` as success.
    * The generic `DELETE /api/songs/{id}` on the API host answers `200` with an empty body and leaves the song in place for regular users, so the owner-scoped IDK route is the one that deletes.
    * Verified live (upload then delete restored `currentSongCount` to 15)
* **Uploads UI:** The My Uploads page manages uploads end to end.
    * A "DOWNLOAD FROM YOUTUBE" panel validates the URL offline with `classify_source_url` and shows the server's wording for each rejection, with Songs/Storage quota bars from `UserLimits`.
    * A drop zone accepts audio files dragged from the OS (filtered to the extensions `audio_content_type` accepts).
    * Files can also be opened through the system file picker via `rfd` (run on the tokio blocking pool, so the window keeps animating while the dialog is open).
    * Editable Title/Artist rows for every pending file with an Upload button, and a trash button that deletes an uploaded song through the owner-scoped `DELETE /api/user/song/{id}`.
    * Quota gating uses `UserLimits::has_song_slot` for URL uploads and `UserLimits::can_fit(size)` for file uploads.
    * A successful URL download closes the panel and clears the field, and the songs list refreshes after every upload, delete, and URL download.
* **OS Drag and Drop:** `ViewportBuilder::with_drag_and_drop(true)` is set on the main window and dropped files are read from `ctx.input(|i| i.raw.dropped_files)`.
    * The drop zone border and icon light up while a drag hovers the window.
    * winit 0.30.13 delivers X11 drops (`WindowEvent::DroppedFile` + XDND, confirmed live by the `XdndAware` window property) and Windows drops.
    * **Wayland drag-and-drop needs the unreleased winit 0.31.x**, which adds `data_device` support, so drops are ignored on Wayland for now.

### Improvements
* Added a separate full-resolution artwork cache asset (`AssetType::FullImage`) using the Cloudflare Images `public` variant.
    * Downloaded lazily only when fullscreen is opened so the 512x512 thumbnails are never served as fullscreen artwork
* Relocated My Playlists, Favorites, and My Uploads to the bottom of the sidebar navigation list, separated from the main navigation by a divider
* **Font Compression:** `build.rs` now gzip-compresses the lazy Noto fonts at the highest level (zlib level 9, smaller than `gzip -9`) into `assets/fonts/*.ttf.gz`.
    * Cached in place by mtime so incremental builds reuse existing archives instead of recompressing
* **Uploads Row Layout:** pending upload rows use fixed field sizes (Title 170×24, Artist 130×24) so the Upload button is never clipped by a long filename
* **Playlists Cache-First Refetch:** `PlaylistActivity::refetch()` (`src/activity/playlist.rs`) renders the cached public playlists immediately on reopening, then refreshes from the server in the background.
    * Previously `refetch()` went straight to the network, so re-entering the playlists page blocked on a ~12s fetch of 500 playlists before anything appeared.
    * A new `load_cached_playlists()` helper (shared with `new()`) reads the tier-2 disk cache, interns the cover art, and rebuilds the `mosaic_covers` grid from the serialized `mosaic_media` (`mosaic_covers` is `#[serde(skip)]`).
    * The grid, fonts, and `has_more` flag render instantly from the cache; the server refresh updates the list in place and keeps the cached display on network failure.

---

## v0.4.1: Radio

### Added features
* **Radio Mode:** Introduced dedicated Radio functionality with persistent volume control and smooth playback switching between Radio and normal tracks
* **Cache Management:** Added time- and storage-based cache eviction, automatic TTL updates on repeated cache hits, and cleanup of stale downloads on start and exit
* **Default Cover Art:** Added default cover art fallback for songs lacking cover art metadata
* **Debugging & Network:** Added debug log file output and proper User Agent headers for network requests

### Improvements
* **UI/UX Refinements:**
  * Scrollable user profile view
  * Relocated sleep timer popup above player controls
  * Enforced minimum window size constraints
* **Codebase & Testing:**
  * Comprehensive code deduplication and modular cleanup
  * Added robust unit tests and isolated cache testing environments
  * Optimized GitHub CI compilation speed

### Bug fixes
* Fixed Playwire audio and terminal window handling on Windows

---

## v0.4.0: The first great rewrite

### Added features
* Korean and Chinese font support for correct rendering of all song names
* Improved song search and sorting with prefix-based filters:
  * Cover artists: `cover:Evil`
  * Original artists: `original:"Ellie Minibot"`
  * Combine multiple filters with spaces: `cover:"Evil" original:"Ellie Minibot" Stupid Heart`
* Session restoration
* Suggested songs for users
* Trending songs
* Card-based playlist and setlists view
* Dedicated Settings screen
* Sleep timer remaining duration display on hover
* System tray integration (Play/Pause, Switch theme)
* Compact sidebar spacing and background setlist retrieval improvements

### Improvements
* Major codebase refactor for modularity
* Tier-2 playlist caching and preemptive audio fetching
* Better offline support for cached items
* Refined volume slider (converted to a progress bar synchronized with config)

### Bug fixes
* Fixed search functionality with cache integration (restored ability to add songs from search)
* Fixed playlist skip logic to properly loop around (both forward and backward across all loop modes)
* Fixed shuffle out-of-bounds crash when shuffling playlists with added/removed items
* Fixed cache restoration after `--clear-cache`
* Fixed metadata and system shuffle control desync issues
* Fixed duplicate titles and sidebar spacing in UI

---

## v0.3.8
* **Discord RPC:** Integrated Discord Rich Presence.
* **Authentication:** Added username/password login.
* **Playlist & Setlists Listing Improvements:**
  * View more entries in playlist listings
  * Sort playlists by Name, Play count, Created date, and Updated date
  * Shrinks playlist selection dropdown upon choosing a playlist
  * View playlists/setlists by year
* **Caching & Playwire:** Playwire now passes the cached image file path upon image caching (passing URL beforehand).
* **Logout:** Cache is now cleared upon logout.

---

## v0.3.7:
* **Sleep Timer:** Highly customizable sleep timer supporting:
  * Preset times: 5, 15, 30, 60, 120 minutes
  * Custom time specified in minutes or cut-off time
* **UI Fixes:** Fixed icon colors across the application.

---

## v0.3.6:
* **Profile:** Upgraded profile view to display badges, levels, and limits.
* **Audio Fixes:** Fixed temporary stream audio not working.

---

## v0.3.5:
* **Favorites:** Added ability to favorite and unfavorite songs directly.
* **Icons:** Added MIT-licensed icons for polished menu visuals.

---

## v0.3.4:
* **Playback Reporting:** Added playback reporting with a 30-second continuous playback threshold before reporting plays.
* **Bug Fixes:** Fixed shuffle button visual update bug.
* **CI/CD:** Automated CI/CD release pipeline for macOS, Windows, and Linux (x86_64 / amd64 and arm64 / aarch64 builds).

---

## v0.3.3:
* **Playlist Tables:** Rich table views for playlists showing song count, play count, and creator information.
* **Official Setlists:** Added table format to official setlists displaying songs, play button, and stream date.
* **User Favorites:** Redid user favorites page to show user songs as a playlist alongside favorite playlists.

---

## v0.3.2:
* **Favorites Playback:** Added playback support for user's favorite songs.
* **Auth Guard:** Required user authentication for user favorites and playlists, hiding buttons when not logged in.

---

## v0.3.1
* **User Playlists & Uploaded Songs:** Added support for pulling personal playlists and uploaded songs.
* **Authentication:** Added Discord authentication.
* **Caching & Navigation:** Added image caching, song cache management, official setlists, public playlists, and repeat button improvements.
