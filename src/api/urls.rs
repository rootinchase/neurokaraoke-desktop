pub struct ApiUrls {
    pub base: &'static str,
    pub api: &'static str,
    pub idk: &'static str,
    pub images: &'static str,
    pub storage: &'static str,
    pub account_hash: &'static str,
    pub socket: &'static str,
    pub radio: &'static str,
}

pub const API_URLS: ApiUrls = ApiUrls {
    base: "https://neurokaraoke.com",
    api: "https://api.neurokaraoke.com",
    idk: "https://idk.neurokaraoke.com",
    images: "https://images.neurokaraoke.com",
    storage: "https://storage.neurokaraoke.com",
    account_hash: "WxURxyML82UkE7gY-PiBKw",
    socket: "https://socket.neurokaraoke.com",
    radio: "https://radio.twinskaraoke.com",
};
