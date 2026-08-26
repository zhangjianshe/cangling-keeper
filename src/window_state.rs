use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, WebviewWindow, WindowEvent};

const FILE_NAME: &str = "window-state.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct WindowState {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    maximized: bool,
}

fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

fn load(data_dir: &Path) -> Option<WindowState> {
    let text = std::fs::read_to_string(state_path(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Restore the last saved window position and size.
pub fn restore(window: &WebviewWindow, data_dir: &Path) {
    let Some(state) = load(data_dir) else {
        return;
    };
    if state.maximized {
        let _ = window.maximize();
        return;
    }
    if state.width == 0 || state.height == 0 {
        return;
    }

    // Only restore the position if it is still on an available monitor
    // (e.g. the monitor it was on has been unplugged).
    let position_visible = window
        .available_monitors()
        .map(|monitors| {
            monitors.iter().any(|m| {
                let p = m.position();
                let s = m.size();
                state.x >= p.x
                    && state.y >= p.y
                    && state.x < p.x + s.width as i32
                    && state.y < p.y + s.height as i32
            })
        })
        .unwrap_or(true);

    if position_visible {
        let _ = window.set_position(PhysicalPosition::new(state.x, state.y));
    }
    let _ = window.set_size(PhysicalSize::new(state.width, state.height));
}

/// Persist the current window position and size.
pub fn save(window: &WebviewWindow, data_dir: &Path) {
    let maximized = window.is_maximized().unwrap_or(false);

    // Keep the last non-maximized bounds so un-maximizing restores them.
    let mut state = load(data_dir).unwrap_or_default();
    state.maximized = maximized;
    if !maximized {
        if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
            state.x = pos.x;
            state.y = pos.y;
            state.width = size.width;
            state.height = size.height;
        }
    }

    if let Ok(json) = serde_json::to_string(&state) {
        let _ = std::fs::write(state_path(data_dir), json);
    }
}

/// Save state when the window is about to close.
pub fn register_close_handler(window: &WebviewWindow, data_dir: &Path) {
    let handle = window.clone();
    let data_dir = data_dir.to_path_buf();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { .. } = event {
            save(&handle, &data_dir);
        }
    });
}
