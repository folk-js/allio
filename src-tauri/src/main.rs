// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::panic;

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

// Gets the currently active window
#[tauri::command]
fn get_active_window_info() -> Result<Option<WindowInfo>, String> {
    match safe_get_active_window() {
        Ok(Some(active_window)) => {
            let window_info = convert_window_info(&active_window);
            Ok(Some(window_info))
        }
        Ok(None) => Ok(None),
        Err(e) => {
            eprintln!("{}", e);

            // Check if it's likely a permission error
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

// Gets all open windows using x-win
#[tauri::command]
fn get_all_windows() -> Result<Vec<WindowInfo>, String> {
    match safe_get_open_windows() {
        Ok(windows) => {
            let window_infos: Vec<WindowInfo> = windows.iter().map(convert_window_info).collect();
            Ok(window_infos)
        }
        Err(e) => {
            eprintln!("{}", e);

            // Check if it's likely a permission error
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            greet,
            get_all_windows,
            get_active_window_info,
            success
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
