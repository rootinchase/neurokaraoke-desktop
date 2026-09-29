mod activity;
mod api;
mod app;
mod audio;
mod auth;
mod config;
mod theme;
mod utilities;

use app::factory::create_app;
use mimalloc::MiMalloc;
use std::sync::Arc;
use eframe::egui::Vec2;

// RustRover is stupid and wants to get rid of this crate... that's needed by egui_extras
use image as _;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> eframe::Result<()> {
    config::init_config_dir();

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
            .with_icon(icon)
            .with_min_inner_size(Vec2::new(640.0, 480.0)),
        ..Default::default()
    };

    eframe::run_native(
        "Karaoke App",
        options,
        Box::new(|cc| Ok(Box::new(create_app(cc, runtime.clone())))),
    )
}
