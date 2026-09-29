use crate::auth::discord::NEURO_KARAOKE_DISCORD;
use crate::debug_log;
use presenceforge::ActivityBuilder;
use presenceforge::async_io::tokio::TokioDiscordIpcClient;
use std::time::Duration;

pub struct DiscordPresencePayload {
    pub title: String,
    pub artist_line: String,
    pub is_playing: bool,
    pub position_secs: u64,
    pub duration_secs: Option<u64>,
    pub cover_url: Option<String>,
}

pub async fn spawn_discord_worker(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<DiscordPresencePayload>,
) {
    let mut client =
        match TokioDiscordIpcClient::new(NEURO_KARAOKE_DISCORD.client_id.to_string()).await {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "spawn_discord_worker: Failed to create Discord structure: {:?}",
                    e
                );
                return;
            }
        };

    let mut connected = false;

    while let Some(payload) = rx.recv().await {
        // Dynamic lazy reconnection verification
        if !connected {
            if client.connect().await.is_ok() {
                connected = true;
                debug_log!("spawn_discord_worker: Connected to local Discord pipe!");
            } else {
                continue;
            }
        }

        // Updated for presenceforge 0.3.0 builder specifications
        let mut activity = ActivityBuilder::new()
            .details(payload.title)
            .large_text("Neuro Karaoke Player");

        if let Some(ref image_url) = payload.cover_url
            && !image_url.is_empty()
        {
            activity = activity.large_image(image_url.trim());
        } else {
            activity = activity.large_image("app_icon"); // Default pre-uploaded asset panel graphic
        }

        if !payload.is_playing {
            activity = activity.state("Paused ⏸");
        } else {
            activity = activity.state(payload.artist_line);

            if let Some(dur) = payload.duration_secs {
                let now = std::time::SystemTime::now();
                // Subtraction protects correctness if egui loop lags slightly behind decoding frames
                let start_time = now
                    .checked_sub(Duration::from_secs(payload.position_secs))
                    .unwrap_or(now);
                let end_time = start_time + Duration::from_secs(dur);

                // Convert SystemTime into raw epoch seconds integers required by the builder API
                if let (Ok(start_unix), Ok(end_unix)) = (
                    start_time.duration_since(std::time::SystemTime::UNIX_EPOCH),
                    end_time.duration_since(std::time::SystemTime::UNIX_EPOCH),
                ) {
                    activity = activity
                        .start_timestamp(start_unix.as_secs())
                        .end_timestamp(end_unix.as_secs() as i64);
                }
            }
        }

        if let Err(e) = client.set_activity(&activity.build()).await {
            eprintln!(
                "spawn_discord_worker: Communication error - dropping client connection state: {:?}",
                e
            );
            // FIX: Clear the activity layout state. The socket cleans up natively when disconnected or reassigned.
            let _ = client.clear_activity().await;
            connected = false; // Flag to attempt reconnection on next track update loop pass
        }
    }
}
