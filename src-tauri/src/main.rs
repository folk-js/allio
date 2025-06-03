// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};

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

// Gets the currently active window (similar to your Python implementation)
#[tauri::command]
fn get_active_window() -> Result<Option<WindowInfo>, String> {
    match active_win_pos_rs::get_active_window() {
        Ok(active_window) => {
            let window_info = WindowInfo {
                id: format!("{:?}", active_window.window_id),
                name: active_window.title,
                x: active_window.position.x,
                y: active_window.position.y,
                w: active_window.position.width,
                h: active_window.position.height,
            };
            Ok(Some(window_info))
        }
        Err(_) => Ok(None),
    }
}

// Gets all windows - for now returns just active window
// This matches the "/windows" endpoint from your Flask server
#[tauri::command]
fn get_windows() -> Result<Vec<WindowInfo>, String> {
    match get_active_window() {
        Ok(Some(window)) => Ok(vec![window]),
        Ok(None) => Ok(vec![]),
        Err(e) => Err(e),
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            greet,
            get_windows,
            get_active_window,
            success
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
