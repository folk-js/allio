// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::panic;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Manager;

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WindowInfo {
    id: String,
    name: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

// Convert x-win WindowInfo to our WindowInfo struct
fn convert_window_info_with_offset(
    window: &x_win::WindowInfo,
    offset_x: i32,
    offset_y: i32,
) -> WindowInfo {
    WindowInfo {
        id: window.id.to_string(),
        name: window.title.clone(),
        x: window.position.x - offset_x,
        y: window.position.y - offset_y,
        w: window.position.width,
        h: window.position.height,
    }
}

// Helper for permission error handling
fn handle_x_win_error(e: String) -> String {
    if e.contains("nil")
        || e.contains("NSRunningApplication")
        || e.contains("accessibility permissions")
    {
        "Permission denied. Please grant accessibility permissions in System Preferences → Security & Privacy → Privacy → Accessibility".to_string()
    } else {
        e
    }
}

// Get the current process PID
fn get_current_pid() -> u32 {
    std::process::id()
}

// Gets all open windows and extracts overlay offset in one pass
#[tauri::command]
fn get_all_windows(_app: tauri::AppHandle) -> Result<Vec<WindowInfo>, String> {
    let current_pid = get_current_pid();

    let result = panic::catch_unwind(|| x_win::get_open_windows());
    let windows = match result {
        Ok(Ok(windows)) => windows,
        Ok(Err(e)) => return Err(handle_x_win_error(format!("Failed to get windows: {}", e))),
        Err(_) => return Err(handle_x_win_error("Panic occurred while getting windows. This usually indicates accessibility permissions are needed.".to_string())),
    };

    // Find our overlay window to get the offset
    let mut overlay_offset_x = 0;
    let mut overlay_offset_y = 0;

    for window in &windows {
        if window.info.process_id == current_pid {
            overlay_offset_x = window.position.x;
            overlay_offset_y = window.position.y;
            break;
        }
    }

    // Apply offset to all windows
    let window_infos: Vec<WindowInfo> = windows
        .iter()
        .map(|w| convert_window_info_with_offset(w, overlay_offset_x, overlay_offset_y))
        .collect();
    Ok(window_infos)
}

#[tauri::command]
fn get_active_window_info(_app: tauri::AppHandle) -> Result<Option<WindowInfo>, String> {
    let current_pid = get_current_pid();

    // Get overlay offset from all windows first
    let overlay_offset = {
        let result = panic::catch_unwind(|| x_win::get_open_windows());
        match result {
            Ok(Ok(windows)) => {
                let mut offset = (0, 0);
                for window in &windows {
                    if window.info.process_id == current_pid {
                        offset = (window.position.x, window.position.y);
                        break;
                    }
                }
                offset
            }
            Ok(Err(_)) => (0, 0),
            Err(_) => (0, 0),
        }
    };

    let result = panic::catch_unwind(|| x_win::get_active_window());
    match result {
        Ok(Ok(active_window)) => {
            let window_info = convert_window_info_with_offset(&active_window, overlay_offset.0, overlay_offset.1);
            Ok(Some(window_info))
        }
        Ok(Err(e)) => Err(handle_x_win_error(format!("Failed to get active window: {}", e))),
        Err(_) => Err(handle_x_win_error("Panic occurred while getting active window. This usually indicates accessibility permissions are needed.".to_string())),
    }
}

static CLICKTHROUGH_ENABLED: AtomicBool = AtomicBool::new(true);

#[tauri::command]
fn toggle_clickthrough(app: tauri::AppHandle) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window("main") {
        let current_ignore = CLICKTHROUGH_ENABLED.load(Ordering::Relaxed);
        let new_ignore = !current_ignore;
        window
            .set_ignore_cursor_events(new_ignore)
            .map_err(|e| e.to_string())?;
        CLICKTHROUGH_ENABLED.store(new_ignore, Ordering::Relaxed);
        Ok(new_ignore)
    } else {
        Err("Main window not found".to_string())
    }
}

#[cfg(target_os = "macos")]
fn get_main_screen_dimensions() -> (f64, f64) {
    unsafe {
        let display_id: CGDirectDisplayID = CGMainDisplayID();
        let width = CGDisplayPixelsWide(display_id) as f64;
        let height = CGDisplayPixelsHigh(display_id) as f64;
        (width, height)
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let (screen_width, screen_height) = get_main_screen_dimensions();
            if let Some(window) = app.get_webview_window("main") {
                window
                    .set_size(tauri::LogicalSize::new(screen_width, screen_height))
                    .ok();
                window
                    .set_position(tauri::LogicalPosition::new(0.0, 0.0))
                    .ok();
                window.set_ignore_cursor_events(true).ok();
                window.show().ok();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_all_windows,
            get_active_window_info,
            toggle_clickthrough,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
