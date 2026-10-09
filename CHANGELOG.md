# Changelog

All notable changes to this project will be documented in this file.

## v0.4.2

### Added features
* **Fullscreen Player:** Added a fullscreen player overlay toggled from the player controls (fullscreen icon, fullscreen-exit icon when active, Esc to exit) that shows the full-resolution cover artwork filling the screen with the embedded play/pause, seek, volume, favorite, timer and queue controls
* **My Uploads:** Added a dedicated view for the user's own uploaded songs, fetched from the server with cache-first loading, a cover-art mosaic, sortable columns (title, plays, date), and per-song play/queue controls
* **Lazy Font Loading:** The Noto JP/KR/SC/Cuneiform fallback fonts are embedded gzip-compressed and only decompressed and registered with egui when text containing their scripts appears in song titles, artists, playlists, or playlist details, so startup stays lightweight; Latin text keeps rendering with Roboto and only missing glyphs come from the Noto fallbacks
* **File-Based Song Upload API:** `LazySongDatabase::upload_song_file(FileUpload)` posts `multipart/form-data` to `POST https://idk.neurokaraoke.com/api/user/song/upload` with the `File` part typed by extension (`audio/mpeg` for `.mp3`) plus `Title` and `Artist` text parts typed `text/plain; charset=utf-8`, matching the web app's form exactly; the server transcodes the upload to `user-audio/{guid}.m4a` and answers `200` with an empty body, so success is detected from the status alone. `UserLimits::{remaining_storage_bytes, has_song_slot, can_fit}` cover the quota check. Verified live against the server. UI wiring is intentionally not included yet
* **YouTube URL Validation:** `classify_source_url` (`src/api/sources.rs`) validates source links offline before `LazySongDatabase::upload_song_from_url` posts to `POST https://idk.neurokaraoke.com/api/user/song/download-from-url`, reproducing the server's `400` wording verbatim (`Invalid URL format`, `Unsupported platform. Only YouTube, Bilibili and Discord attachments are supported.`, `Invalid YouTube URL. Must be a video or playlist.`). Recognised forms cover `watch?v=`, `youtu.be`, `shorts`, `live`, `embed`, `v`, `playlist?list`, `m.`/`music.`/`youtube-nocookie` hosts, Bilibili `BV…`/`av…` and `b23.tv` short links, and Discord attachment URLs. The payload field is `playlistId` (the old `playlist_url` model was wrong) and the new `UploadSongResult` parses `{"success":true,"songId":…,"title":…}`. Verified live against the server
* **Uploaded Song Removal:** `LazySongDatabase::delete_uploaded_song(song_id)` sends `DELETE https://idk.neurokaraoke.com/api/user/song/{id}` and treats `204 No Content` as success. The generic `DELETE /api/songs/{id}` on the API host answers `200` with an empty body and leaves the song in place for regular users, so the owner-scoped IDK route is the one that deletes. Verified live (upload then delete restored `currentSongCount` to 15)

### Improvements
* Added a separate full-resolution artwork cache asset (`AssetType::FullImage`) using the Cloudflare Images `public` variant, downloaded lazily only when fullscreen is opened so the 512x512 thumbnails are never served as fullscreen artwork
* Relocated My Playlists, Favorites, and My Uploads to the bottom of the sidebar navigation list, separated from the main navigation by a divider
* **Font Compression:** `build.rs` now gzip-compresses the lazy Noto fonts at the highest level (zlib level 9, smaller than `gzip -9`) into `assets/fonts/*.ttf.gz`, cached in place by mtime so incremental builds reuse existing archives instead of recompressing

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
