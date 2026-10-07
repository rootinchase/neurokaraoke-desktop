pub use playwire::{
    Capabilities, Event, MediaControls, PlaybackState, PlayerConfig, Repeat, Track,
};

use crate::audio::{LoopMode, Player};
use crate::config::SharedConfig;
use crate::debug_log;

pub(crate) fn init_playwire(player: Player, shared_config: SharedConfig) -> Option<MediaControls> {
    // Windows uses this, but macOS and windows don't.
    #[allow(unused_mut, unused_variables)]
    let mut hwnd_ptr: Option<*mut std::ffi::c_void> = None;
    #[cfg(target_os = "windows")]
    {
        unsafe extern "system" {
            fn GetActiveWindow() -> *mut std::ffi::c_void;
            fn GetWindowThreadProcessId(hwnd: *mut std::ffi::c_void, process_id: *mut u32) -> u32;
            fn GetCurrentProcessId() -> u32;
            fn FindWindowA(class_name: *const u8, window_name: *const u8) -> *mut std::ffi::c_void;
        }

        let our_pid = unsafe { GetCurrentProcessId() };

        let mut hwnd = unsafe { GetActiveWindow() };

        let mut pid = 0;
        if !hwnd.is_null() {
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        }

        if hwnd.is_null() || pid != our_pid {
            let title = b"Neuro Karaoke App\0";
            hwnd = unsafe { FindWindowA(std::ptr::null(), title.as_ptr()) };
        }

        if !hwnd.is_null() {
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid == our_pid {
                hwnd_ptr = Some(hwnd);
            }
        }

        if hwnd_ptr.is_none() {
            debug_log!(
                "⚠️ [Playwire] Warning: Windows HWND could not be resolved yet. Media controls will retry."
            );
            return None;
        }
    }

    let mut dbus_name = "neurokaraoke.desktop";

    if cfg!(debug_assertions) {
        dbus_name = "neurokaraoke.desktop.dev";
    }

    // This gets edited on Windows, so it needs to be mutable
    #[allow(unused_mut)]
    let mut playwire_config = PlayerConfig::new(dbus_name)
        .desktop_entry("neurokaraoke.desktop")
        .track_id_prefix("/neurokaraoke/desktop/track");

    #[cfg(target_os = "windows")]
    if let Some(hwnd) = hwnd_ptr {
        playwire_config = playwire_config.hwnd(hwnd as u64);
    }

    match MediaControls::new(playwire_config, move |event| {
        match event {
            Event::Play => player.play(),
            Event::Pause => player.pause(),
            Event::PlayPause => {
                if let Some(state) = player.get_playback_state() {
                    if state.paused() {
                        player.play();
                    } else {
                        player.pause();
                    }
                }
            }
            Event::Next => player.next_song(),
            Event::Previous => player.previous(),
            Event::SeekTo(pos) => player.seek(pos),
            Event::SetVolume(vol) => player.volume(vol as f32),
            Event::SetRepeat(mode) => {
                let loop_mode = match mode {
                    Repeat::All => LoopMode::All,
                    Repeat::One => LoopMode::One,
                    Repeat::Off => LoopMode::None,
                };
                player.looping(loop_mode);

                // Update shared config so App can see the change
                let mode_u32 = match loop_mode {
                    LoopMode::None => 0,
                    LoopMode::One => 1,
                    LoopMode::All => 2,
                };
                shared_config
                    .loop_mode
                    .store(mode_u32, std::sync::atomic::Ordering::SeqCst);
            }
            Event::SetShuffle(mode) => {
                player.shuffle(mode);
                shared_config
                    .shuffle
                    .store(mode, std::sync::atomic::Ordering::SeqCst);
            }
            _ => {}
        }
    }) {
        Ok(controls) => {
            debug_log!("✨ [Playwire] Media controls initialized successfully.");
            Some(controls)
        }
        Err(e) => {
            debug_log!(
                "❌ [Playwire] Failed to initialize playwire media controls: {:?}. Disabling media controls.",
                e
            );
            None
        }
    }
}
