use crate::audio::Player;
use crate::config::SharedConfig;
use crate::utilities::discord::DiscordPresencePayload;
use crate::utilities::playwire::{self, MediaControls, Repeat, Track};
use tokio::sync::mpsc::UnboundedSender;

pub struct PlaybackIntegrationManager {
    pub media_controls: Option<MediaControls>,
    pub current_track: Option<Track>,
    discord_tx: UnboundedSender<DiscordPresencePayload>,
}

impl PlaybackIntegrationManager {
    pub fn new(
        player: Player,
        shared_config: SharedConfig,
        discord_tx: UnboundedSender<DiscordPresencePayload>,
    ) -> Self {
        let media_controls = playwire::init_playwire(player, shared_config);
        Self {
            media_controls,
            current_track: None,
            discord_tx,
        }
    }

    pub fn get_track(&self) -> Option<&Track> {
        self.current_track.as_ref()
    }

    pub fn update_metadata(&mut self, track: Track) {
        self.current_track = Some(track);
    }

    pub fn update_playback(
        &mut self,
        state: &crate::audio::PlaybackState,
        repeat: Repeat,
        shuffle: bool,
        volume: f64,
        discord_payload: Option<DiscordPresencePayload>,
    ) {
        let track = self.get_track().cloned();
        if let Some(controls) = &mut self.media_controls {
            let _ = controls.set_state(&playwire::PlaybackState {
                track,
                playing: !state.paused(),
                position: state.position(),
                duration: Some(state.duration()),
                volume,
                repeat,
                shuffle,
                capabilities: playwire::Capabilities::default(),
            });
        }

        if let Some(payload) = discord_payload {
            let _ = self.discord_tx.send(payload);
        }
    }
}
