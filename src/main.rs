mod activity;
mod api;
mod app;
mod audio;
mod auth;
mod config;
mod theme;
mod utilities;

use crate::config::init_config_dir;
use crate::utilities::cache::cache_dir;
use crate::utilities::persistence::clear_app_state;
use app::factory::create_app;
use eframe::egui::{Vec2, ViewportBuilder};
use eframe::icon_data::from_png_bytes;
use eframe::{NativeOptions, Result, run_native};
use mimalloc::MiMalloc;
use std::env::args;
use std::fs::{read_dir, remove_dir_all, remove_file};
use std::sync::Arc;
use tokio::runtime::Builder;

// RustRover is stupid and wants to get rid of this crate... that's needed by egui_extras
use image as _;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> Result<()> {
    init_config_dir();

    if args().any(|arg| arg == "--dump-cache" || arg == "--clear-cache") {
        println!("🧹 [Cache] Dumping and clearing cache...");
        let c_dir = cache_dir();
        if let Ok(entries) = read_dir(c_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let _ = remove_file(&path);
                } else if path.is_dir() {
                    let _ = remove_dir_all(&path);
                }
            }
        }
        let _ = clear_app_state();
        println!("✨ [Cache] Cache successfully cleared.");
    }

    let runtime = Arc::new(Builder::new_multi_thread().enable_all().build().unwrap());

    let _guard = runtime.enter();

    let icon = from_png_bytes(include_bytes!("../assets/icon.png")).expect("Invalid icon");

    let options = NativeOptions {
        viewport: ViewportBuilder::default()
            .with_title("Neuro Karaoke App")
            .with_app_id("com.neurokaraoke.desktop")
            .with_icon(icon)
            .with_min_inner_size(Vec2::new(640.0, 480.0)),
        ..Default::default()
    };

    run_native(
        "Neuro Karaoke App",
        options,
        Box::new(|cc| Ok(Box::new(create_app(cc, runtime.clone())))),
    )
}
