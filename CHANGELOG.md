# Changelog

All notable changes to this project will be documented in this file.

## v0.4.3

### Improvements
* **Debug Log Username Redaction:** `redact_username` (`src/utilities/util.rs`) masks usernames written to `debug.log`. Names of six characters or more keep their first two and last two characters, so someone who already knows the username can confirm a match while outsiders cannot reconstruct it (`rootinchase` → `ro•••••••se`). Names of five characters or fewer are reduced to the first character plus a mask, because keeping a suffix would expose the whole name, and single-character names are masked entirely. The auth login log line in `src/auth/service.rs` now uses it.
* **CodeQL Triage:** all 21 CodeQL alerts reviewed. Of the 10 open alerts, 9 (`src/utilities/cache.rs` lines 188/199/203/218/224 and `src/audio/player.rs` lines 832/870/969/988) log public Cloudflare image and song UUIDs, matching the existing false-positive dismissals for alerts 1-5, 7, 8, 18 and 19. Alert 6 (`src/auth/service.rs:86`) flagged the username and is addressed by the redaction above.
* **Home Memory Release:** `HomeActivity::clear()` (`src/activity/home.rs`) resets the suggested, trending, and setlist loading states back to `Loading` and clears the setlist name, dropping the loaded `Vec<SongDTO>` payloads from RAM. `ui()` (`src/app/ui.rs`) captures the activity before the sidebar render and, when it changes, calls `clear()` on navigating away from Home and re-fetches suggested/trending/setlist on reopening Home, so the home data no longer sits in memory while the user is on another page.
* **Search Memory Release:** `SearchActivity::clear()` (`src/activity/search.rs`) clears the query text, resets the sort option/direction, and drops the tracked song, so the search state is released when the user navigates away from Search. The same activity-change detection in `ui()` calls it on leaving Search, and Search starts fresh the next time it is opened.
* **Profile Memory Release:** `ProfileActivity::clear()` (`src/activity/profile.rs`) drops the badges, badge images, user limits, and login form inputs when the user navigates away from Profile, while keeping `profile_data`/`avatar_state` because the sidebar renders the avatar on every page. `ProfileActivity::refetch()` re-fetches badges and limits from the server on reopening Profile, and the activity-change detection in `ui()` drives both.
* **Playlists Memory Release:** `PlaylistActivity::clear_sync()` (`src/activity/playlist.rs`) drops the loaded playlist lists, all-playlists cache, selected playlist, the `playlist_details` DashMap, and the search/menu state when the user navigates away from Playlists or My Playlists (the shared `favorite_playlists` set is kept). `PlaylistActivity::refetch()` re-fetches the playlist list on reopening, mirroring the fetch in `new()`. The activity-change detection in `ui()` drives both for `Playlists` and `MyPlaylists`.
* **Playlist Cover-Art Texture Release:** `clear_sync()` now also frees every cover-art texture egui registered for the rendered playlists. Scrolling a large playlist list registers a `bytes://<cached_path>` texture per cover in egui's texture store, and clearing the playlist data alone does not drop those GPU-side textures, so memory stays high after scrolling. `cover_art_uri()` (`src/activity/mod.rs`) recomputes the exact URI `resolve_and_render_art` uses for each playlist's `media` and `mosaic_media`, and `clear_sync()` calls `Context::forget_image(uri)` for each one. `ui()` passes `ui.ctx()` into `clear_sync()` to drive it.
* **Setlists Memory and Cover-Art Texture Release:** `SetlistActivity::clear_sync()` (`src/activity/setlist.rs`) drops the loaded setlist list, the `setlist_details` DashMap, the selected setlist, and the last total count when the user navigates away from Setlists, and frees every cover-art texture egui registered for the rendered setlists via `Context::forget_image` — the same texture-release treatment as Playlists. `current_year` and `available_years` are kept so `SetlistActivity::refetch()` reloads the same year on reopening. The activity-change detection in `ui()` drives both for `Setlists`. `cover_art_uri()` was moved to `src/activity/mod.rs` as a shared `pub` helper so both `playlist.rs` and `setlist.rs` reuse the exact URI `resolve_and_render_art` registers.
* **Favorites Memory and Cover-Art Texture Release:** `FavoritesActivity::clear_sync()` (`src/activity/favorites.rs`) drops the loaded favorite playlists and songs lists, the `playlist_details` DashMap, the selected playlist, and the fetching flag when the user navigates away from Favorites, and frees every cover-art texture egui registered for the rendered playlists' `media`/`mosaic_media` and the favorite-songs mosaic tiles' `cover_art` via `Context::forget_image`. `favorite_songs`/`favorite_playlists` are kept because the player UI and playlist detail view read them on every page. `FavoritesActivity::refetch()` re-runs `fetch_favorites()` on reopening, and the activity-change detection in `ui()` drives both. The existing async `clear()` (which also flushes `favorite_songs`) remains the logout path.
* **Uploads Memory Release:** `UploadsActivity::clear()` (`src/activity/uploads.rs`) drops the loaded uploads list, the user quota, the sort option/direction, the URL panel and its input, the pending file list, and the upload feedback when the user navigates away from Uploads. Uploads renders no cover art, so there are no textures to free. `UploadsActivity::refetch()` re-runs `fetch_uploads()` on reopening, and the activity-change detection in `ui()` drives both.
* **Settings Memory Release:** `SettingsActivity::clear()` (`src/activity/settings.rs`) drops the cached storage figures and the clear-cache status when the user navigates away from Settings, and `SettingsActivity::refetch()` re-reads the storage figures on reopening. The activity-change detection in `ui()` drives both. Radio is intentionally left as-is: its polling loop repopulates the live now-playing state within seconds regardless of the active page, so clearing it on navigation away yields no memory benefit.

### Bug fixes
* **Removed Raw Auth Response Logging:** the inbound raw auth response body and the outbound JSON payload are no longer written to `debug.log` (`src/auth/service.rs`). The inbound body contains the JWT session token and the outbound payload contains the Discord `access_token`, so both were plaintext credential leaks in a local log file. CodeQL flagged neither.
* **Playlists Scroll Crash:** the virtualized playlist grid (`src/activity/playlist.rs`) computed the slice start index from the persisted `last_scroll_y` without clamping it to the list length. Rapid scrolling stores a deep offset, and when the list shrinks underneath it (search filter, sort, load-more reset), `start > len` made `sorted_playlists[start..end]` panic with "range start index 42 out of range for slice of length 20". A shared `visible_range()` helper (`src/activity/mod.rs`) clamps `start` to `len` so the slice is empty instead of crashing. The setlist grid (`src/activity/setlist.rs`) had the identical latent bug and now uses the same helper.

### Testing
* Added `test_redact_username_keeps_prefix_and_suffix`, `test_redact_username_masks_short_names` and `test_redact_username_is_unicode_safe` (`src/utilities/util.rs`), bringing the suite to 52 passing tests with 5 ignored
* Added `test_visible_range_top_of_list`, `test_visible_range_mid_list`, `test_visible_range_deep_offset_shrunk_list`, `test_visible_range_empty_list` and `test_visible_range_end_never_exceeds_len` (`src/activity/mod.rs`), covering the virtualized grid slice helper including the rapid-scroll/shrunk-list crash case, bringing the suite to 68 passing tests with 5 ignored

---

## v0.4.2

### Added features
* **Fullscreen Player:** Added a fullscreen player overlay toggled from the player controls (fullscreen icon, fullscreen-exit icon when active, Esc to exit) that shows the full-resolution cover artwork filling the screen with the embedded play/pause, seek, volume, favorite, timer and queue controls
* **My Uploads:** Added a dedicated view for the user's own uploaded songs, fetched from the server with cache-first loading, a cover-art mosaic, sortable columns (title, plays, date), and per-song play/queue controls
* **Lazy Font Loading:** The Noto JP/KR/SC/Cuneiform fallback fonts are embedded gzip-compressed and only decompressed and registered with egui when text containing their scripts appears in song titles, artists, playlists, or playlist details, so startup stays lightweight; Latin text keeps rendering with Roboto and only missing glyphs come from the Noto fallbacks
* **File-Based Song Upload API:** `LazySongDatabase::upload_song_file(FileUpload)` posts `multipart/form-data` to `POST https://idk.neurokaraoke.com/api/user/song/upload` with the `File` part typed by extension (`audio/mpeg` for `.mp3`) plus `Title` and `Artist` text parts typed `text/plain; charset=utf-8`, matching the web app's form exactly; the server transcodes the upload to `user-audio/{guid}.m4a` and answers `200` with an empty body, so success is detected from the status alone. `UserLimits::{remaining_storage_bytes, has_song_slot, can_fit}` cover the quota check. Verified live against the server. UI wiring is intentionally not included yet
* **YouTube URL Validation:** `classify_source_url` (`src/api/sources.rs`) validates source links offline before `LazySongDatabase::upload_song_from_url` posts to `POST https://idk.neurokaraoke.com/api/user/song/download-from-url`, reproducing the server's `400` wording verbatim (`Invalid URL format`, `Unsupported platform. Only YouTube, Bilibili and Discord attachments are supported.`, `Invalid YouTube URL. Must be a video or playlist.`). Recognised forms cover `watch?v=`, `youtu.be`, `shorts`, `live`, `embed`, `v`, `playlist?list`, `m.`/`music.`/`youtube-nocookie` hosts, Bilibili `BV…`/`av…` and `b23.tv` short links, and Discord attachment URLs. The payload field is `playlistId` (the old `playlist_url` model was wrong) and the new `UploadSongResult` parses `{"success":true,"songId":…,"title":…}`. Verified live against the server
* **Uploaded Song Removal:** `LazySongDatabase::delete_uploaded_song(song_id)` sends `DELETE https://idk.neurokaraoke.com/api/user/song/{id}` and treats `204 No Content` as success. The generic `DELETE /api/songs/{id}` on the API host answers `200` with an empty body and leaves the song in place for regular users, so the owner-scoped IDK route is the one that deletes. Verified live (upload then delete restored `currentSongCount` to 15)
* **Uploads UI:** The My Uploads page manages uploads end to end — a "DOWNLOAD FROM YOUTUBE" panel that validates the URL offline with `classify_source_url` and shows the server's wording for each rejection, Songs/Storage quota bars from `UserLimits`, a drop zone that accepts audio files dragged from the OS (filtered to the extensions `audio_content_type` accepts) or opened through the system file picker via `rfd` (run on the tokio blocking pool, so the window keeps animating while the dialog is open), editable Title/Artist rows for every pending file with an Upload button, and a trash button that deletes an uploaded song through the owner-scoped `DELETE /api/user/song/{id}`. Quota gating uses `UserLimits::has_song_slot` for URL uploads and `UserLimits::can_fit(size)` for file uploads; a successful URL download closes the panel and clears the field, and the songs list refreshes after every upload, delete, and URL download.
* **OS Drag and Drop:** `ViewportBuilder::with_drag_and_drop(true)` is set on the main window, dropped files are read from `ctx.input(|i| i.raw.dropped_files)`, and the drop zone border and icon light up while a drag hovers the window. winit 0.30.13 delivers X11 drops (`WindowEvent::DroppedFile` + XDND, confirmed live by the `XdndAware` window property) and Windows drops. **Wayland drag-and-drop needs the unreleased winit 0.31.x**, which adds `data_device` support, so drops are ignored on Wayland for now.

### Improvements
* Added a separate full-resolution artwork cache asset (`AssetType::FullImage`) using the Cloudflare Images `public` variant, downloaded lazily only when fullscreen is opened so the 512x512 thumbnails are never served as fullscreen artwork
* Relocated My Playlists, Favorites, and My Uploads to the bottom of the sidebar navigation list, separated from the main navigation by a divider
* **Font Compression:** `build.rs` now gzip-compresses the lazy Noto fonts at the highest level (zlib level 9, smaller than `gzip -9`) into `assets/fonts/*.ttf.gz`, cached in place by mtime so incremental builds reuse existing archives instead of recompressing
* **Uploads Row Layout:** pending upload rows use fixed field sizes (Title 170×24, Artist 130×24) so the Upload button is never clipped by a long filename

---

## v0.4.1

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
