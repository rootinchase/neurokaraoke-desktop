use serde::{Deserialize, Serialize};
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct PlaybackState {
    pub(crate) start: Instant,
    pub(crate) paused: Option<Instant>,
    pub(crate) duration: Duration,
    pub(crate) song: Uuid,
    pub(crate) loading: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LoopMode {
    #[default]
    None,
    One,
    All,
}

#[derive(Debug, Clone)]
pub struct PlayerState {
    pub(crate) volume: f32,
    pub(crate) shuffle: bool,
    pub(crate) loop_mode: LoopMode,
    pub(crate) playlist: Option<Arc<[Uuid]>>,
    pub(crate) playlist_name: Option<String>,
    pub(crate) url_playlist: Option<Arc<[crate::api::SongDTO]>>,
}

pub enum PlaybackCommand {
    Pause,
    Play,
    Volume(f32),
    Shuffle(bool),
    Loop(LoopMode),
    Playlist(Option<Arc<[Uuid]>>),
    UrlPlaylist(Option<Arc<[crate::api::SongDTO]>>),
    Playlists(
        Option<Arc<[Uuid]>>,
        Option<Arc<[crate::api::SongDTO]>>,
        Option<String>,
    ),
    Song(Option<Uuid>, Box<dyn FnOnce(&Player) + Send + 'static>),
    UrlPlayback(
        Option<Uuid>,
        crate::api::SongDTO,
        Box<dyn FnOnce(&Player) + Send + 'static>,
    ),
    RadioStream(String, Box<dyn FnOnce(&Player) + Send + 'static>),
    SongReady(
        Option<Uuid>,
        std::fs::File,
        Option<Box<dyn FnOnce(&Player) + Send + 'static>>,
    ),
    RadioStreamReady(std::fs::File, Box<dyn FnOnce(&Player) + Send + 'static>),
    Seek(Duration),
    Shutdown,
    NextSong,
    AppendToPlaylist(Uuid),
}

#[derive(Debug)]
pub struct Player {
    pub(crate) refs: Option<Arc<AtomicU32>>,
    pub(crate) state: Arc<Mutex<Option<PlaybackState>>>,
    pub(crate) player_state: Arc<Mutex<PlayerState>>,
    pub(crate) sender: tokio::sync::mpsc::Sender<PlaybackCommand>,
    pub(crate) current_url_metadata: Arc<Mutex<Option<crate::api::SongDTO>>>,
}
