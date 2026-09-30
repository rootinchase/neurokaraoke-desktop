mod activity;
mod api;
mod app;
mod audio;
mod auth;
mod config;
mod theme;
mod utilities;

use app::factory::create_app;
use eframe::egui::Vec2;
use mimalloc::MiMalloc;
use std::sync::Arc;

// RustRover is stupid and wants to get rid of this crate... that's needed by egui_extras
use image as _;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> eframe::Result<()> {
    config::init_config_dir();

    if std::env::args().any(|arg| arg == "--dump-cache" || arg == "--clear-cache") {
        println!("🧹 [Cache] Dumping and clearing cache...");
        let c_dir = utilities::cache::cache_dir();
        if let Ok(entries) = std::fs::read_dir(c_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    let _ = std::fs::remove_file(&path);
                } else if path.is_dir() {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }
        let _ = utilities::persistence::clear_app_state();
        println!("✨ [Cache] Cache successfully cleared.");
    }

    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap(),
    );

    let _guard = runtime.enter();

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("Invalid icon");

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Neuro Karaoke App")
            .with_app_id("com.neurokaraoke.desktop")
            .with_icon(icon)
            .with_min_inner_size(Vec2::new(640.0, 480.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Neuro Karaoke App",
        options,
        Box::new(|cc| Ok(Box::new(create_app(cc, runtime.clone())))),
    )
}