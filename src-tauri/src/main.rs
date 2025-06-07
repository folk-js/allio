// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    panic::{self},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

mod accessibility;
use accessibility::{
    get_ui_tree_by_pid, is_listening_for_events, start_accessibility_events,
    stop_accessibility_events,
};

// Constants - optimized polling rate
const POLLING_INTERVAL_MS: u64 = 8; // ~120 FPS - fast enough for smooth tracking
const OVERLAY_MAIN_URL: &str = "http://localhost:1420/index.html";
const OVERLAY_SAND_URL: &str = "http://localhost:1420/src-web/sand.html";

// App State
#[derive(Default)]
struct AppState {
    clickthrough_enabled: AtomicBool,
    is_sand_mode: AtomicBool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct WindowInfo {
    id: String,
    name: String,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    focused: bool,
    process_id: u32,
}

// Convert x-win WindowInfo to our WindowInfo struct
impl WindowInfo {
    fn from_x_win(window: &x_win::WindowInfo, focused: bool) -> Self {
        WindowInfo {
            id: window.id.to_string(),
            name: window.title.clone(),
            x: window.position.x,
            y: window.position.y,
            w: window.position.width,
            h: window.position.height,
            focused,
            process_id: window.info.process_id,
        }
    }
}

impl WindowInfo {
    fn with_offset(mut self, offset_x: i32, offset_y: i32) -> Self {
        self.x -= offset_x;
        self.y -= offset_y;
        self
    }
}

#[tauri::command]
fn toggle_clickthrough(
    state: tauri::State<AppState>,
    app: tauri::AppHandle,
) -> Result<bool, String> {
    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let current_ignore = state.clickthrough_enabled.load(Ordering::Relaxed);
    let new_ignore = !current_ignore;

    window
        .set_ignore_cursor_events(new_ignore)
        .map_err(|e| e.to_string())?;

    state
        .clickthrough_enabled
        .store(new_ignore, Ordering::Relaxed);
    Ok(new_ignore)
}

// Accessibility commands are now in accessibility.rs

#[cfg(target_os = "macos")]
fn get_main_screen_dimensions() -> (f64, f64) {
    unsafe {
        let display_id: CGDirectDisplayID = CGMainDisplayID();
        let width = CGDisplayPixelsWide(display_id) as f64;
        let height = CGDisplayPixelsHigh(display_id) as f64;
        (width, height)
    }
}

#[tauri::command]
fn toggle_page_mode(state: tauri::State<AppState>, app: tauri::AppHandle) -> Result<bool, String> {
    let current_mode = state.is_sand_mode.load(Ordering::Relaxed);
    let new_mode = !current_mode;
    state.is_sand_mode.store(new_mode, Ordering::Relaxed);

    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let url = if new_mode {
        OVERLAY_SAND_URL
    } else {
        OVERLAY_MAIN_URL
    };

    window
        .navigate(url.parse().unwrap())
        .map_err(|e| e.to_string())?;
    Ok(new_mode)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .setup(|app| {
            let (screen_width, screen_height) = get_main_screen_dimensions();
            if let Some(window) = app.get_webview_window("main") {
                window
                    .set_size(tauri::LogicalSize::new(screen_width, screen_height))
                    .ok();
                window
                    .set_position(tauri::LogicalPosition::new(0.0, 0.0))
                    .ok();
                window.set_ignore_cursor_events(false).ok();
                window.show().ok();
            }

            // Set up global shortcut with proper state handling
            #[cfg(desktop)]
            {
                let toggle_shortcut =
                    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyE);

                // New shortcut for page switching
                let page_toggle_shortcut =
                    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyP);

                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |app_handle, shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                if shortcut == &toggle_shortcut {
                                    let _ = toggle_clickthrough_rust(app_handle.clone());
                                } else if shortcut == &page_toggle_shortcut {
                                    let _ = toggle_page_mode_rust(app_handle.clone());
                                }
                            }
                        })
                        .build(),
                )?;

                app.global_shortcut().register(toggle_shortcut)?;
                app.global_shortcut().register(page_toggle_shortcut)?;
            }

            // Start the window polling thread (fast, no accessibility calls)
            let app_handle = app.handle().clone();
            thread::spawn(move || {
                window_polling_loop(app_handle);
            });

            // Accessibility system is now command-based only

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            toggle_clickthrough,
            toggle_page_mode,
            get_ui_tree_by_pid,
            start_accessibility_events,
            stop_accessibility_events,
            is_listening_for_events,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Add this function to handle the toggle from Rust (for shortcuts)
fn toggle_clickthrough_rust(app: tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let current_ignore = state.clickthrough_enabled.load(Ordering::Relaxed);
    let new_ignore = !current_ignore;

    window.set_ignore_cursor_events(new_ignore)?;
    state
        .clickthrough_enabled
        .store(new_ignore, Ordering::Relaxed);

    Ok(())
}

// Add this function to handle page mode toggle from Rust (for shortcuts)
fn toggle_page_mode_rust(app: tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let current_mode = state.is_sand_mode.load(Ordering::Relaxed);
    let new_mode = !current_mode;
    state.is_sand_mode.store(new_mode, Ordering::Relaxed);

    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let url = if new_mode {
        OVERLAY_SAND_URL
    } else {
        OVERLAY_MAIN_URL
    };

    window.navigate(url.parse().unwrap())?;
    Ok(())
}

// Simple polling loop - poll and emit immediately when data changes
fn window_polling_loop(app_handle: tauri::AppHandle) {
    let mut last_windows: Option<Vec<WindowInfo>> = None;

    loop {
        let loop_start = Instant::now();

        // Get fresh data from system
        let current_windows = get_all_windows_with_focus();

        // Check if data changed
        let windows_changed = last_windows.as_ref() != Some(&current_windows);

        // Only emit if something actually changed
        if windows_changed {
            let payload = WindowUpdatePayload {
                windows: current_windows.clone(),
            };

            if let Err(_) = app_handle.emit("window-update", &payload) {
                // Silently ignore errors
            }

            last_windows = Some(current_windows);
        }

        // Precise interval handling - sleep for remaining time, or skip if behind
        let elapsed = loop_start.elapsed();
        let target_interval = Duration::from_millis(POLLING_INTERVAL_MS);
        if elapsed < target_interval {
            thread::sleep(target_interval - elapsed);
        }
    }
}

// Combined function to get all windows with focused state in single call
fn get_all_windows_with_focus() -> Vec<WindowInfo> {
    let current_pid = std::process::id();

    // Get all windows and active window in parallel
    let all_windows_result = panic::catch_unwind(|| x_win::get_open_windows());
    let active_window_result = panic::catch_unwind(|| x_win::get_active_window());

    let (all_windows, active_window_id) = match (all_windows_result, active_window_result) {
        (Ok(Ok(windows)), Ok(Ok(active))) => (windows, Some(active.id)),
        (Ok(Ok(windows)), _) => (windows, None),
        _ => return Vec::new(),
    };

    // Find overlay offset
    let mut overlay_offset = (0, 0);
    for window in &all_windows {
        if window.info.process_id == current_pid {
            overlay_offset = (window.position.x, window.position.y);
            break;
        }
    }

    // Convert all windows, excluding our overlay, and mark focused
    all_windows
        .iter()
        .filter(|w| w.info.process_id != current_pid)
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            WindowInfo::from_x_win(w, focused).with_offset(overlay_offset.0, overlay_offset.1)
        })
        .collect()
}

// Event payload structures
#[derive(Debug, Serialize, Deserialize, Clone)]
struct WindowUpdatePayload {
    windows: Vec<WindowInfo>,
}
