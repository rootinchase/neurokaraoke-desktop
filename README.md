
# Native desktop app for https://neurokaraoke.com/

## This app is currently not finished, you may want to check out the [neuro-karaoke-wrapper](https://github.com/AferilVT/neuro-karaoke-wrapper) project if you want a full support!

## OS Support

| OS            | Support level  |
|---------------|----------------|
| Linux/Wayland | Full Support   |
| Linux/X11     | Supported      |
| Windows       | Supported      |
| macOS         | Unable to test |


## Features:
- Public and Personal Playlists playback
- Official setlists viewing and playback
- Song favorites support (add/remove)
- Advanced song search and sorting with prefix filters (`cover:`, `original:`)
- User profile display (badges, levels, and limits)
- Customizable sleep timer
- Discord RPC integration and username/password authentication
- System tray support (Play/Pause, theme switching)
- Session restoration, suggested songs, and trending songs
- Card-based playlist and setlists view
- Song playback (opus and standard supported, will try to use opus)
- Song play count reporting (with 30s continuous playback threshold)
- Shuffle, loop, and volume controls
- Queue viewing
- Themes and multi-language font support (Korean, Chinese, Roboto)
- Song and artwork caching (with tier-2 playlist caching and preemptive fetching)
- Fullscreen player (full-resolution cover art with play/pause, seek, volume, favorite, sleep timer, and queue controls; Esc to exit)
- Fullscreen playlist view (cover art, stats, searchable tracks table, play/shuffle/favorite, and copy playlist link)
- Radio support (Shoutcast live radio with persistent volume and smooth switching between radio and tracks)
- HLS radio playback (MPEG-TS segments demuxed into ADTS AAC and decoded to PCM)
- HLS adaptive bitrate (bandwidth estimation, automatic tier switching, and a manual tier lock: Auto/Lofi/Midfi/Hifi)
- My Uploads view (your own uploaded songs with cover-art mosaic, sortable columns, and per-song play/queue controls)
- Song uploads from local audio files (drag-and-drop, system file picker, editable title/artist)
- Song uploads from YouTube, Bilibili, and Discord URLs, validated offline before upload
- Uploaded song removal
- OS drag-and-drop of audio files (X11 and Windows; Wayland needs the unreleased winit 0.31.x)
- Songs and storage quota bars
- Dedicated settings screen
- Cache management (time- and storage-based eviction, TTL updates, and stale download cleanup)
- Default cover art fallback for songs without cover art metadata
- Lazy font loading (Noto fallback fonts are decompressed only when their scripts appear)
- Synced lyrics overlay in the fullscreen player (active/next/past lines, top-left toggle button, and unsynced lyrics)
- Debug log file output
- ...and more coming soon!

## Upcoming Features:
#### 0.5.x
- Editing playlists
- Listen together

#### 1.x.x
- Hide window
  - Windows
  - mac OS
  - Linux
- Full API Test Suite
- Translations


## Contributing:
Please leave a PR/issue
