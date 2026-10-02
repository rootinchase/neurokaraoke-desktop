use crate::api::{
    API_URLS, FavoriteEntry, FavoriteItem, LazySongDatabase, LoadingState, Playlist,
    PlaylistDetail, ProfileResponse, SetlistStats, Song, SongDTO, TrendingTimes, UploadSong,
    UserLimits,
};
use crate::config::SharedConfig;
use crate::debug_log;
use anyhow::anyhow;
use dashmap::DashMap;
use reqwest::Client;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::{Value, json};
use std::sync::Arc;
use uuid::Uuid;

impl LazySongDatabase {
    pub fn new(
        client: Client,
        map: Arc<DashMap<Uuid, LoadingState<Song>>>,
        guest_id: Arc<str>,
        shared_config: SharedConfig,
    ) -> Self {
        Self {
            client,
            map,
            guest_id,
            shared_config,
        }
    }

    pub async fn get_profile(&self, token: &str) -> anyhow::Result<ProfileResponse> {
        let request = self
            .client
            .get(format!("{}/api/badge/profile", API_URLS.api))
            .bearer_auth(token);

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch profile: {}", response.status()));
        }

        let profile: ProfileResponse = response.json().await?;
        Ok(profile)
    }

    pub async fn get_user_limits(&self) -> anyhow::Result<UserLimits> {
        let request = self
            .client
            .get(format!("{}/api/user/upload-limits", API_URLS.api));
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch user limits: {}",
                response.status()
            ));
        }

        let limits: UserLimits = response.json().await?;
        Ok(limits)
    }

    pub async fn fetch_favorite_playlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let url = format!("{}/api/favorites/type?type=3", API_URLS.api);
        let request = self.client.get(url);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Failed to fetch favorite playlists: {}",
                response.status()
            ));
        }

        let items: Vec<FavoriteItem> = response.json().await?;

        let playlists = items.into_iter().filter_map(|item| item.playlist).collect();

        Ok(playlists)
    }

    async fn apply_auth(
        &self,
        mut req_builder: reqwest::RequestBuilder,
    ) -> reqwest::RequestBuilder {
        let token_lock = self.shared_config.auth_token.read().unwrap();

        if let Some(token) = token_lock.as_ref() {
            // User is signed in: append the verified JWT token directly
            req_builder = req_builder.bearer_auth(token);
        } else {
            // User is anonymous: fall back to the guest identifier header
            req_builder = req_builder.header("x-guest-id", self.guest_id.to_string());
        }

        req_builder
    }

    pub async fn get_public_playlists(
        &self,
        sort_by: Option<&str>,
        sort_desc: bool,
        start_index: u64,
        page_size: u64,
    ) -> anyhow::Result<Vec<Playlist>> {
        let mut url = format!(
            "{}/api/playlist/public?startIndex={}&pageSize={}",
            API_URLS.api, start_index, page_size
        );
        if let Some(sort) = sort_by {
            url.push_str(&format!("&sortBy={}&sortDescending={}", sort, sort_desc));
        }

        let mut request = self.client.get(url);
        request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_user_playlists(&self) -> anyhow::Result<Vec<Playlist>> {
        let mut request = self
            .client
            .get(format!("{}/api/user/playlists", API_URLS.api));
        request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_official_setlists(&self, year: u32) -> anyhow::Result<Vec<Playlist>> {
        let url = format!(
            "{}/api/playlists?pageSize=75&isSetlist=True&year={}",
            API_URLS.api, year
        );
        let mut request = self.client.get(url);
        request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let playlists: Vec<Playlist> = serde_json::from_value(json)?;
        Ok(playlists)
    }

    pub async fn get_setlist_stats(&self) -> anyhow::Result<SetlistStats> {
        let url = format!("{}/api/setlists/stats", API_URLS.api);
        let mut request = self.client.get(url);
        request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch setlist stats: {}",
                response.status()
            ));
        }

        let stats: SetlistStats = response.json().await?;
        Ok(stats)
    }

    pub async fn add_to_favorites(&self, song_id: Uuid) -> anyhow::Result<()> {
        debug_log!("Adding song to favorites: {}", song_id);
        let url = format!("{}/api/user/favorites/{}", API_URLS.api, song_id);
        let request = self.client.put(url);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to add to favorites: {}", response.status()));
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn upload_song(&self, upload: UploadSong) -> anyhow::Result<()> {
        debug_log!("Adding song to favorites: {}", upload.url);
        let url = format!("{}/api/user/song/download-from-url", API_URLS.idk);

        let request = self.client.post(url).json(&upload);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to upload: {}", response.status()));
        }
        Ok(())
    }

    pub async fn remove_from_favorites(&self, song_id: Uuid) -> anyhow::Result<()> {
        debug_log!("Removing song from favorites: {}", song_id);
        let url = format!("{}/api/user/favorites/{}", API_URLS.api, song_id);
        let request = self.client.delete(url);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to remove from favorites: {}",
                response.status()
            ));
        }
        Ok(())
    }

    pub async fn report_play_count(&self, song_id: Uuid) -> anyhow::Result<()> {
        debug_log!("🎵 Reporting play count for song: {}", song_id);
        let url = format!("{}/api/songs/playCount/{}", API_URLS.api, song_id);
        let request = self.client.put(url);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            debug_log!(
                "❌ Failed to report play count for song {}: status {}",
                song_id,
                response.status()
            );
            return Err(anyhow!(
                "Failed to report play count: {}",
                response.status()
            ));
        }
        debug_log!("✅ Successfully reported play count for song: {}", song_id);
        Ok(())
    }

    pub async fn get_favorite_songs(&self) -> anyhow::Result<Vec<SongDTO>> {
        let mut request = self
            .client
            .get(format!("{}/api/favorites/type?type=0", API_URLS.api));
        request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;

        // Flexible parsing: Handle different possible root keys or bare array
        let items = if let Some(arr) = json.as_array() {
            arr.clone()
        } else if let Some(obj) = json.as_object() {
            obj.get("songs")
                .or_else(|| obj.get("items"))
                .or_else(|| obj.get("favorites"))
                .or_else(|| obj.get("data"))
                .and_then(|v| v.as_array())
                .cloned()
                .ok_or_else(|| {
                    anyhow!("Could not find list of favorites in response: {:?}", json)
                })?
        } else {
            return Err(anyhow!("Invalid response structure: {:?}", json));
        };

        // Parse items. Some might be wrapped in a "song" object
        let mut songs: Vec<SongDTO> = Vec::new();
        for v in items {
            match serde_json::from_value::<FavoriteEntry>(v.clone()) {
                Ok(fav_entry) => {
                    if let Some(song) = fav_entry.song {
                        if song.is_valid() {
                            songs.push(song);
                        } else {
                            debug_log!(
                                "Warning: skipping entry with invalid song metadata: {:?}",
                                song.id
                            );
                        }
                    } else {
                        debug_log!("Warning: entry has no song field");
                    }
                }
                Err(e) => {
                    debug_log!("Error deserializing favorite entry: {:?}, item: {:?}", e, v);
                }
            }
        }

        Ok(songs)
    }

    pub async fn get_playlist_details(&self, id: Uuid) -> anyhow::Result<PlaylistDetail> {
        let mut request = self
            .client
            .get(format!("{}/api/playlist/{}", API_URLS.api, id));
        request = self.apply_auth(request).await; // <-- Inject headers

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch: {}", response.status()));
        }

        let json: Value = response.json().await?;
        let detail: PlaylistDetail = serde_json::from_value(json)?;
        // debug_log!("Deserialized PlaylistDetail: {:?}", detail);

        Ok(detail)
    }

    pub async fn get_suggested(&self, limit: usize) -> anyhow::Result<Vec<SongDTO>> {
        let url = format!("{}/api/user/suggestions?limit={}", API_URLS.api, limit);
        let request = self.client.get(url);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch suggested songs: {}",
                response.status()
            ));
        }

        let songs: Vec<SongDTO> = response.json().await?;
        Ok(songs)
    }

    pub async fn get_trending(&self, times: TrendingTimes) -> anyhow::Result<Vec<SongDTO>> {
        let days = times as u32;
        let url = format!("{}/api/explore/trendings?days={}", API_URLS.api, days);
        let request = self.client.get(url);
        let request = self.apply_auth(request).await;

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch trending songs: {}",
                response.status()
            ));
        }

        let songs: Vec<SongDTO> = response.json().await?;
        Ok(songs)
    }

    pub fn get<T>(&self, id: &Uuid, f: impl FnOnce(&Song) -> T) -> LoadingState<T> {
        if let Some(r) = self.map.get(id) {
            match &*r {
                LoadingState::Loaded(song) => LoadingState::Loaded(f(song)),
                LoadingState::Failed(err) => LoadingState::Failed(err.clone()),
                LoadingState::Loading => LoadingState::Loading,
            }
        } else {
            self.map.insert(id.clone(), LoadingState::Loading);
            let id = *id;
            let client = self.client.clone();
            let map = self.map.clone();
            let db_self = self.clone(); // Clone database handle to share within the task context

            tokio::spawn(async move {
                let url = format!("{}/api/songs/{}", API_URLS.api, id.to_string());
                let mut req = client.get(url);
                req = db_self.apply_auth(req).await; // <-- Inject token directly into the thread loop

                map.insert(
                    id,
                    match async {
                        Ok(serde_json::from_slice(
                            req.send().await?.bytes().await?.as_ref(),
                        )?)
                    }
                    .await
                    .map_err(Arc::new)
                    {
                        Ok(song) => LoadingState::Loaded(song),
                        Err(err) => LoadingState::Failed(err),
                    },
                );
            });
            LoadingState::Loading
        }
    }

    pub async fn load_all<T>(
        &self,
        f: impl FnMut(&Song) -> T,
    ) -> anyhow::Result<Arc<[LoadingState<T>]>> {
        let json = self
            .client
            .post(format!("{}/api/songs", API_URLS.api))
            .json(&json!({"page": 1, "pageSize": 0}))
            .send()
            .await?
            .json::<Value>()
            .await?;
        let total_count = json
            .get("totalCount")
            .ok_or_else(|| anyhow!("missing total count"))?
            .as_u64()
            .ok_or_else(|| anyhow!("missing total count"))?;
        if let Value::Object(ref mut obj) = self
            .client
            .post(format!("{}/api/songs", API_URLS.api))
            .json(&json!({"page": 1, "pageSize": total_count}))
            .send()
            .await?
            .json::<Value>()
            .await?
        {
            let songs: Vec<Value> = serde_json::from_value(
                obj.remove("items")
                    .ok_or_else(|| anyhow!("missing items"))?,
            )?;
            self.load(songs, f)
        } else {
            Err(anyhow!("invalid response type"))
        }
    }

    fn load<T>(
        &self,
        values: Vec<Value>,
        mut f: impl FnMut(&Song) -> T,
    ) -> anyhow::Result<Arc<[LoadingState<T>]>> {
        let mut result = Vec::with_capacity(values.len());
        for value in values {
            let id = Uuid::parse_str(
                value
                    .get("id")
                    .ok_or_else(|| anyhow!("song missing id???"))?
                    .as_str()
                    .ok_or_else(|| anyhow!("song missing id???"))?,
            )?;
            match serde_json::from_value::<Song>(value) {
                Ok(song) => {
                    result.push(LoadingState::Loaded(f(&song)));
                    self.map.insert(id, LoadingState::Loaded(song));
                }
                Err(err) => {
                    let e = Arc::new(anyhow!(err));
                    result.push(LoadingState::Failed(e.clone()));
                    self.map.insert(id, LoadingState::Failed(e));
                }
            }
        }
        Ok(result.into())
    }

    pub fn get_map(&self) -> &Arc<DashMap<Uuid, LoadingState<Song>>> {
        &self.map
    }
}

impl Serialize for LazySongDatabase {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;

        for entry in self.map.iter() {
            if let LoadingState::Loaded(song) = entry.value() {
                map.serialize_entry(entry.key(), song)
                    .map_err(serde::ser::Error::custom)?;
            }
        }

        map.end()
    }
}
