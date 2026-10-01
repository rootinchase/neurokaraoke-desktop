# Changelog

All notable changes to this project will be documented in this file.

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
