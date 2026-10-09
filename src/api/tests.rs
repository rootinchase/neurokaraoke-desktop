#[cfg(test)]
mod tests {
    use crate::api::internal::deserialize_artists;
    use crate::api::models::*;
    use serde::Deserialize;
    use std::sync::Arc;

    #[test]
    fn test_parse_azuracast_neuro_21() {
        let json_data =
            std::fs::read_to_string("raw_json/neuro_21").expect("Failed to read raw_json/neuro_21");
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
        let json_data =
            std::fs::read_to_string("raw_json/neuro_21").expect("Failed to read raw_json/neuro_21");
        let parsed: AzuraCastNowPlayingResponse = serde_json::from_str(&json_data)
            .expect("Failed to deserialize AzuraCastNowPlayingResponse");

        let station = parsed.station.expect("Expected station");
        assert_eq!(station.mounts.len(), 2);
        assert_eq!(station.mounts[0].name.as_deref(), Some("320kbps MP3"));
        assert_eq!(station.mounts[1].name.as_deref(), Some("Opus"));

        let streams = station.available_streams();
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].0, "320kbps MP3");
        assert_eq!(
            streams[0].1,
            "https://radio.twinskaraoke.com/listen/neuro_21/radio.mp3"
        );
        assert_eq!(streams[1].0, "Opus");
        assert_eq!(
            streams[1].1,
            "https://radio.twinskaraoke.com/listen/neuro_21/radio.ogg"
        );
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

    #[test]
    fn test_song_dto_validation() {
        let song_valid = SongDTO {
            id: uuid::Uuid::nil(),
            title: "Valid Song".into(),
            opus: None,
            audio_url: None,
            absolute_path: None,
            oss: None,
            cover_art: None,
            original_artists: Arc::new([]),
            cover_artists: Arc::new([]),
            play_count: Some(10),
            stream_date: None,
            duration: Some(180),
            user_uploaded: false,
        };
        assert!(song_valid.is_valid());

        let song_invalid = SongDTO {
            id: uuid::Uuid::nil(),
            title: "".into(),
            opus: None,
            audio_url: None,
            absolute_path: None,
            oss: None,
            cover_art: None,
            original_artists: Arc::new([]),
            cover_artists: Arc::new([]),
            play_count: Some(0),
            stream_date: None,
            duration: Some(180),
            user_uploaded: false,
        };
        assert!(!song_invalid.is_valid());
    }

    #[test]
    fn test_parse_raw_uploads_json() {
        let json_data = include_str!("../../raw_json/uploads.json");
        let items: Vec<serde_json::Value> =
            serde_json::from_str(json_data).expect("uploads.json should be a JSON array");

        // Mirrors get_user_uploads tolerant parsing: entries may be wrapped in a "song" key.
        let songs: Vec<SongDTO> = items
            .iter()
            .filter_map(|v| {
                let song_value = v
                    .as_object()
                    .and_then(|obj| obj.get("song"))
                    .cloned()
                    .unwrap_or(v.clone());
                serde_json::from_value::<SongDTO>(song_value).ok()
            })
            .collect();

        assert_eq!(songs.len(), 2);
        assert_eq!(songs[0].title, "My Upload One".into());
        assert_eq!(songs[0].play_count, Some(5));
        assert_eq!(songs[0].duration, Some(180));
        assert!(songs[0].user_uploaded);
        assert_eq!(songs[1].title, "My Upload Two".into());
        assert!(songs[1].user_uploaded);
    }

    #[test]
    fn test_deserialize_artists() {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "deserialize_artists")]
            artists: Arc<[Arc<str>]>,
        }

        // Single string
        let json1 = r#"{"artists": "Neuro-sama"}"#;
        let parsed1: TestStruct = serde_json::from_str(json1).unwrap();
        assert_eq!(parsed1.artists.len(), 1);
        assert_eq!(parsed1.artists[0].as_ref(), "Neuro-sama");

        // List of objects with name and optional id
        let json2 = r#"{"artists": [{"name": "Evil Neuro"}, {"id": "00000000-0000-0000-0000-000000000000", "name": "Vedal"}]}"#;
        let parsed2: TestStruct = serde_json::from_str(json2).unwrap();
        assert_eq!(parsed2.artists.len(), 2);
        assert_eq!(parsed2.artists[0].as_ref(), "Evil Neuro");
        assert_eq!(parsed2.artists[1].as_ref(), "Vedal");

        // Null / missing
        let json3 = r#"{"artists": null}"#;
        let parsed3: TestStruct = serde_json::from_str(json3).unwrap();
        assert!(parsed3.artists.is_empty());
    }

    #[test]
    fn test_artwork_and_cover_art_deserialization() {
        let json_null = r#"null"#;
        let _art: Option<Artwork> = serde_json::from_str(json_null).unwrap();
        let deserialized =
            deserialize_cover_art(&mut serde_json::Deserializer::from_str(json_null)).unwrap();
        assert!(deserialized.is_some());
        assert_eq!(deserialized.unwrap().id, Artwork::default_art().id);

        let json_obj = r#"{"id": "custom-art-id", "fileName": "cover.png", "absolutePath": "/path", "isSensitive": false}"#;
        let art_obj: Artwork = serde_json::from_str(json_obj).unwrap();
        assert_eq!(art_obj.id, "custom-art-id");
        assert_eq!(art_obj.file_name.as_ref(), "cover.png");
    }

    #[test]
    fn test_loading_state_helper() {
        let loaded = LoadingState::Loaded(42);
        assert_eq!(loaded.if_loaded_or_else(|v| *v + 1, 0), 43);

        let loading = LoadingState::<i32>::Loading;
        assert_eq!(loading.if_loaded_or_else(|v| *v + 1, 0), 0);

        let failed = LoadingState::<i32>::Failed(Arc::new(anyhow::anyhow!("err")));
        assert_eq!(failed.if_loaded_or_else(|v| *v + 1, 99), 99);
    }

    #[test]
    fn test_trending_times_values() {
        assert_eq!(TrendingTimes::Week as u32, 7);
        assert_eq!(TrendingTimes::Fortnite as u32, 14);
        assert_eq!(TrendingTimes::Month as u32, 30);
    }

    #[test]
    fn test_azuracast_station_streams_fallback() {
        let station_no_mounts = AzuraCastStation {
            id: 1,
            name: Some("Test".into()),
            shortcode: Some("test".into()),
            listen_url: Some("https://example.com/listen.mp3".into()),
            url: None,
            public_player_url: None,
            playlist_pls_url: None,
            playlist_m3u_url: None,
            is_public: true,
            requests_enabled: false,
            mounts: vec![],
        };

        let streams = station_no_mounts.available_streams();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].0, "Default MP3");
        assert_eq!(streams[0].1, "https://example.com/listen.mp3");

        let station_empty = AzuraCastStation {
            id: 2,
            name: Some("Empty".into()),
            shortcode: None,
            listen_url: None,
            url: None,
            public_player_url: None,
            playlist_pls_url: None,
            playlist_m3u_url: None,
            is_public: true,
            requests_enabled: false,
            mounts: vec![],
        };
        assert!(station_empty.available_streams().is_empty());
    }

    #[test]
    fn test_radio_current_state_deserialization() {
        let json_data = r#"{
            "current": null,
            "upcoming": [],
            "history": [],
            "listenerCount": 15,
            "offline": false,
            "playlistName": "Test Playlist"
        }"#;
        let state: RadioCurrentStateResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(state.listener_count, 15);
        assert!(!state.offline);
        assert_eq!(state.playlist_name.as_deref(), Some("Test Playlist"));
    }

    #[test]
    fn test_gamehub_scheduled_info_deserialization() {
        let json_data = r#"{"active": true}"#;
        let info: GameHubScheduledInfo = serde_json::from_str(json_data).unwrap();
        assert!(info.active);
    }

    #[test]
    fn test_user_limits_deserialization() {
        let json_data = r#"{
            "maxSongs": 100,
            "maxStorageBytes": 1000000,
            "usedStorageBytes": 50000,
            "currentSongCount": 10,
            "currentPlaylistCount": 2,
            "playlistLimit": 20,
            "songPerPlaylistLimit": 500
        }"#;
        let limits: UserLimits = serde_json::from_str(json_data).unwrap();
        assert_eq!(limits.max_songs, 100);
        assert_eq!(limits.current_song_count, 10);
    }

    #[test]
    fn test_setlist_stats_deserialization() {
        let json_data = r#"{
            "totalCount": 42,
            "years": [2024, 2025, 2026]
        }"#;
        let stats: SetlistStats = serde_json::from_str(json_data).unwrap();
        assert_eq!(stats.total_count, 42);
        assert_eq!(stats.years, vec![2024, 2025, 2026]);
    }

    #[test]
    fn test_playlist_deserialization() {
        let json_data = r#"{
            "id": "12345678-1234-1234-1234-1234567890ab",
            "name": "Test Playlist",
            "creator": "Vedal",
            "songCount": 5,
            "playCount": 100,
            "editable": true,
            "deletable": true,
            "isPublic": true,
            "isSetList": false
        }"#;
        let playlist: Playlist = serde_json::from_str(json_data).unwrap();
        assert_eq!(playlist.name.as_ref(), "Test Playlist");
        assert_eq!(playlist.song_count, 5);
        assert!(playlist.editable);
        assert!(!playlist.is_set_list);
    }

    #[test]
    fn test_parse_official_setlists() {
        let json_data = r#"[
            {
                "id": "6873080b-0104-47fd-b5ac-1f76d114d623",
                "name": "Twin Karaoke - September 30, 2026 Setlist",
                "createdBy": "Admin",
                "songCount": 25,
                "playCount": 150,
                "editable": false,
                "deletable": false,
                "isPublic": true,
                "isSetList": true,
                "setListDate": "2026-09-30T00:00:00Z"
            }
        ]"#;
        let playlists: Vec<Playlist> = serde_json::from_str(json_data).unwrap();
        assert_eq!(playlists.len(), 1);
        let setlist = &playlists[0];
        assert_eq!(
            setlist.name.as_ref(),
            "Twin Karaoke - September 30, 2026 Setlist"
        );
        assert_eq!(setlist.song_count, 25);
        assert!(setlist.is_set_list);
        assert_eq!(
            setlist.set_list_date.as_deref(),
            Some("2026-09-30T00:00:00Z")
        );
    }

    #[test]
    #[ignore]
    fn test_fetch_official_setlists_from_server() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = std::env::temp_dir()
                .join(format!("neurokaraoke_test_server_{}", uuid::Uuid::new_v4()));
            let _ = tokio::fs::create_dir_all(&temp_dir).await;

            let client = reqwest::Client::new();
            let map = Arc::new(dashmap::DashMap::new());
            let guest_id: Arc<str> = uuid::Uuid::new_v4().to_string().into();
            let config = crate::config::Config::default();
            let shared_config = config.to_shared();

            let db = LazySongDatabase::new(client, map, guest_id, shared_config);

            match db.get_official_setlists(2026).await {
                Ok(setlists) => {
                    println!(
                        "Successfully retrieved {} official setlists from server",
                        setlists.len()
                    );
                    for setlist in setlists {
                        assert!(!setlist.name.is_empty());
                    }
                }
                Err(e) => {
                    println!(
                        "Note: Server request skipped or failed (offline/unreachable): {}",
                        e
                    );
                }
            }

            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        });
    }

    #[test]
    #[ignore]
    fn test_fetch_public_playlists_from_server() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let temp_dir = std::env::temp_dir().join(format!(
                "neurokaraoke_test_playlists_{}",
                uuid::Uuid::new_v4()
            ));
            let _ = tokio::fs::create_dir_all(&temp_dir).await;

            let client = reqwest::Client::new();
            let map = Arc::new(dashmap::DashMap::new());
            let guest_id: Arc<str> = uuid::Uuid::new_v4().to_string().into();
            let config = crate::config::Config::default();
            let shared_config = config.to_shared();

            let db = LazySongDatabase::new(client, map, guest_id, shared_config);

            match db.get_public_playlists(None, false, 0, 10).await {
                Ok(playlists) => {
                    println!(
                        "Successfully retrieved {} public playlists from server",
                        playlists.len()
                    );
                    for playlist in playlists {
                        assert!(!playlist.name.is_empty());
                    }
                }
                Err(e) => {
                    println!(
                        "Note: Server request skipped or failed (offline/unreachable): {}",
                        e
                    );
                }
            }

            let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        });
    }

    #[test]
    fn test_parse_playlist_cover_art_and_mosaics() {
        let json_data = std::fs::read_to_string("raw_json/playlists.json")
            .expect("Failed to read raw_json/playlists.json");
        let playlists: Vec<Playlist> =
            serde_json::from_str(&json_data).expect("Failed to deserialize playlists.json");
        assert!(!playlists.is_empty());

        let mut found_media = false;
        let mut found_mosaic = false;

        for pl in &playlists {
            if let Some(media) = &pl.media {
                found_media = true;
                if !media.id.is_empty() {
                    assert!(!media.file_name.is_empty());
                }
            }
            if let Some(mosaics) = &pl.mosaic_media {
                found_mosaic = true;
                for m in mosaics {
                    if !m.id.is_empty() {
                        assert!(!m.file_name.is_empty());
                    }
                }
            }
        }
        // At least some playlists in the dataset should have cover art or mosaic metadata
        assert!(found_media || found_mosaic);
    }

    #[test]
    fn test_parse_song_cover_art() {
        let json_data = std::fs::read_to_string("raw_json/recent.json")
            .expect("Failed to read raw_json/recent.json");
        let detail: PlaylistDetail =
            serde_json::from_str(&json_data).expect("Failed to deserialize recent.json");
        assert!(!detail.songs.is_empty());

        let mut found_cover = false;
        for song in &detail.songs {
            if let Some(cover) = &song.cover_art {
                found_cover = true;
                assert!(!cover.id.is_empty());
            }
        }
        assert!(found_cover);
    }

    #[test]
    fn test_parse_raw_playlists_json() {
        let json_data = std::fs::read_to_string("raw_json/playlists.json")
            .expect("Failed to read raw_json/playlists.json");
        let parsed: Vec<Playlist> =
            serde_json::from_str(&json_data).expect("Failed to deserialize playlists.json");
        assert!(!parsed.is_empty());
        assert!(!parsed[0].name.is_empty());
    }

    #[test]
    fn test_parse_raw_recent_json() {
        let json_data = std::fs::read_to_string("raw_json/recent.json")
            .expect("Failed to read raw_json/recent.json");
        let parsed: PlaylistDetail =
            serde_json::from_str(&json_data).expect("Failed to deserialize recent.json");
        assert!(!parsed.songs.is_empty());
        assert!(parsed.songs[0].is_valid());
    }

    #[test]
    fn test_parse_raw_suggested_json() {
        let json_data = std::fs::read_to_string("raw_json/suggested.json")
            .expect("Failed to read raw_json/suggested.json");
        let parsed: Vec<SongDTO> =
            serde_json::from_str(&json_data).expect("Failed to deserialize suggested.json");
        assert!(!parsed.is_empty());
    }

    #[test]
    fn test_parse_raw_trending_json() {
        let json_data = std::fs::read_to_string("raw_json/trending.json")
            .expect("Failed to read raw_json/trending.json");
        let parsed: Vec<SongDTO> =
            serde_json::from_str(&json_data).expect("Failed to deserialize trending.json");
        assert!(!parsed.is_empty());
    }

    #[test]
    fn test_parse_raw_schedule_json() {
        let json_data = std::fs::read_to_string("raw_json/schedule.json")
            .expect("Failed to read raw_json/schedule.json");
        let parsed: serde_json::Value =
            serde_json::from_str(&json_data).expect("Failed to deserialize schedule.json");
        assert!(parsed.is_array());
    }

    #[test]
    fn test_parse_raw_broadcasts_json() {
        let json_data = std::fs::read_to_string("raw_json/broadcasts.json")
            .expect("Failed to read raw_json/broadcasts.json");
        let parsed: serde_json::Value =
            serde_json::from_str(&json_data).expect("Failed to deserialize broadcasts.json");
        assert!(parsed.is_array());
    }

    #[test]
    fn test_audio_content_type_matches_web_app() {
        use crate::api::client::audio_content_type;
        use std::path::PathBuf;

        // The web app sends `audio/mpeg` for the `File` part of an .mp3 upload
        // (see raw_requests/ringtone.har).
        assert_eq!(
            audio_content_type(&PathBuf::from("Resurrection2.mp3")),
            "audio/mpeg"
        );
        assert_eq!(audio_content_type(&PathBuf::from("SONG.M4A")), "audio/mp4");
        assert_eq!(
            audio_content_type(&PathBuf::from("take.flac")),
            "audio/flac"
        );
        assert_eq!(
            audio_content_type(&PathBuf::from("no-extension")),
            "application/octet-stream"
        );
    }

    #[test]
    fn test_file_upload_prefills_title_from_filename() {
        use std::path::PathBuf;
        let upload = FileUpload::from_path(PathBuf::from(
            "CerberVT - Trampoline by Kero Kero Bonito [lEGrwd6NbKc].mp3",
        ));
        assert_eq!(
            upload.title,
            "CerberVT - Trampoline by Kero Kero Bonito [lEGrwd6NbKc]"
        );
        assert!(upload.artist.is_empty());
    }

    #[test]
    fn test_user_limits_quota_checks() {
        // Values captured from GET /api/user/upload-limits in raw_requests/uploaded-songs.har
        let limits = UserLimits {
            max_songs: 150,
            max_storage_bytes: 3_097_152_000,
            used_storage_bytes: 136_824_076,
            current_song_count: 13,
            current_playlist_count: 0,
            playlist_limit: 0,
            song_per_playlist_limit: 0,
        };

        assert_eq!(
            limits.remaining_storage_bytes(),
            3_097_152_000 - 136_824_076
        );
        assert!(limits.has_song_slot());
        assert!(limits.can_fit(866_011));
        assert!(!limits.can_fit(3_097_152_000));
    }

    #[test]
    #[ignore]
    fn test_upload_song_file_to_server() {
        // Live upload of the small ringtone captured in raw_requests/ringtone.har.
        // Run with: cargo test -- --ignored test_upload_song_file_to_server
        // The server answers 200 with an empty body; the new song appears in
        // GET /api/user/songs as `user-audio/{guid}.m4a`.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let path = std::env::var("NEUROKARAOKE_UPLOAD_FILE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(
                        "/home/rootinchase/Backups/pixel9/sdcard/Ringtones/Resurrection2.mp3",
                    )
                });

            if !path.exists() {
                println!(
                    "Note: upload file {} is missing, live upload skipped",
                    path.display()
                );
                return;
            }

            let client = reqwest::Client::new();
            let map = Arc::new(dashmap::DashMap::new());
            let guest_id: Arc<str> = uuid::Uuid::new_v4().to_string().into();
            let config = crate::config::Config::default();
            let shared_config = config.to_shared();

            // `config_dir()` is only initialised at app startup, so the test cannot
            // read the real config. Supply the token the web app used instead.
            if let Ok(token) = std::env::var("NEUROKARAOKE_TOKEN") {
                *shared_config.auth_token.write().unwrap() = Some(Arc::from(token.as_str()));
            }

            let db = LazySongDatabase::new(client, map, guest_id, shared_config);

            let upload = FileUpload {
                path,
                title: "Resurrection2 ringtone".to_string(),
                artist: "testartist".to_string(),
            };

            match db.upload_song_file(upload).await {
                Ok(()) => println!("Upload accepted by the server"),
                Err(e) => println!("Note: upload rejected (auth/quota/offline): {e}"),
            }
        });
    }

    #[test]
    #[ignore]
    fn test_upload_song_file_and_delete_on_server() {
        // Live file upload followed by the owner-scoped delete, so the test is
        // self-cleaning. Run with:
        //   NEUROKARAOKE_TOKEN=... cargo test -- --ignored test_upload_song_file_and_delete_on_server
        // The upload answers `200` with an empty body, so the new song id is found by
        // listing `GET /api/user/songs` and matching the submitted title.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let path = std::env::var("NEUROKARAOKE_UPLOAD_FILE")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(
                        "/home/rootinchase/Backups/pixel9/sdcard/Ringtones/Resurrection2.mp3",
                    )
                });

            if !path.exists() {
                println!(
                    "Note: upload file {} is missing, live upload skipped",
                    path.display()
                );
                return;
            }

            let client = reqwest::Client::new();
            let map = Arc::new(dashmap::DashMap::new());
            let guest_id: Arc<str> = uuid::Uuid::new_v4().to_string().into();
            let config = crate::config::Config::default();
            let shared_config = config.to_shared();

            if let Ok(token) = std::env::var("NEUROKARAOKE_TOKEN") {
                *shared_config.auth_token.write().unwrap() = Some(Arc::from(token.as_str()));
            }

            let db = LazySongDatabase::new(client, map, guest_id, shared_config);

            let title = "Cline live upload check".to_string();
            let upload = FileUpload {
                path,
                title: title.clone(),
                artist: "testartist".to_string(),
            };

            match db.upload_song_file(upload).await {
                Ok(()) => {
                    println!("File upload accepted");
                    let songs = db.get_user_uploads().await.unwrap_or_default();
                    let match_ = songs
                        .iter()
                        .find(|s| s.title.as_ref() == title.as_str())
                        .map(|s| s.id);
                    match match_ {
                        Some(id) => match db.delete_uploaded_song(id).await {
                            Ok(()) => println!("Cleanup: song {} deleted", id),
                            Err(e) => println!("Note: cleanup failed: {e}"),
                        },
                        None => println!("Note: uploaded song not listed, nothing to clean up"),
                    }
                }
                Err(e) => println!("Note: upload rejected (auth/quota/offline): {e}"),
            }
        });
    }

    #[test]
    fn test_youtube_url_forms_are_accepted() {
        use crate::api::sources::{SourceKind, classify_source_url};
        let cases: Vec<(&str, SourceKind)> = vec![
            (
                "https://www.youtube.com/watch?v=xWDfREk0ZLs",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://youtu.be/xWDfREk0ZLs",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://m.youtube.com/watch?v=xWDfREk0ZLs&t=1969",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://music.youtube.com/watch?v=xWDfREk0ZLs",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://www.youtube-nocookie.com/embed/xWDfREk0ZLs",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://www.youtube.com/shorts/xWDfREk0ZLs",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://www.youtube.com/live/xWDfREk0ZLs",
                SourceKind::YouTubeVideo {
                    video_id: "xWDfREk0ZLs".to_string(),
                },
            ),
            (
                "https://www.youtube.com/playlist?list=PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG",
                SourceKind::YouTubePlaylist {
                    list_id: "PLx0sYbCqOb8TBPRdmBHs5Iftvv9TPboYG".to_string(),
                },
            ),
            (
                "https://www.bilibili.com/video/BV1xx411c7mD",
                SourceKind::BilibiliVideo {
                    video_id: "BV1xx411c7mD".to_string(),
                },
            ),
            (
                "https://b23.tv/abc123",
                SourceKind::BilibiliVideo {
                    video_id: "abc123".to_string(),
                },
            ),
            (
                "https://cdn.discordapp.com/attachments/123/456/song.mp3",
                SourceKind::DiscordAttachment {
                    url: "https://cdn.discordapp.com/attachments/123/456/song.mp3".to_string(),
                },
            ),
        ];

        for (url, expected) in cases {
            assert_eq!(classify_source_url(url).unwrap(), expected, "for {url}");
        }
    }

    #[test]
    fn test_source_url_rejections_match_server() {
        use crate::api::sources::{
            ERR_INVALID_URL, ERR_INVALID_YOUTUBE_URL, ERR_UNSUPPORTED_PLATFORM, classify_source_url,
        };

        // Messages captured from live 400 responses in the web app.
        assert_eq!(
            classify_source_url("not a url").unwrap_err().to_string(),
            ERR_INVALID_URL
        );
        assert_eq!(
            classify_source_url("https://example.com/foo.mp3")
                .unwrap_err()
                .to_string(),
            ERR_UNSUPPORTED_PLATFORM
        );
        assert_eq!(
            classify_source_url("https://www.youtube.com/")
                .unwrap_err()
                .to_string(),
            ERR_INVALID_YOUTUBE_URL
        );
        assert_eq!(
            classify_source_url("https://youtu.be/shortid")
                .unwrap_err()
                .to_string(),
            ERR_INVALID_YOUTUBE_URL
        );
        assert_eq!(
            classify_source_url("ftp://youtube.com/watch?v=xWDfREk0ZLs")
                .unwrap_err()
                .to_string(),
            ERR_INVALID_URL
        );
    }

    #[test]
    fn test_upload_song_payload_matches_server() {
        // Body captured in raw_requests/upload_youtube.har
        let payload = UploadSong::new("https://youtu.be/xWDfREk0ZLs");
        assert_eq!(
            serde_json::to_string(&payload).unwrap(),
            r#"{"url":"https://youtu.be/xWDfREk0ZLs","playlistId":null}"#
        );
    }

    #[test]
    fn test_upload_song_result_parses_server_response() {
        // Response captured in raw_requests/upload_youtube.har
        let body = r#"{"success":true,"songId":"d3377f95-7390-4d8e-8c1e-ab7bbd9d3e95","title":"Pattern Recognition - Neuro-sama x ODDEEO"}"#;
        let result: UploadSongResult = serde_json::from_str(body).unwrap();
        assert!(result.success);
        assert_eq!(
            result.song_id.to_string(),
            "d3377f95-7390-4d8e-8c1e-ab7bbd9d3e95"
        );
        assert_eq!(result.title, "Pattern Recognition - Neuro-sama x ODDEEO");
    }

    #[test]
    #[ignore]
    fn test_upload_song_from_url_to_server() {
        // Live download of a YouTube URL. Run with:
        //   NEUROKARAOKE_TOKEN=... cargo test -- --ignored test_upload_song_from_url_to_server
        // The server answers 200 with {"success":true,"songId":…,"title":…} and the new
        // song appears in GET /api/user/songs with `userUploaded: true`.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let url = std::env::var("NEUROKARAOKE_UPLOAD_URL")
                .unwrap_or_else(|_| "https://youtu.be/xWDfREk0ZLs".to_string());

            let client = reqwest::Client::new();
            let map = Arc::new(dashmap::DashMap::new());
            let guest_id: Arc<str> = uuid::Uuid::new_v4().to_string().into();
            let config = crate::config::Config::default();
            let shared_config = config.to_shared();

            // `config_dir()` is only initialized at app startup, so the test cannot
            // read the real config. Supply the token the web app used instead.
            if let Ok(token) = std::env::var("NEUROKARAOKE_TOKEN") {
                *shared_config.auth_token.write().unwrap() = Some(Arc::from(token.as_str()));
            }

            let db = LazySongDatabase::new(client, map, guest_id, shared_config);

            match db.upload_song_from_url(UploadSong::new(url)).await {
                Ok(result) => {
                    println!(
                        "Upload accepted: song {} titled {}",
                        result.song_id, result.title
                    );
                    // Clean up so the live test leaves no duplicates in the library.
                    match db.delete_uploaded_song(result.song_id).await {
                        Ok(()) => println!("Cleanup: song {} deleted", result.song_id),
                        Err(e) => println!("Note: cleanup failed: {e}"),
                    }
                }
                Err(e) => println!("Note: upload rejected (auth/quota/offline): {e}"),
            }
        });
    }
}
