#[cfg(test)]
mod tests {
    use crate::api::models::*;

    #[test]
    fn test_parse_azuracast_neuro_21() {
        let json_data = std::fs::read_to_string("raw_json/neuro_21")
            .expect("Failed to read raw_json/neuro_21");
        let parsed: AzuraCastNowPlayingResponse = serde_json::from_str(&json_data)
            .expect("Failed to deserialize AzuraCastNowPlayingResponse");

        assert!(parsed.is_online);
        let np = parsed.now_playing.expect("Expected now_playing");
        assert_eq!(np.duration, Some(238));
        assert_eq!(np.elapsed, Some(160));
        assert_eq!(np.remaining, Some(78));
        assert_eq!(np.duration_secs(), 238);
        assert!(np.elapsed_secs() >= 160);
        assert_eq!(np.remaining_secs(), np.duration_secs() - np.elapsed_secs());

        let next = parsed.playing_next.expect("Expected playing_next");
        // Float duration 235.25877585249 should round to 235
        assert_eq!(next.duration, Some(235));
    }

    #[test]
    fn test_parse_azuracast_station_mounts() {
        let json_data = std::fs::read_to_string("raw_json/neuro_21")
            .expect("Failed to read raw_json/neuro_21");
        let parsed: AzuraCastNowPlayingResponse = serde_json::from_str(&json_data)
            .expect("Failed to deserialize AzuraCastNowPlayingResponse");

        let station = parsed.station.expect("Expected station");
        assert_eq!(station.mounts.len(), 2);
        assert_eq!(station.mounts[0].name.as_deref(), Some("320kbps MP3"));
        assert_eq!(station.mounts[1].name.as_deref(), Some("Opus"));

        let streams = station.available_streams();
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].0, "320kbps MP3");
        assert_eq!(streams[0].1, "https://radio.twinskaraoke.com/listen/neuro_21/radio.mp3");
        assert_eq!(streams[1].0, "Opus");
        assert_eq!(streams[1].1, "https://radio.twinskaraoke.com/listen/neuro_21/radio.ogg");
    }

    #[test]
    fn test_effective_now_playing_transition() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut response = AzuraCastNowPlayingResponse {
            station: None,
            listeners: None,
            now_playing: Some(AzuraCastTrackInfo {
                sh_id: Some(1),
                played_at: Some(now_secs - 100),
                duration: Some(200),
                playlist: None,
                streamer: None,
                is_request: None,
                song: Some(AzuraCastSong {
                    id: Some("song1".into()),
                    text: Some("Song 1".into()),
                    artist: Some("Artist 1".into()),
                    title: Some("Title 1".into()),
                    art: None,
                    custom_fields: None,
                }),
                elapsed: Some(100),
                remaining: Some(100),
            }),
            playing_next: Some(AzuraCastTrackInfo {
                sh_id: Some(2),
                played_at: Some(now_secs + 100),
                duration: Some(200),
                playlist: None,
                streamer: None,
                is_request: None,
                song: Some(AzuraCastSong {
                    id: Some("song2".into()),
                    text: Some("Song 2".into()),
                    artist: Some("Artist 2".into()),
                    title: Some("Title 2".into()),
                    art: None,
                    custom_fields: None,
                }),
                elapsed: Some(0),
                remaining: Some(200),
            }),
            song_history: vec![],
            is_online: true,
        };

        let effective = response.effective_now_playing().unwrap();
        assert_eq!(effective.sh_id, Some(1));

        response.playing_next.as_mut().unwrap().played_at = Some(now_secs - 10);
        let promoted = response.effective_now_playing().unwrap();
        assert_eq!(promoted.sh_id, Some(2));
    }

    #[test]
    fn test_elapsed_secs_fallback() {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let track = AzuraCastTrackInfo {
            sh_id: Some(1),
            played_at: Some(now_secs - 100000),
            duration: Some(200),
            playlist: None,
            streamer: None,
            is_request: None,
            song: None,
            elapsed: Some(75),
            remaining: Some(125),
        };

        assert_eq!(track.elapsed_secs(), 75);
    }
}
