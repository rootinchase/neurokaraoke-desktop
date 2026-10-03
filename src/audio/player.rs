use crate::api::{API_URLS, LazySongDatabase, LoadingState};
use crate::audio::types::*;
use crate::debug_log;
use crate::utilities::cache::PersistentMediaCache;
use eframe::egui;
use rand::prelude::SliceRandom;
use rodio::Source;
use rodio::decoder::DecoderBuilder;
use std::io::BufReader;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use uuid::Uuid;

impl PlaybackState {
    pub fn duration(&self) -> Duration {
        self.duration
    }
    pub fn position(&self) -> Duration {
        if self.loading {
            Duration::from_secs(0)
        } else {
            (self.paused.unwrap_or_else(Instant::now) - self.start).min(self.duration)
        }
    }
    pub fn paused(&self) -> bool {
        self.paused.is_some()
    }
    pub fn song(&self) -> Uuid {
        self.song
    }

    fn new_loading(song: Uuid) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            paused: Some(now),
            duration: Duration::from_secs(0),
            song,
            loading: true,
        }
    }

    fn pause(&mut self) {
        self.paused.get_or_insert_with(Instant::now);
    }

    fn play(&mut self) {
        if self.paused.is_some() {
            if self.position() >= self.duration {
                self.start = Instant::now();
            } else {
                self.start = Instant::now() - self.position();
            }
            self.paused.take();
        }
    }

    fn seek(&mut self, position: Duration) {
        let position = position.min(self.duration);
        self.start = self.paused.unwrap_or_else(Instant::now) - position;
    }
}

impl Clone for Player {
    fn clone(&self) -> Self {
        if let Some(refs) = &self.refs {
            refs.fetch_add(1, Ordering::Relaxed);
        }
        Self {
            refs: self.refs.clone(),
            state: self.state.clone(),
            player_state: self.player_state.clone(),
            sender: self.sender.clone(),
            current_url_metadata: self.current_url_metadata.clone(),
        }
    }
}

impl Player {
    pub fn new(
        rt: Arc<Runtime>,
        ctx: egui::Context,
        database: LazySongDatabase,
        cache: Arc<PersistentMediaCache>,
    ) -> Player {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let p = Player {
            refs: Some(Arc::new(AtomicU32::new(0))),
            state: Arc::new(Mutex::new(None)),
            player_state: Arc::new(Mutex::new(PlayerState {
                volume: 1.0,
                shuffle: false,
                loop_mode: LoopMode::None,
                playlist: None,
                playlist_name: None,
                url_playlist: None,
            })),
            sender: tx,
            current_url_metadata: Arc::new(Mutex::new(None)),
        };

        let mut player = p.clone();
        player.internal();

        thread::spawn(move || {
            let mut handle =
                rodio::DeviceSinkBuilder::open_default_sink().expect("open default audio stream");
            handle.log_on_drop(false);
            let mut mixer = rodio::Player::connect_new(&handle.mixer());

            let init_vol = player.player_state.lock().unwrap().volume;
            mixer.set_volume(init_vol.powi(3));
            mixer.pause();

            let client = reqwest::Client::new();

            let mut ordered_playlist: Option<Arc<[Uuid]>> = None;
            let mut shuffle = false;
            let mut loop_mode = LoopMode::None;

            let cache_for_prefetch = cache.clone();
            let client_for_prefetch = client.clone();
            let database_for_prefetch = database.clone();
            let player_for_prefetch = player.clone();
            let rt_for_prefetch = rt.clone();

            let prefetch_next = move |current_uuid: Uuid| {
                let (playlist, url_playlist) = {
                    let ps = player_for_prefetch.player_state.lock().unwrap();
                    (ps.playlist.clone(), ps.url_playlist.clone())
                };

                let pl = match playlist {
                    Some(pl) => pl,
                    None => return,
                };

                let idx = match pl.iter().position(|&u| u == current_uuid) {
                    Some(i) => i,
                    None => return,
                };

                let next_idx = if idx + 1 < pl.len() {
                    idx + 1
                } else if loop_mode == LoopMode::All || loop_mode == LoopMode::One {
                    0
                } else {
                    return;
                };

                let next_uuid = pl[next_idx];
                let next_url_dto = url_playlist
                    .as_ref()
                    .and_then(|upl| upl.get(next_idx).cloned());

                let cache_worker = cache_for_prefetch.clone();
                let client_worker = client_for_prefetch.clone();
                let db_worker = database_for_prefetch.clone();

                rt_for_prefetch.spawn(async move {
                    debug_log!("🚀 [Prefetch] Preemptively fetching next song audio and cover art (UUID: {})", next_uuid);
                    if let Some(dto) = next_url_dto {
                        // Prefetch audio
                        if let Some(audio_url) = dto.audio_url.as_ref().or(dto.absolute_path.as_ref()) {
                            let url = if audio_url.starts_with("http://") || audio_url.starts_with("https://") {
                                audio_url.to_string()
                            } else {
                                let clean_path = audio_url.trim_start_matches('/');
                                format!("{}/{}", API_URLS.storage, clean_path)
                            };
                            let url = if url.contains(API_URLS.base) {
                                url.replace(API_URLS.base, API_URLS.storage)
                            } else {
                                url
                            }.replace(' ', "%20");

                            let _ = cache_worker.get_or_download_audio(&client_worker, next_uuid, url).await;
                        }

                        // Prefetch cover art
                        if let Some(cover) = &dto.cover_art {
                            let cf = cover.cloudflare_id.as_deref().map(|s| s.as_ref());
                            let img_url = crate::utilities::cache::get_thumbnail_url(cf, &cover.absolute_path, "crop,gravity=auto");
                            if let Ok(art_uuid) = Uuid::parse_str(&cover.id) {
                                if !img_url.is_empty() {
                                    let _ = cache_worker.get_or_download_image(art_uuid, img_url).await;
                                }
                            }
                        } else if let Some(audio_url) = dto.audio_url.as_ref().or(dto.absolute_path.as_ref()) {
                            let img_url = audio_url.replace("/audio/", "/images/").replace(".mp3", ".webp").replace(".m4a", ".webp");
                            if img_url.as_str() != audio_url.as_ref() {
                                let _ = cache_worker.get_or_download_image(next_uuid, img_url).await;
                            }
                        }
                    } else {
                        // DB song fallback
                        let path_opt = db_worker.get(&next_uuid, |s| s.opus.clone().or_else(|| s.absolute_path.clone()));
                        if let LoadingState::Loaded(Some(path_str)) = path_opt {
                            let url = format!("{}/{}", API_URLS.storage, path_str.as_ref()).replace(' ', "%20");
                            let _ = cache_worker.get_or_download_audio(&client_worker, next_uuid, url).await;

                            let img_url = format!("{}/{}", API_URLS.storage, path_str.as_ref().replace("audio/", "images/").replace(".mp3", ".webp").replace(".ogg", ".webp"));
                            let _ = cache_worker.get_or_download_image(next_uuid, img_url).await;
                        }
                    }
                });
            };

            let p = player.clone();
            // 1. UPDATE: Change the closure signature to accept a mutable reference to loop_mode
            let reorder = |playlist: &mut Option<Arc<[Uuid]>>,
                           shuffle: bool,
                           swap: bool,
                           loop_mode_ref: &mut LoopMode| {
                let mut ps = p.player_state.lock().unwrap();
                let pl = ps.playlist.clone();
                let upl = ps.url_playlist.clone();

                // 2. ADD: Sync the thread-local loop mode with the mutex source of truth
                *loop_mode_ref = ps.loop_mode;

                if shuffle {
                    let song = p.state.lock().unwrap().map(|x| x.song());
                    if let Some(pl) = pl {
                        let len = pl.len();
                        let mut indices: Vec<usize> = (0..len).collect();
                        indices.shuffle(&mut rand::rng());

                        let mut new_pl: Vec<Uuid> = indices.iter().map(|&i| pl[i]).collect();
                        let mut new_upl: Vec<crate::api::SongDTO> = upl
                            .filter(|u| u.len() == len)
                            .map(|u| indices.iter().filter_map(|&i| u.get(i).cloned()).collect())
                            .unwrap_or_else(|| vec![]);

                        if let Some(song) = song {
                            if let Some(i) = new_pl.iter().position(|s| *s == song) {
                                if swap {
                                    new_pl.swap(0, i);
                                    if !new_upl.is_empty() {
                                        new_upl.swap(0, i);
                                    }
                                } else if i == 0 && len > 1 {
                                    let swap_idx = rand::random_range(1..len);
                                    new_pl.swap(0, swap_idx);
                                    if !new_upl.is_empty() {
                                        new_upl.swap(0, swap_idx);
                                    }
                                }
                            }
                        }
                        *playlist = Some(new_pl.clone().into());
                        ps.playlist = Some(new_pl.into());
                        ps.url_playlist = if new_upl.is_empty() {
                            None
                        } else {
                            Some(new_upl.into())
                        };
                    }
                } else {
                    *playlist = pl;
                }
            };

            let mut reported_play_song: Option<Uuid> = None;

            loop {
                'block: {
                    let lock = player.state.lock().unwrap();
                    if let Some(state) = lock.as_ref()
                        && !state.paused()
                        && !state.loading
                    {
                        let pos = state.position();
                        let dur = state.duration();
                        let song_uuid = state.song();

                        if reported_play_song != Some(song_uuid) {
                            if pos >= Duration::from_secs(30)
                                || (dur > Duration::from_secs(0) && pos >= dur)
                            {
                                reported_play_song = Some(song_uuid);
                                let db_clone = database.clone();
                                rt.spawn(async move {
                                    if let Err(e) = db_clone.report_play_count(song_uuid).await {
                                        debug_log!(
                                            "❌ Failed to report play count for {}: {}",
                                            song_uuid,
                                            e
                                        );
                                    }
                                });
                            }
                        }

                        if pos >= dur && dur > Duration::from_secs(0) {
                            debug_log!(
                                "Transition: Position {} >= Duration {}, state.song={:?}",
                                pos.as_secs(),
                                dur.as_secs(),
                                state.song
                            );
                            let state = state.clone();
                            drop(lock);

                            let mut playlist_to_use = ordered_playlist.clone();
                            if playlist_to_use.is_none() {
                                let ps = player.player_state.lock().unwrap();
                                playlist_to_use = ps.playlist.clone();
                            }

                            if let Some(playlist) = playlist_to_use {
                                let (len, idx) = {
                                    let mut idx = None;
                                    for i in 0..playlist.len() {
                                        if state.song == playlist[i] {
                                            idx = Some(i);
                                            break;
                                        }
                                    }
                                    (playlist.len(), idx)
                                };

                                debug_log!(
                                    "Transition: Found index {:?} in playlist of length {}, loop_mode={:?}",
                                    idx,
                                    len,
                                    loop_mode
                                );
                                if let Some(idx) = idx {
                                    // FIX 1: Prioritize LoopMode::One BEFORE checking if we reached the end of the playlist.
                                    // This catches URL tracks and DB tracks anywhere in the playlist.
                                    if loop_mode == LoopMode::One {
                                        let player_state = player.player_state.lock().unwrap();
                                        if let Some(url_playlist) = &player_state.url_playlist
                                            && url_playlist.len() == playlist.len()
                                        {
                                            debug_log!(
                                                "Transition: Replaying current URL song via LoopMode::One at index {}",
                                                idx
                                            );
                                            player.url_playback(
                                                Some(playlist[idx]),
                                                url_playlist[idx].clone(),
                                                Player::play,
                                            );
                                        } else {
                                            debug_log!(
                                                "Transition: Replaying current DB song via LoopMode::One"
                                            );
                                            player.song(Some(playlist[idx]), Player::play);
                                        }
                                        break 'block;
                                    }

                                    if idx + 1 >= len {
                                        // Song ended, check loop/shuffle
                                        debug_log!(
                                            "Transition: Song ended, index {} >= len {}",
                                            idx,
                                            len
                                        );
                                        match loop_mode {
                                            LoopMode::All => {
                                                debug_log!(
                                                    "Transition: LoopMode::All, restarting playlist"
                                                );

                                                let player_state =
                                                    player.player_state.lock().unwrap();
                                                if let Some(url_playlist) =
                                                    &player_state.url_playlist
                                                    && url_playlist.len() == playlist.len()
                                                {
                                                    player.url_playback(
                                                        Some(playlist[0]),
                                                        url_playlist[0].clone(),
                                                        Player::play,
                                                    );
                                                } else {
                                                    player.song(Some(playlist[0]), Player::play);
                                                }
                                                break 'block;
                                            }
                                            LoopMode::One => {
                                                unreachable!("Handled above");
                                            }
                                            LoopMode::None => {
                                                // Keep the playlist set, but stop playback
                                                if let Some(state) =
                                                    player.state.lock().unwrap().as_mut()
                                                {
                                                    state.pause();
                                                    // Set position to duration to prevent re-triggering the 'pos >= dur' check
                                                    state.seek(state.duration());
                                                }
                                                break 'block;
                                            }
                                        }
                                    } else {
                                        // Normal transition (Only executes if LoopMode is All or None)
                                        let next_idx = idx + 1;
                                        debug_log!(
                                            "Transition: Normal transition to index {}",
                                            next_idx
                                        );

                                        let player_state = player.player_state.lock().unwrap();
                                        if let Some(url_playlist) = &player_state.url_playlist
                                            && url_playlist.len() == playlist.len()
                                        {
                                            debug_log!(
                                                "2 Transition: Loading next URL song at index {}",
                                                next_idx
                                            );
                                            player.url_playback(
                                                Some(playlist[next_idx]),
                                                url_playlist[next_idx].clone(),
                                                Player::play,
                                            );
                                        } else {
                                            player.song(Some(playlist[next_idx]), Player::play);
                                        }
                                        break 'block;
                                    }
                                }
                            } else {
                                // No playlist set yet, or song not in playlist
                                if let Some(playlist) = ordered_playlist.as_ref()
                                    && loop_mode != LoopMode::None
                                {
                                    debug_log!(
                                        "Transition: Song not in playlist or no playlist, loading first song"
                                    );
                                    let player_state = player.player_state.lock().unwrap();
                                    if let Some(url_playlist) = &player_state.url_playlist
                                        && url_playlist.len() == playlist.len()
                                    {
                                        player.url_playback(
                                            Some(playlist[0]),
                                            url_playlist[0].clone(),
                                            Player::play,
                                        );
                                    } else {
                                        player.song(Some(playlist[0]), Player::play);
                                    }
                                    break 'block;
                                } else {
                                    break 'block;
                                }
                            }
                        }
                    }
                }

                match rx.try_recv() {
                    Ok(command) => match command {
                        PlaybackCommand::Pause => {
                            if let Some(state) = player.state.lock().unwrap().as_mut() {
                                mixer.pause();
                                state.pause();
                                ctx.request_repaint();
                            }
                        }

                        PlaybackCommand::Play => {
                            if let Some(state) = player.state.lock().unwrap().as_mut() {
                                mixer.play();
                                state.play();
                                ctx.request_repaint();
                            }
                        }

                        PlaybackCommand::Volume(v) => {
                            mixer.set_volume(v.powi(3));
                            player.player_state.lock().unwrap().volume = v;
                        }

                        PlaybackCommand::Shuffle(enabled) => {
                            player.player_state.lock().unwrap().shuffle = enabled;
                            if shuffle != enabled {
                                reorder(&mut ordered_playlist, enabled, true, &mut loop_mode);
                            }
                            shuffle = enabled;
                        }

                        PlaybackCommand::Loop(mode) => {
                            player.player_state.lock().unwrap().loop_mode = mode;
                            loop_mode = mode;
                        }

                        PlaybackCommand::Playlist(playlist) => {
                            let lock = player.state.lock().unwrap();
                            if let Some(playlist) = playlist.as_ref()
                                && let Some(state) = lock.as_ref()
                                && !playlist.contains(&state.song())
                            {
                                player
                                    .sender
                                    .try_send(PlaybackCommand::Song(None, Box::new(|_| {})))
                                    .unwrap();
                            }
                            drop(lock);

                            mixer.pause();
                            mixer.clear();
                            // Safely clear metadata when switching playlists
                            *player.current_url_metadata.lock().unwrap() = None;

                            let vol = player.player_state.lock().unwrap().volume;
                            mixer.set_volume(0.0);
                            mixer.set_volume(vol.powi(3));

                            player.player_state.lock().unwrap().playlist = playlist;
                            reorder(&mut ordered_playlist, shuffle, true, &mut loop_mode);
                        }

                        PlaybackCommand::UrlPlaylist(playlist) => {
                            player.player_state.lock().unwrap().url_playlist = playlist;
                        }

                        PlaybackCommand::Playlists(playlist, url_playlist, playlist_name) => {
                            let mut ps = player.player_state.lock().unwrap();
                            ps.playlist = playlist;
                            ps.url_playlist = url_playlist;
                            ps.playlist_name = playlist_name;
                            drop(ps);
                            reorder(&mut ordered_playlist, shuffle, true, &mut loop_mode);
                        }

                        PlaybackCommand::AppendToPlaylist(uuid) => {
                            let mut ps = player.player_state.lock().unwrap();
                            let mut pl = ps
                                .playlist
                                .as_ref()
                                .map(|x| x.to_vec())
                                .unwrap_or_else(Vec::new);
                            pl.push(uuid);
                            ps.playlist = Some(pl.into());
                        }

                        PlaybackCommand::Seek(mut position) => {
                            if let Some(state) = player.state.lock().unwrap().as_mut() {
                                position = position.min(state.duration);
                                if let Err(e) = mixer.try_seek(position) {
                                    eprintln!("{}", e);
                                }

                                state.seek(position);
                                ctx.request_repaint();
                            }
                        }

                        PlaybackCommand::NextSong => {
                            let mut next_song_to_play = None;

                            {
                                let lock = player.state.lock().unwrap();
                                let player_state = player.player_state.lock().unwrap();

                                debug_log!("NextSong: Called");

                                if let (Some(playlist), Some(state)) =
                                    (&player_state.playlist, &*lock)
                                {
                                    let current_index =
                                        playlist.iter().position(|&uuid| uuid == state.song());
                                    debug_log!(
                                        "NextSong: Current index: {:?}, Playlist len: {}",
                                        current_index,
                                        playlist.len()
                                    );

                                    if let Some(idx) = current_index {
                                        let len = playlist.len();
                                        let mut search_indices = (idx + 1..len).collect::<Vec<_>>();
                                        // Unconditional wrap: always check from the beginning if we've reached the end
                                        search_indices.extend(0..idx + 1);

                                        for next_idx in search_indices {
                                            if let Some(url_playlist) = &player_state.url_playlist {
                                                if url_playlist.len() == playlist.len() {
                                                    if let Some(next_song) =
                                                        url_playlist.get(next_idx)
                                                    {
                                                        if next_song.audio_url.is_some() {
                                                            next_song_to_play = Some((
                                                                Some(playlist[next_idx]),
                                                                Some(next_song.clone()),
                                                            ));
                                                            break;
                                                        }
                                                    }
                                                } else {
                                                    next_song_to_play =
                                                        Some((Some(playlist[next_idx]), None));
                                                    break;
                                                }
                                            } else {
                                                next_song_to_play =
                                                    Some((Some(playlist[next_idx]), None));
                                                break;
                                            }
                                        }
                                    }
                                }

                                drop(lock);
                                drop(player_state);
                            }

                            if let Some((opt_uuid, opt_dto)) = next_song_to_play {
                                // Safely clear metadata if the new song isn't the one we are transitioning to
                                let mut meta_lock = player.current_url_metadata.lock().unwrap();
                                if let Some(meta) = &*meta_lock {
                                    if opt_uuid.is_none() || Some(meta.id) != opt_uuid {
                                        *meta_lock = None;
                                    }
                                } else {
                                    *meta_lock = None;
                                }

                                if let Some(dto) = opt_dto {
                                    debug_log!("NextSong: Loading URL song transition");
                                    player.url_playback(opt_uuid, dto, Player::play);
                                } else {
                                    debug_log!("NextSong: Loading non-URL song transition");
                                    player.song(opt_uuid, Player::play);
                                }
                                ctx.request_repaint();
                            } else {
                                debug_log!("NextSong: Reached end of playlist");
                            }
                        }

                        PlaybackCommand::Shutdown => break,

                        PlaybackCommand::Song(uuid, cb) => {
                            let mut lock = player.state.lock().unwrap();
                            if let Some(uuid) = uuid {
                                *player.current_url_metadata.lock().unwrap() = None;
                                if let Some(pl) = ordered_playlist.as_ref()
                                    && !pl.contains(&uuid)
                                {
                                    player
                                        .sender
                                        .try_send(PlaybackCommand::Playlist(None))
                                        .unwrap();
                                }

                                // FIX: Check if it's a target repeat condition (same song id but mixer finished)
                                let is_same_song =
                                    lock.as_ref().map(|s| s.song == uuid).unwrap_or(false);

                                debug_log!(
                                    "PlaybackCommand::Song: uuid={:?}, is_same_song={}, lock.song={:?}, mixer.empty={}",
                                    uuid,
                                    is_same_song,
                                    lock.as_ref().map(|s| s.song),
                                    mixer.empty()
                                );

                                if lock.as_ref().map(|s| s.song != uuid).unwrap_or(true)
                                    || (mixer.empty() && !is_same_song)
                                {
                                    debug_log!(
                                        "PlaybackCommand::Song: Condition met, reloading track."
                                    );
                                    mixer.pause();
                                    mixer.clear();
                                    *lock = Some(PlaybackState::new_loading(uuid));
                                    // Helper to safely clear metadata if the ID doesn't match
                                    let mut meta_lock = player.current_url_metadata.lock().unwrap();
                                    if let Some(meta) = &*meta_lock {
                                        if Some(meta.id) != Some(uuid) {
                                            *meta_lock = None;
                                        }
                                    } else {
                                        *meta_lock = None;
                                    }
                                    drop(lock);
                                    match database.get(&uuid, |s| {
                                        s.opus.clone().or_else(|| s.absolute_path.clone())
                                    }) {
                                        LoadingState::Loaded(path) => {
                                            debug_log!("🟢 [Audio API] Song loaded: {:?}", path);
                                            if let Some(path_str) = path {
                                                let handle = player.clone();
                                                let cache_worker = cache.clone();
                                                let client_worker = client.clone(); // `client` is available in `internal` scope
                                                let cb_worker = cb;
                                                let url = format!(
                                                    "{}/{}",
                                                    API_URLS.storage,
                                                    path_str.as_ref()
                                                ); // Construct URL

                                                rt.spawn(async move {
                                                    match cache_worker.get_or_download_audio(&client_worker, uuid, url).await {
                                                        Ok(tokio_file) => {
                                                            let std_file = tokio_file.into_std().await;
                                                            handle.sender.try_send(PlaybackCommand::SongReady(Some(uuid), std_file, Some(cb_worker))).ok();
                                                            debug_log!("🟢 [Audio API] Song ready after cache resolution");
                                                        }
                                                        Err(e) => {
                                                            debug_log!("🔴 [Audio API] Song resolution error: {:?}", e);
                                                        }
                                                    }
                                                });
                                            } else {
                                                debug_log!("🔴 [Audio API] Song path is empty");
                                            }
                                        }
                                        LoadingState::Loading => {
                                            debug_log!("🟡 [Audio API] Song loading: {:?}", uuid);
                                            continue;
                                        }
                                        LoadingState::Failed(e) => {
                                            debug_log!(
                                                "🔴 [Audio API] Song load failed: {:?}, error: {:?}",
                                                uuid,
                                                e
                                            );
                                            continue;
                                        }
                                    }
                                } else {
                                    mixer.pause();
                                    if let Err(e) = mixer.try_seek(Duration::default()) {
                                        eprintln!("Failed to loop repeat position: {}", e);
                                    }
                                    if let Some(state) = lock.as_mut() {
                                        state.seek(Duration::default());
                                        state.play();
                                    }
                                    mixer.play();

                                    cb(&player);
                                }
                            } else {
                                mixer.clear();
                                *lock = None;
                            }

                            ctx.request_repaint();
                        }

                        PlaybackCommand::UrlPlayback(uuid, song_dto, cb) => {
                            let mut lock = player.state.lock().unwrap();

                            mixer.pause();
                            mixer.clear();
                            mixer.set_volume(0.0);

                            let target_uuid = uuid.unwrap_or_else(Uuid::new_v4);
                            debug_log!(
                                "📥 [Audio API] Initiating pipeline resolution for song: '{}' (UUID: {})",
                                song_dto.title,
                                target_uuid
                            );

                            *lock = Some(PlaybackState::new_loading(target_uuid));
                            drop(lock);

                            *player.current_url_metadata.lock().unwrap() = Some(song_dto.clone());
                            ctx.request_repaint();

                            if let Some(audio_url) = song_dto
                                .audio_url
                                .as_ref()
                                .or(song_dto.absolute_path.as_ref())
                            {
                                let url = if audio_url.starts_with("http://")
                                    || audio_url.starts_with("https://")
                                {
                                    audio_url.to_string()
                                } else {
                                    let clean_path = audio_url.trim_start_matches('/');
                                    format!("{}/{}", API_URLS.storage, clean_path)
                                };

                                // Fix absolute path base mismatches if the API returned neurokaraoke.com directly
                                let url = if url.contains(API_URLS.base) {
                                    url.replace(API_URLS.base, API_URLS.storage)
                                } else {
                                    url
                                };

                                // FIX: Safely replace raw spaces with percent-encoded equivalents (%20)
                                // to prevent reqwest from rejecting paths with spaces
                                let url = url.replace(' ', "%20");

                                let handle = player.clone();
                                let client_worker = client.clone();

                                // Pass down your Arc<PersistentMediaCache> instance down to the spawned async routine
                                let cache_worker = cache.clone();

                                rt.spawn(async move {
                                    // Call centralized cache subsystem for download & path verification
                                    match cache_worker.get_or_download_audio(&client_worker, target_uuid, url).await {
                                        Ok(tokio_file) => {
                                            let std_file = tokio_file.into_std().await;

                                            handle.sender.try_send(PlaybackCommand::SongReady(uuid, std_file, Some(cb))).ok();
                                            debug_log!("UrlPlayback: Track ready and sourced successfully from Cache abstraction");
                                        }
                                        Err(e) => {
                                            debug_log!("UrlPlayback: Cache resolution subsystem error: {}", e);
                                        }
                                    }
                                });
                            } else {
                                debug_log!(
                                    "UrlPlayback: No audio URL AssetType available for song: {}",
                                    song_dto.title
                                );
                            }
                            ctx.request_repaint();
                        }

                        PlaybackCommand::SongReady(uuid, file, cb) => {
                            let len = match file.metadata() {
                                Ok(meta) => meta.len(),
                                Err(_) => continue,
                            };

                            let current_song_id =
                                player.state.lock().unwrap().as_ref().map(|s| s.song());
                            if let (Some(incoming), Some(current)) = (uuid, current_song_id) {
                                if incoming != current {
                                    debug_log!(
                                        "⚠️ [Audio API] Discarding outdated network stream."
                                    );
                                    continue;
                                }
                            }

                            if let Ok(decoder) = DecoderBuilder::new()
                                .with_data(BufReader::new(file))
                                .with_byte_len(len)
                                .build()
                            {
                                let mut lock = player.state.lock().unwrap();

                                mixer.pause();
                                mixer.clear();
                                {
                                    let mut meta_lock = player.current_url_metadata.lock().unwrap();
                                    if let Some(meta) = &*meta_lock {
                                        if uuid.is_none() || Some(meta.id) != uuid {
                                            *meta_lock = None;
                                        }
                                    } else {
                                        *meta_lock = None;
                                    }
                                }

                                let current_vol = player.player_state.lock().unwrap().volume;
                                mixer.set_volume(current_vol.powi(3));
                                debug_log!("🟢 [Audio API] Volume set to: {}", current_vol);

                                let duration =
                                    decoder.total_duration().unwrap_or_else(Duration::default);
                                let target_uuid = uuid.unwrap_or_else(Uuid::new_v4);

                                debug_log!(
                                    "🟢 [Audio API] SUCCESS: Reusing mixer instance. UUID: {}, Duration: {}s",
                                    target_uuid,
                                    duration.as_secs()
                                );

                                // Correctly set start to now so that position() = Instant::now() - start = 0
                                *lock = Some(PlaybackState {
                                    start: Instant::now(),
                                    paused: None,
                                    duration,
                                    song: target_uuid,
                                    loading: false,
                                });

                                // Nuclear Option: Re-instantiate the mixer to guarantee a fresh pipeline
                                drop(mixer);
                                mixer = rodio::Player::connect_new(&handle.mixer());
                                mixer.set_volume(current_vol.powi(3));

                                mixer.append(decoder);
                                debug_log!(
                                    "🟢 [Audio API] Decoder appended to new mixer instance. Song: {:?}",
                                    target_uuid
                                );

                                mixer.play();
                                debug_log!("🟢 [Audio API] Mixer play command issued.");

                                prefetch_next(target_uuid);

                                if let Some(cb) = cb {
                                    cb(&player);
                                }

                                // FIX: Force target UI frame paint sequence calculation loops instantly here
                                ctx.request_repaint();
                            } else {
                                debug_log!(
                                    "❌ [Audio API] Rodio failed to parse the downloaded file format headers."
                                );
                            }
                        }

                        PlaybackCommand::RadioStream(url, cb) => {
                            let mut lock = player.state.lock().unwrap();
                            mixer.pause();
                            mixer.clear();
                            mixer.set_volume(0.0);

                            let target_uuid = Uuid::new_v4();
                            *lock = Some(PlaybackState {
                                start: Instant::now(),
                                paused: None,
                                duration: Duration::from_secs(3600 * 24),
                                song: target_uuid,
                                loading: true,
                            });
                            drop(lock);
                            *player.current_url_metadata.lock().unwrap() = None;
                            ctx.request_repaint();

                            let handle = player.clone();
                            let temp_path = crate::utilities::cache::cache_dir().join("radio_stream.tmp");
                            let mut cb_opt = Some(cb);

                            thread::spawn(move || {
                                match reqwest::blocking::get(&url) {
                                    Ok(mut resp) => {
                                        if let Ok(mut file) = std::fs::File::create(&temp_path) {
                                            let mut buf = [0u8; 8192];
                                            use std::io::{Read, Write};
                                            let mut ready_sent = false;
                                            loop {
                                                match resp.read(&mut buf) {
                                                    Ok(0) => break,
                                                    Ok(n) => {
                                                        if let Ok(()) = file.write_all(&buf[..n]) {
                                                            let _ = file.flush();
                                                            if !ready_sent {
                                                                ready_sent = true;
                                                                if let Some(callback) = cb_opt.take() {
                                                                    if let Ok(read_file) = std::fs::File::open(&temp_path) {
                                                                        handle.sender.try_send(PlaybackCommand::RadioStreamReady(read_file, callback)).ok();
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                    Err(_) => break,
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        debug_log!("Failed to connect to radio stream {}: {}", url, e);
                                    }
                                }
                            });
                        }

                        PlaybackCommand::RadioStreamReady(file, cb) => {
                            let len = match file.metadata() {
                                Ok(meta) => meta.len(),
                                Err(_) => 1024 * 1024 * 100, // fallback
                            };

                            if let Ok(decoder) = DecoderBuilder::new()
                                .with_data(BufReader::new(file))
                                .with_byte_len(len)
                                .build()
                            {
                                let mut lock = player.state.lock().unwrap();
                                mixer.pause();
                                mixer.clear();

                                let current_vol = player.player_state.lock().unwrap().volume;
                                mixer.set_volume(current_vol.powi(3));

                                *lock = Some(PlaybackState {
                                    start: Instant::now(),
                                    paused: None,
                                    duration: Duration::from_secs(3600 * 24),
                                    song: Uuid::nil(),
                                    loading: false,
                                });

                                drop(mixer);
                                mixer = rodio::Player::connect_new(&handle.mixer());
                                mixer.set_volume(current_vol.powi(3));
                                mixer.append(decoder);
                                mixer.play();

                                cb(&player);
                                ctx.request_repaint();
                            } else {
                                debug_log!("Failed to decode radio stream");
                            }
                        }
                    },

                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break,
                }

                thread::sleep(Duration::from_millis(10));
            }
        });

        p
    }

    pub fn get_playback_state(&self) -> Option<PlaybackState> {
        *self.state.lock().unwrap()
    }

    pub fn get_volume(&self) -> f32 {
        self.player_state.lock().unwrap().volume
    }

    pub fn get_shuffle(&self) -> bool {
        self.player_state.lock().unwrap().shuffle
    }

    pub fn get_loop_mode(&self) -> LoopMode {
        self.player_state.lock().unwrap().loop_mode
    }

    pub fn pause(&self) {
        self.sender.try_send(PlaybackCommand::Pause).unwrap();
    }

    pub fn play(&self) {
        self.sender.try_send(PlaybackCommand::Play).unwrap();
    }

    pub fn volume(&self, volume: f32) {
        self.sender
            .try_send(PlaybackCommand::Volume(volume))
            .unwrap();
    }

    pub fn shuffle(&self, shuffle: bool) {
        self.sender
            .try_send(PlaybackCommand::Shuffle(shuffle))
            .unwrap();
        debug_log!("audio.rs: Setting shuffle mode to: {}", shuffle)
    }

    pub fn looping(&self, mode: LoopMode) {
        self.sender.try_send(PlaybackCommand::Loop(mode)).ok();
    }

    pub fn append_to_playlist(&self, uuid: Uuid) {
        self.sender
            .try_send(PlaybackCommand::AppendToPlaylist(uuid))
            .ok();
    }

    pub fn playlists(
        &self,
        playlist: Option<Arc<[Uuid]>>,
        url_playlist: Option<Arc<[crate::api::SongDTO]>>,
        playlist_name: Option<String>,
    ) {
        let mut ps = self.player_state.lock().unwrap();
        ps.playlist = playlist.clone();
        ps.url_playlist = url_playlist.clone();
        ps.playlist_name = playlist_name.clone();
        drop(ps);
        self.sender
            .try_send(PlaybackCommand::Playlists(
                playlist,
                url_playlist,
                playlist_name,
            ))
            .ok();
    }

    pub fn get_playlist_name(&self) -> Option<String> {
        self.player_state.lock().unwrap().playlist_name.clone()
    }

    pub fn remove_from_playlist(&self, uuid: Uuid) {
        let mut player_state = self.player_state.lock().unwrap();
        if let Some(pl) = &player_state.playlist {
            let new_pl: Vec<Uuid> = pl.iter().cloned().filter(|&id| id != uuid).collect();
            player_state.playlist = Some(new_pl.into());
        }
        if let Some(url_pl) = &player_state.url_playlist {
            let new_url_pl: Vec<crate::api::SongDTO> = url_pl
                .iter()
                .cloned()
                .zip(
                    player_state
                        .playlist
                        .as_ref()
                        .map(|p| p.iter())
                        .into_iter()
                        .flatten(),
                )
                .map(|(s, _)| s)
                .collect();
            player_state.url_playlist = Some(new_url_pl.into());
        }
    }

    pub fn playlist(&self, playlist: Option<Arc<[Uuid]>>) {
        self.sender
            .try_send(PlaybackCommand::Playlist(playlist))
            .ok();
    }

    pub fn url_playlist(&self, playlist: Option<Arc<[crate::api::SongDTO]>>) {
        self.sender
            .try_send(PlaybackCommand::UrlPlaylist(playlist))
            .ok();
    }
    pub fn clear_playlist(&self) {
        self.playlist(None);
        self.url_playlist(None);
    }
    pub fn url_playback(
        &self,
        uuid: Option<Uuid>,
        song_dto: crate::api::SongDTO,
        commands_after_load: impl FnOnce(&Player) + Send + 'static,
    ) {
        self.sender
            .try_send(PlaybackCommand::UrlPlayback(
                uuid,
                song_dto,
                Box::new(commands_after_load),
            ))
            .ok();
    }
    pub fn radio_stream(
        &self,
        url: String,
        commands_after_load: impl FnOnce(&Player) + Send + 'static,
    ) {
        self.sender
            .try_send(PlaybackCommand::RadioStream(
                url,
                Box::new(commands_after_load),
            ))
            .ok();
    }
    pub fn previous(&self) {
        let player_state = self.player_state.lock().unwrap();
        let playlist = player_state.playlist.as_ref().cloned();
        let url_playlist = player_state.url_playlist.as_ref().cloned();
        drop(player_state);

        if let Some(playlist) = playlist {
            let lock = self.state.lock().unwrap();
            if let Some(state) = lock.as_ref() {
                let current_index = playlist.iter().position(|&uuid| uuid == state.song());
                if let Some(idx) = current_index {
                    let prev_idx = if idx > 0 { idx - 1 } else { playlist.len() - 1 };
                    if let Some(url_playlist) = &url_playlist
                        && url_playlist.len() == playlist.len()
                    {
                        debug_log!("Previous: Loading previous URL song at index {}", prev_idx);
                        self.url_playback(
                            Some(playlist[prev_idx]),
                            url_playlist[prev_idx].clone(),
                            Player::play,
                        );
                    } else {
                        debug_log!("Previous: Loading previous DB song");
                        self.song(Some(playlist[prev_idx]), Player::play);
                    }
                }
            }
        }
    }

    pub fn next_song(&self) {
        self.sender.try_send(PlaybackCommand::NextSong).ok();
    }

    pub fn play_song(&self, uuid: Uuid) {
        let player_state = self.player_state.lock().unwrap();

        if let Some(playlist) = &player_state.playlist {
            if let Some(index) = playlist.iter().position(|&id| id == uuid) {
                if let Some(url_playlist) = &player_state.url_playlist {
                    if let Some(song_dto) = url_playlist.get(index) {
                        if song_dto.audio_url.is_some() {
                            self.url_playback(Some(uuid), song_dto.clone(), Player::play);
                            return;
                        }
                    }
                }
            }
        }

        self.song(Some(uuid), Player::play);
    }

    pub fn song(
        &self,
        song: Option<Uuid>,
        commands_after_load: impl FnOnce(&Player) + Send + 'static,
    ) {
        self.sender
            .try_send(PlaybackCommand::Song(song, Box::new(commands_after_load)))
            .ok();
    }

    pub fn seek(&self, position: Duration) {
        self.sender.try_send(PlaybackCommand::Seek(position)).ok();
    }

    pub fn get_playlist(&self) -> Option<Arc<[Uuid]>> {
        self.player_state.lock().unwrap().playlist.clone()
    }

    pub fn get_url_playlist(&self) -> Option<Arc<[crate::api::SongDTO]>> {
        self.player_state.lock().unwrap().url_playlist.clone()
    }

    fn internal(&mut self) {
        if let Some(refs) = &self.refs {
            refs.fetch_add(1, Ordering::Relaxed);
        }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        if let Some(refs) = &self.refs
            && refs.fetch_sub(1, Ordering::Relaxed) == 1
        {
            let _ = self.sender.try_send(PlaybackCommand::Shutdown);
        }
    }
}
