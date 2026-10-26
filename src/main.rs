#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = cangling_keeper_lib::run_web_from_args() {
        eprintln!("无法启动 cangling-keeper Web 服务：{error}");
        std::process::exit(1);
    }
}
