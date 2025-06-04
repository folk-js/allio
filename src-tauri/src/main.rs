// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use x_win::{get_active_window, get_open_windows};

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

// Convert x-win WindowInfo to our WindowInfo struct
fn convert_window_info(window: &x_win::WindowInfo) -> WindowInfo {
    WindowInfo {
        id: window.id.to_string(),
        name: window.title.clone(),
        x: window.position.x as f64,
        y: window.position.y as f64 - 38.0,
        w: window.position.width as f64,
        h: window.position.height as f64,
    }
}

// Gets the currently active window
#[tauri::command]
fn get_active_window_info() -> Result<Option<WindowInfo>, String> {
    match get_active_window() {
        Ok(active_window) => {
            let window_info = convert_window_info(&active_window);
            Ok(Some(window_info))
        }
        Err(e) => Err(format!("Failed to get active window: {}", e)),
    }
}

// Gets all open windows using x-win
#[tauri::command]
fn get_all_windows() -> Result<Vec<WindowInfo>, String> {
    match get_open_windows() {
        Ok(windows) => {
            let window_infos: Vec<WindowInfo> = windows.iter().map(convert_window_info).collect();
            Ok(window_infos)
        }
        Err(e) => Err(format!("Failed to get windows: {}", e)),
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
            get_all_windows,
            get_active_window_info,
            success
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
