use crate::api::{LazySongDatabase, LoadingState};
use crate::cache::{AssetType, Cache};
use eframe::egui;
use rand::prelude::SliceRandom;
use rodio::decoder::DecoderBuilder;
use rodio::Source;
use std::io::BufReader;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
/// Represents a state of playback
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

    fn new(duration: Duration, song: Uuid) -> Self {
        let now = Instant::now();
        Self {
            start: now,
            paused: Some(now),
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

#[derive(Debug, Clone)]
struct PlayerState {
    volume: f32,
    shuffle: bool,
    looping: bool,
    playlist: Option<Arc<[Uuid]>>
}

enum PlaybackCommand {
    Pause,
    Play,
    Volume(f32),
    Shuffle(bool),
    Loop(bool),
    Playlist(Option<Arc<[Uuid]>>),
    Song(Option<Uuid>, Box<dyn FnOnce(&Player) + Send + 'static>),
    SongReady(Uuid, std::fs::File),
    Seek(Duration),
    Shutdown,
    NextSong, // NEW: Added NextSong command
}

#[derive(Debug)]
pub struct Player {
    refs: Option<Arc<AtomicU32>>,
    state: Arc<Mutex<Option<PlaybackState>>>,
    player_state: Arc<Mutex<PlayerState>>,
    sender: tokio::sync::mpsc::Sender<PlaybackCommand>,
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
                looping: false,
                playlist: None,
            })),
            sender: tx,
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
            let mut looping = false;

            let p = player.clone();
            let reorder = |playlist: &mut Option<Arc<[Uuid]>>, shuffle: bool, swap: bool| {
                let pl = p.player_state.lock().unwrap().playlist.clone();
                *playlist = if shuffle {
                    let song = p.state.lock().unwrap().map(|x| x.song());
                    pl.and_then(|pl| {
                        let mut vec = pl.to_vec();
                        vec.shuffle(&mut rand::rng());
                        let len = vec.len();
                        if let Some(song) = song {
                            if let Some(i) = vec.iter().position(|s| *s == song) {
                                if swap {
                                    vec.swap(0, i);
                                } else if i == 0 && len > 1 {
                                    vec.swap(0, rand::random_range(1..len));
                                }
                                Some(vec.into())
                            } else {
                                None
                            }
                        } else {
                            Some(vec.into())
                        }
                    })
                } else {
                    pl.clone()
                }
            };

            loop {
                'block: {
                    let lock = player.state.lock().unwrap();
                    if let Some(state) = lock.as_ref() && !state.paused() && state.position() == state.duration {
                        let state = state.clone();
                        drop(lock);

                        if let Some(playlist) = ordered_playlist.as_ref().map(|p| p.clone()) {
                            let (len, idx) = {
                                let mut idx = 0;
                                for i in 0..playlist.len() {
                                    if state.song == playlist[i] {
                                        idx = i + 1;
                                        break;
                                    }
                                }
                                (playlist.len(), idx)
                            };

                            if idx == 0 {
                                player.player_state.lock().unwrap().playlist = None;
                                ordered_playlist = None;
                                break 'block;
                            }

                            if idx >= len {
                                // Song ended, check loop/shuffle
                                if looping {
                                    if shuffle { reorder(&mut ordered_playlist, shuffle, false); }
                                    let playlist = ordered_playlist.as_ref().unwrap();
                                    player.song(Some(playlist[idx % playlist.len()]), Player::play);
                                    break 'block;
                                } else {
                                    player.player_state.lock().unwrap().playlist = None;
                                    ordered_playlist = None;
                                    break 'block;
                                }
                            } else {
                                // Normal transition
                                player.song(Some(playlist[idx]), Player::play);
                                break 'block;
                            }
                        } else {
                            // No playlist set yet
                            if looping {
                                player.song(Some(state.song), Player::play);
                                break 'block;
                            } else {
                                break 'block;
                            }
                        } {
                            *player.state.lock().unwrap() = None;
                            mixer.clear();
                        }
                    }
                }

                if loop {
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
                            PlaybackCommand::Loop(enabled) => {
                                player.player_state.lock().unwrap().looping = enabled;
                                looping = enabled;
                            }
                            PlaybackCommand::Playlist(playlist) => {
                                let lock = player.state.lock().unwrap();
                                if let Some(playlist) = playlist.as_ref() && let Some(state) = lock.as_ref() && !playlist.contains(&state.song) {
                                    player.sender.try_send(PlaybackCommand::Song(None, Box::new(|_| {}))).unwrap();
                                }
                                drop(lock);

                                player.player_state.lock().unwrap().playlist = playlist;
                                reorder(&mut ordered_playlist, shuffle, true);
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
                                if let Some(playlist) = player.player_state.lock().unwrap().playlist.clone() {
                                    if let Some(state) = lock.as_ref() {
                                        let current_index = playlist.iter().position(|&uuid| uuid == state.song());

                                        if let Some(idx) = current_index {
                                            let (len, _) = (playlist.len(), idx + 1);

                                            if let Some(next_idx) = Some(idx + 1).filter(|&i| i < len) {
                                                let next_uuid = playlist[next_idx];

                                                // Reset state and signal new song request
                                                let mut lock = player.state.lock().unwrap();
                                                *lock = None;

                                                // This sends a request to the background thread to load and play the next song
                                                player.song(Some(next_uuid), Player::play);
                                                ctx.request_repaint();
                                            }
                                        }
                                    }
                                }
                            },
                            PlaybackCommand::Shutdown => break true,

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
                                                let cache = cache.clone();
                                                rt.spawn(async move {
                                                    if cache.is_online() {
                                                        if let Ok(file) = cache.get_or_else(&(uuid, AssetType::Audio), || async move { req.send().await.unwrap().bytes().await }).await {
                                                            handle.sender.try_send(PlaybackCommand::SongReady(uuid, file.into_std().await)).unwrap();
                                                            cb(&handle);
                                                        }
                                                    } else {
                                                        if let Some(file) = cache.get(&(uuid, AssetType::Audio)).await {
                                                            handle.sender.try_send(PlaybackCommand::SongReady(uuid, file.into_std().await)).unwrap();
                                                            cb(&handle);
                                                        } else {
                                                            eprintln!("song not cached and offline");
                                                        }
                                                    }


                                                });
                                            }

                                            LoadingState::Loading => {
                                                player.sender.try_send(PlaybackCommand::Song(Some(uuid), cb)).unwrap();
                                                break false;
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

                            PlaybackCommand::SongReady(uuid, file) => {
                                let len = match file.metadata() {
                                    Ok(meta) => meta.len(),
                                    Err(_) => continue,
                                };
                                if let Ok(decoder) = DecoderBuilder::new().with_data(BufReader::new(file)).with_byte_len(len).build() {
                                    let mut lock = player.state.lock().unwrap();

                                    mixer.clear();
                                    *lock = Some(PlaybackState::new(decoder.total_duration().unwrap_or_else(Duration::default), uuid));
                                    mixer.append(decoder);

                                    ctx.request_repaint();
                                }
                            },
                        },
                        Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break false,
                        Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => break true,
                    }
                } { break false }

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
    pub fn shuffle(&self, enabled: bool) {
        self.sender.try_send(PlaybackCommand::Shuffle(enabled)).unwrap();
    }
    pub fn looping(&self, enabled: bool) {
        self.sender.try_send(PlaybackCommand::Loop(enabled)).unwrap();
    }
    pub fn playlist(&self, playlist: Option<Arc<[Uuid]>>) {
        self.sender.try_send(PlaybackCommand::Playlist(playlist)).unwrap();
    }
    pub fn get_playlist(&self) -> Option<Arc<[Uuid]>> {
        self.player_state.lock().unwrap().playlist.clone()
    }
    pub fn seek(&self, position: Duration) {
        self.sender.try_send(PlaybackCommand::Seek(position)).unwrap();
    }
    pub fn song(&self, song: Option<Uuid>, commands_after_load: impl FnOnce(&Player) + Send + 'static) {
        self.sender.try_send(PlaybackCommand::Song(song, Box::new(commands_after_load))).unwrap();
    }
    
    pub fn previous(&self) {
        // Lock player_state to access the playlist reference
        let player_state = self.player_state.lock().unwrap();
        let playlist_option = player_state.playlist.as_ref().map(|p| p.clone());

        // Lock state to get current song UUID
        let player_state_guard = self.state.lock().unwrap();
        let current_uuid_option = player_state_guard.as_ref().map(|s| s.song());
        drop(player_state_guard);

        if let (Some(playlist), Some(current_uuid)) = (playlist_option, current_uuid_option) {
            // Find the index of the currently playing song
            let current_index = playlist.iter().position(|&uuid| uuid == current_uuid);

            if let Some(index) = current_index {
                // Check if there is a song available before the current one
                if index > 0 {
                    let prev_index = index - 1;
                    let prev_uuid = playlist[prev_index];

                    // Signal the player to load and play the previous song UUID.
                    self.song(Some(prev_uuid), Player::play);
                }
            }
        }
        // If no playlist is set or the current song is the first in the list, do nothing.
    }

    pub fn next_song(&self) {
        let playlist = self.player_state.lock().unwrap().playlist.clone();
        let state = *self.state.lock().unwrap();

        if let (Some(playlist), Some(state_ref)) = (playlist, state) {
            let current_index = playlist.iter().position(|&uuid| uuid == state_ref.song()).map(|i| i);

            if let Some(idx) = current_index {
                let (len, _) = (playlist.len(), idx + 1);

                if let Some(next_idx) = Some(idx + 1).filter(|&i| i < len) {
                    let next_uuid = playlist[next_idx];

                    // Reset state and signal new song request
                    {
                        let mut lock = self.state.lock().unwrap();
                        *lock = None;
                    }

                    // This sends a request to the background thread to load and play the next song,
                    // forcing the state machine to re-evaluate the playlist using the internal logic.
                    self.song(Some(next_uuid), Player::play);
                }
            }
        }
    }

    fn internal(&mut self) {
        self.refs = None;
    }
}

impl Clone for Player {
    fn clone(&self) -> Self {
        if let Some(refs) = &self.refs { refs.fetch_add(1, std::sync::atomic::Ordering::Relaxed); }
        Self {
            refs: self.refs.clone(),
            state: self.state.clone(),
            player_state: self.player_state.clone(),
            sender: self.sender.clone(),
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        if let Some(refs) = &self.refs && refs.fetch_sub(1, std::sync::atomic::Ordering::Relaxed) == 1 {
            self.sender.try_send(PlaybackCommand::Shutdown).unwrap();
        }
    }
}