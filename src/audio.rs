use serde::{Deserialize, Serialize};
use crate::api::{LazySongDatabase, LoadingState};
use crate::cache::{AssetType, Cache};
use eframe::egui;
use rand::prelude::SliceRandom;
use rodio::decoder::DecoderBuilder;
use rodio::Source;
use std::io::BufReader;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct PlaybackState {
    start: Instant,
    paused: Option<Instant>,
    duration: Duration,
    song: Uuid,
}

impl PlaybackState {
    pub fn duration(&self) -> Duration { self.duration }
    pub fn position(&self) -> Duration { (self.paused.unwrap_or_else(Instant::now) - self.start).min(self.duration) }
    pub fn paused(&self) -> bool { self.paused.is_some() }
    pub fn song(&self) -> Uuid { self.song }

    fn new(duration: Duration, song: Uuid, is_playing: bool) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            paused: if is_playing { None } else { Some(now) },
            duration,
            song,
        }
    }

    fn pause(&mut self) {
        self.paused.get_or_insert_with(Instant::now);
    }

    fn play(&mut self) {
        if self.paused.is_some() {
            self.start = Instant::now() - self.position();
            self.paused.take();
        }
    }

    fn seek(&mut self, position: Duration) {
        let position = position.min(self.duration);
        self.start = self.paused.unwrap_or_else(Instant::now) - position;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LoopMode { #[default] None, One, All }

#[derive(Debug, Clone)]
struct PlayerState {
    volume: f32,
    shuffle: bool,
    loop_mode: LoopMode,
    playlist: Option<Arc<[Uuid]>>,
    url_playlist: Option<Arc<[crate::api::SongDTO]>>,
}

enum PlaybackCommand {
    Pause,
    Play,
    Volume(f32),
    Shuffle(bool),
    Loop(LoopMode),
    Playlist(Option<Arc<[Uuid]>>),
    UrlPlaylist(Option<Arc<[crate::api::SongDTO]>>),
    Song(Option<Uuid>, Box<dyn FnOnce(&Player) + Send + 'static>),
    UrlPlayback(Option<Uuid>, crate::api::SongDTO, Box<dyn FnOnce(&Player) + Send + 'static>),
    SongReady(Option<Uuid>, std::fs::File, Option<Box<dyn FnOnce(&Player) + Send + 'static>>),
    Seek(Duration),
    Shutdown,
    NextSong,
}

#[derive(Debug)]
pub struct Player {
    refs: Option<Arc<AtomicU32>>,
    state: Arc<Mutex<Option<PlaybackState>>>,
    player_state: Arc<Mutex<PlayerState>>,
    sender: tokio::sync::mpsc::Sender<PlaybackCommand>,
    pub current_url_metadata: Arc<Mutex<Option<crate::api::SongDTO>>>,
}

impl Clone for Player {
    fn clone(&self) -> Self {
        if let Some(refs) = &self.refs { refs.fetch_add(1, Ordering::Relaxed); }
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
    pub fn new(rt: Arc<Runtime>, ctx: egui::Context, database: LazySongDatabase, cache: Arc<Cache>) -> Player {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);

        let p = Player {
            refs: Some(Arc::new(AtomicU32::new(0))),
            state: Arc::new(Mutex::new(None)),
            player_state: Arc::new(Mutex::new(PlayerState {
                volume: 1.0,
                shuffle: false,
                loop_mode: LoopMode::None,
                playlist: None,
                url_playlist: None,
            })),
            sender: tx,
            current_url_metadata: Arc::new(Mutex::new(None)),
        };

        let mut player = p.clone();
        player.internal();
        thread::spawn(move || {
            let mut handle = rodio::DeviceSinkBuilder::open_default_sink()
                .expect("open default audio stream");
            handle.log_on_drop(false);
            let mixer = rodio::Player::connect_new(&handle.mixer());

            mixer.set_volume(player.player_state.lock().unwrap().volume);
            mixer.pause();
            let client = reqwest::Client::new();

            let mut ordered_playlist: Option<Arc<[Uuid]>> = None;
            let mut shuffle = false;
            let mut loop_mode = LoopMode::None;

            let p = player.clone();
            let reorder = |playlist: &mut Option<Arc<[Uuid]>>, shuffle: bool, swap: bool| {
                let mut ps = p.player_state.lock().unwrap();
                let pl = ps.playlist.clone();
                let upl = ps.url_playlist.clone();
                
                if shuffle {
                    let song = p.state.lock().unwrap().map(|x| x.song());
                    if let Some(pl) = pl {
                        let len = pl.len();
                        let mut indices: Vec<usize> = (0..len).collect();
                        indices.shuffle(&mut rand::rng());
                        
                        let mut new_pl: Vec<Uuid> = indices.iter().map(|&i| pl[i]).collect();
                        let mut new_upl: Vec<crate::api::SongDTO> = upl.map(|upl| indices.iter().map(|&i| upl[i].clone()).collect()).unwrap_or_else(|| vec![]);
                        
                        if let Some(song) = song {
                            if let Some(i) = new_pl.iter().position(|s| *s == song) {
                                if swap {
                                    new_pl.swap(0, i);
                                    if !new_upl.is_empty() { new_upl.swap(0, i); }
                                } else if i == 0 && len > 1 {
                                    let swap_idx = rand::random_range(1..len);
                                    new_pl.swap(0, swap_idx);
                                    if !new_upl.is_empty() { new_upl.swap(0, swap_idx); }
                                }
                            }
                        }
                        *playlist = Some(new_pl.clone().into());
                        ps.playlist = Some(new_pl.into());
                        ps.url_playlist = if new_upl.is_empty() { None } else { Some(new_upl.into()) };
                    }
                } else {
                    *playlist = pl;
                }
            };

            loop {
                'block: {
                    let lock = player.state.lock().unwrap();
                    if let Some(state) = lock.as_ref() && !state.paused() {
                        let pos = state.position();
                        let dur = state.duration();
                        if pos >= dur && dur > Duration::from_secs(0) {
                            crate::debug_log!("Transition: Position {} >= Duration {}", pos.as_secs(), dur.as_secs());
                            let state = state.clone();
                            drop(lock);

                            if let Some(playlist) = ordered_playlist.as_ref().map(|p| p.clone()) {
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

                                if let Some(idx) = idx {
                                    if idx + 1 >= len {
                                        // Song ended, check loop/shuffle
                                        crate::debug_log!("Transition: Song ended, index {} >= len {}", idx, len);
                                        match loop_mode {
                                            LoopMode::All => {
                                                if shuffle { reorder(&mut ordered_playlist, shuffle, false); }
                                                let playlist = ordered_playlist.as_ref().unwrap();
                                                let next_idx = 0; // Loop back
                                                
                                                let player_state = player.player_state.lock().unwrap();
                                                if let Some(url_playlist) = &player_state.url_playlist && url_playlist.len() == playlist.len() {
                                                    crate::debug_log!("Transition: Loading next URL song at index {}", next_idx);
                                                    player.url_playback(Some(playlist[next_idx]), url_playlist[next_idx].clone(), Player::play);
                                                } else {
                                                    player.song(Some(playlist[next_idx]), Player::play);
                                                }
                                                break 'block;
                                            }
                                            LoopMode::One => {
                                                // Replay current song
                                                player.song(Some(playlist[idx]), Player::play);
                                                break 'block;
                                            }
                                            LoopMode::None => {
                                                player.player_state.lock().unwrap().playlist = None;
                                                ordered_playlist = None;
                                                break 'block;
                                            }
                                        }
                                    } else {
                                        // Normal transition
                                        let next_idx = idx + 1;
                                        crate::debug_log!("Transition: Normal transition to index {}", next_idx);
                                        let player_state = player.player_state.lock().unwrap();
                                        if let Some(url_playlist) = &player_state.url_playlist && url_playlist.len() == playlist.len() {
                                            crate::debug_log!("Transition: Loading next URL song at index {}", next_idx);
                                            player.url_playback(Some(playlist[next_idx]), url_playlist[next_idx].clone(), Player::play);
                                        } else {
                                            player.song(Some(playlist[next_idx]), Player::play);
                                        }
                                        break 'block;
                                    }
                                } else {
                                    // Current song not in playlist, stop
                                    player.player_state.lock().unwrap().playlist = None;
                                    ordered_playlist = None;
                                    break 'block;
                                }
                            } else {
                                // No playlist set yet
                                if loop_mode != LoopMode::None {
                                    player.song(Some(state.song), Player::play);
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
                        },
                        PlaybackCommand::Play => {
                            if let Some(state) = player.state.lock().unwrap().as_mut() {
                                mixer.play();
                                state.play();
                                ctx.request_repaint();
                            }
                        },
                        PlaybackCommand::Volume(mut v) => {
                            v = v.powi(3);
                            mixer.set_volume(v);
                            player.player_state.lock().unwrap().volume = v;
                        }
                        PlaybackCommand::Shuffle(enabled) => {
                            player.player_state.lock().unwrap().shuffle = enabled;
                            if shuffle != enabled { reorder(&mut ordered_playlist, enabled, true); }
                            shuffle = enabled;
                        }
                        PlaybackCommand::Loop(mode) => {
                            player.player_state.lock().unwrap().loop_mode = mode;
                            loop_mode = mode;
                        }
                        PlaybackCommand::Playlist(playlist) => {
                            let lock = player.state.lock().unwrap();
                            if let Some(playlist) = playlist.as_ref() && let Some(state) = lock.as_ref() && !playlist.contains(&state.song()) {
                                player.sender.try_send(PlaybackCommand::Song(None, Box::new(|_| {}))).unwrap();
                            }
                            drop(lock);

                            player.player_state.lock().unwrap().playlist = playlist;
                            reorder(&mut ordered_playlist, shuffle, true);
                        }
                        PlaybackCommand::UrlPlaylist(playlist) => {
                            player.player_state.lock().unwrap().url_playlist = playlist;
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
                        },
                        PlaybackCommand::NextSong => {
                            let lock = player.state.lock().unwrap();
                            let player_state = player.player_state.lock().unwrap();
                            
                            if let Some(playlist) = player_state.playlist.clone() && let Some(state) = lock.as_ref() {
                                let current_index = playlist.iter().position(|&uuid| uuid == state.song());
                                crate::debug_log!("NextSong: Current index: {:?}, Playlist len: {}", current_index, playlist.len());

                                if let Some(idx) = current_index {
                                    let len = playlist.len();
                                    if let Some(next_idx) = Some(idx + 1).filter(|&i| i < len) {
                                        
                                        // Check if there's a corresponding song in the url_playlist
                                        if let Some(url_playlist) = &player_state.url_playlist && url_playlist.len() == playlist.len() {
                                            crate::debug_log!("NextSong: Loading URL song at index {}", next_idx);
                                            player.url_playback(Some(playlist[next_idx]), url_playlist[next_idx].clone(), Player::play);
                                        } else {
                                            player.song(Some(playlist[next_idx]), Player::play);
                                        }
                                        ctx.request_repaint();
                                    }
                                }
                            }
                            drop(lock);
                            drop(player_state);
                        },
                        PlaybackCommand::Shutdown => break,

                        PlaybackCommand::Song(uuid, cb) => {
                            let mut lock = player.state.lock().unwrap();
                            if let Some(uuid) = uuid {
                                if let Some(pl) = ordered_playlist.as_ref() && !pl.contains(&uuid) {
                                    player.sender.try_send(PlaybackCommand::Playlist(None)).unwrap();
                                }
                                if lock.as_ref().map(|s| s.song != uuid).unwrap_or(true) || mixer.empty() {
                                    mixer.clear();
                                    *lock = None;
                                    drop(lock);
                                    match database.get(&uuid, |s| s.opus.clone().or_else(|| s.absolute_path.clone())) {
                                        LoadingState::Loaded(path) => {
                                            let audio = path.map(|path| format!("https://storage.neurokaraoke.com/{}", path));
                                            if audio.is_none() { continue; }

                                            let req = client.get(audio.unwrap().clone());
                                            let handle = player.clone();
                                            let cache = cache.clone(); // Correct clone
                                            rt.spawn(async move {
                                                if cache.is_online() {

                                                    if let Ok(file) = cache.get_or_else(&(uuid, AssetType::Audio), || async move { req.send().await.unwrap().bytes().await }).await {
                                                        handle.sender.try_send(PlaybackCommand::SongReady(Some(uuid), file.into_std().await, Some(cb))).unwrap();
                                                    }
                                                } else {
                                                    if let Some(file) = cache.get(&(uuid, AssetType::Audio)).await {
                                                        handle.sender.try_send(PlaybackCommand::SongReady(Some(uuid), file.into_std().await, Some(cb))).unwrap();
                                                    } else {
                                                        crate::debug_log!("song not cached and offline");
                                                    }
                                                }
                                            });
                                        }

                                        LoadingState::Loading => {
                                            player.sender.try_send(PlaybackCommand::Song(Some(uuid), cb)).unwrap();
                                        }

                                        LoadingState::Failed(_) => {
                                            continue;
                                        }
                                    }
                                } else {
                                    player.seek(Duration::default());
                                }
                            } else {
                                mixer.clear();
                                *lock = None;
                            }

                            ctx.request_repaint();
                        },

                        PlaybackCommand::UrlPlayback(uuid, song_dto, cb) => {
                            let mut lock = player.state.lock().unwrap();
                            mixer.clear();
                            // Initialize with the provided UUID (or new one if None)
                            *lock = Some(PlaybackState::new(Duration::from_secs(0), uuid.unwrap_or_else(Uuid::new_v4), true));
                            drop(lock);

                            *player.current_url_metadata.lock().unwrap() = Some(song_dto.clone());

                            let req = client.get(song_dto.audio_url.to_string());
                            let handle = player.clone();
                            let cache = cache.clone();
                            rt.spawn(async move {
                                let temp_path = std::env::temp_dir().join(format!("audio_{}", Uuid::new_v4()));

                                if let Ok(response) = req.send().await {
                                    if let Ok(bytes) = response.bytes().await {
                                        tokio::fs::write(&temp_path, &bytes).await.unwrap();
                                        let file = std::fs::File::open(&temp_path).unwrap();
                                        handle.sender.try_send(PlaybackCommand::SongReady(uuid, file, Some(cb))).unwrap();
                                    }
                                }
                            });
                            ctx.request_repaint();
                        },
                        PlaybackCommand::SongReady(uuid, file, cb) => {
                            let len = match file.metadata() {
                                Ok(meta) => meta.len(),
                                Err(_) => continue,
                            };
                            if let Ok(decoder) = DecoderBuilder::new().with_data(BufReader::new(file)).with_byte_len(len).build() {

                                let mut lock = player.state.lock().unwrap();

                                mixer.clear();
                                *lock = Some(PlaybackState::new(decoder.total_duration().unwrap_or_else(Duration::default), uuid.unwrap_or_else(Uuid::new_v4), true));
                                mixer.append(decoder);

                                if let Some(cb) = cb {
                                    cb(&player);
                                }

                                ctx.request_repaint();
                            }
                        },
                    },
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {},
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

    pub fn pause(&self) {
        self.sender.try_send(PlaybackCommand::Pause).unwrap();
    }
    pub fn play(&self) {
        self.sender.try_send(PlaybackCommand::Play).unwrap();
    }
    pub fn volume(&self, volume: f32) {
        self.sender.try_send(PlaybackCommand::Volume(volume)).unwrap();
    }
    pub fn shuffle(&self, shuffle: bool) {
        self.sender.try_send(PlaybackCommand::Shuffle(shuffle)).unwrap();
    }
    pub fn looping(&self, mode: LoopMode) {
        self.sender.try_send(PlaybackCommand::Loop(mode)).unwrap();
    }
    pub fn playlist(&self, playlist: Option<Arc<[Uuid]>>) {
        self.sender.try_send(PlaybackCommand::Playlist(playlist)).unwrap();
    }
    pub fn url_playlist(&self, playlist: Option<Arc<[crate::api::SongDTO]>>) {
        self.sender.try_send(PlaybackCommand::UrlPlaylist(playlist)).unwrap();
    }
    pub fn url_playback(&self, uuid: Option<Uuid>, song_dto: crate::api::SongDTO, commands_after_load: impl FnOnce(&Player) + Send + 'static) {
        self.sender.try_send(PlaybackCommand::UrlPlayback(uuid, song_dto, Box::new(commands_after_load))).unwrap();
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
                if let Some(idx) = current_index && idx > 0 {
                    let prev_idx = idx - 1;
                    
                    if let Some(url_playlist) = &url_playlist && url_playlist.len() == playlist.len() {
                        crate::debug_log!("Previous: Loading previous URL song at index {}", prev_idx);
                        self.url_playback(Some(playlist[prev_idx]), url_playlist[prev_idx].clone(), Player::play);
                    } else {
                        crate::debug_log!("Previous: Loading previous DB song");
                        self.song(Some(playlist[prev_idx]), Player::play);
                    }
                }
            }
        }
    }
    pub fn next_song(&self) {
        self.sender.try_send(PlaybackCommand::NextSong).unwrap();
    }
    pub fn song(&self, song: Option<Uuid>, commands_after_load: impl FnOnce(&Player) + Send + 'static) {
        self.sender.try_send(PlaybackCommand::Song(song, Box::new(commands_after_load))).unwrap();
    }
    pub fn seek(&self, position: Duration) {
        self.sender.try_send(PlaybackCommand::Seek(position)).unwrap();
    }
    pub fn get_playlist(&self) -> Option<Arc<[Uuid]>> {
        self.player_state.lock().unwrap().playlist.clone()
    }
    fn internal(&mut self) {
        if let Some(refs) = &self.refs { refs.fetch_add(1, Ordering::Relaxed); }
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        if let Some(refs) = &self.refs && refs.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.sender.try_send(PlaybackCommand::Shutdown).unwrap();
        }
    }
}
