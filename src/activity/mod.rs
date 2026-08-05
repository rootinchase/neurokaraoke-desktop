pub mod home;

use eframe::egui::{include_image, ImageSource};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivityType {
    Home, Search, Profile
}

impl ActivityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Search => "Search",
            Self::Profile => "Profile",
        }
    }

    pub fn icon(&self) -> Option<ImageSource<'static>> {
        match self {
            Self::Home => Some(include_image!("../../assets/home.png")),
            Self::Search => Some(include_image!("../../assets/search.png")),
            _ => None,
        }
    }
}