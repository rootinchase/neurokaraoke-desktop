pub mod home;
pub mod playlist;
pub mod setlist;
pub mod profile;
pub mod favorites;

use eframe::egui::{include_image, ImageSource};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityType {
    Home, Search, Profile, Playlists, MyPlaylists, Setlists, Favorites
}

impl ActivityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Profile => "Profile",
            Self::Playlists => "Public Playlists",
            Self::MyPlaylists => "My Playlists",
            Self::Setlists => "Official Setlists",
            Self::Favorites => "Favorites",
        }
    }

    pub fn icon(&self) -> Option<ImageSource<'static>> {
        match self {
            Self::Home => Some(include_image!("../../assets/home.png")),
            Self::Search => Some(include_image!("../../assets/search.png")),
            Self::Playlists => Some(include_image!("../../assets/search.png")),
            Self::MyPlaylists => Some(include_image!("../../assets/search.png")),
            Self::Setlists => Some(include_image!("../../assets/search.png")),
            Self::Favorites => Some(include_image!("../../assets/search.png")),
            Self::Profile => Some(include_image!("../../assets/icon.png")),
        }
    }
}