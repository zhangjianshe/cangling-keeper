// Prevents an additional console window on Windows in release builds, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The AppImage's GTK hook forces `GDK_BACKEND=x11`, which renders through
    // XWayland. On fractional-scaled Wayland sessions (e.g. 125%) XWayland
    // renders at 1x and the compositor upscales the result, making text blurry.
    // Prefer native Wayland rendering so GTK/WebKit use the correct fractional
    // scale factor and render crisply.
    let is_wayland = std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE")
            .map(|v| v == "wayland")
            .unwrap_or(false);
    if is_wayland {
        // SAFETY: called once at startup, before any threads/GTK init.
        unsafe {
            std::env::set_var("GDK_BACKEND", "wayland");
        }
    }

    cangling_keeper_lib::run()
}
