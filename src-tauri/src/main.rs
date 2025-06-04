// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::panic;
use tauri::Manager;

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WindowInfo {
    id: String,
    name: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiResponse {
    status: String,
    message: String,
}

// Safe wrapper around x-win calls to catch panics
fn safe_get_active_window() -> Result<Option<x_win::WindowInfo>, String> {
    let result = panic::catch_unwind(|| x_win::get_active_window());

    match result {
        Ok(Ok(window)) => Ok(Some(window)),
        Ok(Err(e)) => Err(format!("Failed to get active window: {}", e)),
        Err(_) => Err("Panic occurred while getting active window. This usually indicates accessibility permissions are needed.".to_string()),
    }
}

// Safe wrapper around x-win calls to catch panics
fn safe_get_open_windows() -> Result<Vec<x_win::WindowInfo>, String> {
    let result = panic::catch_unwind(|| x_win::get_open_windows());

    match result {
        Ok(Ok(windows)) => Ok(windows),
        Ok(Err(e)) => Err(format!("Failed to get windows: {}", e)),
        Err(_) => Err("Panic occurred while getting windows. This usually indicates accessibility permissions are needed.".to_string()),
    }
}

// Update convert_window_info to accept an offset
fn convert_window_info_with_offset(
    window: &x_win::WindowInfo,
    offset_x: f64,
    offset_y: f64,
) -> WindowInfo {
    WindowInfo {
        id: window.id.to_string(),
        name: window.title.clone(),
        x: window.position.x as f64 - offset_x,
        y: window.position.y as f64 - offset_y,
        w: window.position.width as f64,
        h: window.position.height as f64,
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

    match safe_get_open_windows() {
        Ok(windows) => {
            // First pass: find our overlay window to get the offset
            let mut overlay_offset_x = 0.0;
            let mut overlay_offset_y = 0.0;

            for window in &windows {
                if window.info.process_id == current_pid {
                    overlay_offset_x = window.position.x as f64;
                    overlay_offset_y = window.position.y as f64;
                    println!("Overlay x: {}", overlay_offset_x);
                    break;
                }
            }

            // Second pass: apply offset to all windows
            let window_infos: Vec<WindowInfo> = windows
                .iter()
                .map(|w| convert_window_info_with_offset(w, overlay_offset_x, overlay_offset_y))
                .collect();
            Ok(window_infos)
        }
        Err(e) => {
            eprintln!("{}", e);

            if e.contains("nil")
                || e.contains("NSRunningApplication")
                || e.contains("accessibility permissions")
            {
                Err("Permission denied. Please grant accessibility permissions in System Preferences → Security & Privacy → Privacy → Accessibility".to_string())
            } else {
                Err(e)
            }
        }
    }
}

#[tauri::command]
fn get_active_window_info(_app: tauri::AppHandle) -> Result<Option<WindowInfo>, String> {
    let current_pid = get_current_pid();

    // Get overlay offset from all windows first
    let overlay_offset = match safe_get_open_windows() {
        Ok(windows) => {
            let mut offset = (0.0, 0.0);
            for window in &windows {
                if window.info.process_id == current_pid {
                    println!("Overlay x: {}", window.position.x);
                    offset = (window.position.x as f64, window.position.y as f64);
                    break;
                }
            }
            offset
        }
        Err(_) => (0.0, 0.0),
    };

    match safe_get_active_window() {
        Ok(Some(active_window)) => {
            let window_info =
                convert_window_info_with_offset(&active_window, overlay_offset.0, overlay_offset.1);
            Ok(Some(window_info))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            eprintln!("{}", e);

            if e.contains("nil")
                || e.contains("NSRunningApplication")
                || e.contains("accessibility permissions")
            {
                Err("Permission denied. Please grant accessibility permissions in System Preferences → Security & Privacy → Privacy → Accessibility".to_string())
            } else {
                Err(e)
            }
        }
    }
}

// Success response (matching your Python API)
#[tauri::command]
fn success() -> ApiResponse {
    ApiResponse {
        status: "success".to_string(),
        message: "we did it".to_string(),
    }
}

// Keep the original greet function for compatibility
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
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
            println!(
                "Detected screen dimensions: {}x{}",
                screen_width, screen_height
            );

            if let Some(window) = app.get_webview_window("main") {
                // Set the window to cover the entire screen at position (0,0)
                let _ = window.set_size(tauri::LogicalSize::new(screen_width, screen_height));
                let _ = window.set_position(tauri::LogicalPosition::new(0.0, 0.0));
                let _ = window.set_ignore_cursor_events(true);
                let _ = window.show();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            get_all_windows,
            get_active_window_info,
            success
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
