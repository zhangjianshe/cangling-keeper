// Prevents an additional console window on Windows in release builds, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if std::env::args().any(|arg| arg == "--web") {
        if let Err(error) = cangling_keeper_lib::run_web_from_args() {
            eprintln!("无法启动 cangling-keeper Web 服务：{error}");
            std::process::exit(1);
        }
        return;
    }

    // GTK/WebKit's native Wayland path can fail with a protocol error on some
    // compositor versions. Prefer the available X11/XWayland display until
    // that stack is known to be stable for this application.
    if std::env::var_os("DISPLAY").is_some() {
        // SAFETY: called once at startup, before any threads/GTK init.
        unsafe {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }

    cangling_keeper_lib::run()
}
