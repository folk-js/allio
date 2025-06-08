// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    panic::{self},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
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
mod websocket;

use accessibility::{
    get_ui_tree_by_pid, is_listening_for_events, start_accessibility_events,
    stop_accessibility_events,
};
use websocket::WebSocketState;

// Constants - optimized polling rate
const POLLING_INTERVAL_MS: u64 = 8; // ~120 FPS - fast enough for smooth tracking

// App modes
#[derive(Debug, Clone, Copy, PartialEq)]
enum OverlayMode {
    Main = 0,  // Coordinate card
    Debug = 1, // Full debug interface
    Sand = 2,  // Sand simulation
}

impl Default for OverlayMode {
    fn default() -> Self {
        OverlayMode::Main
    }
}

impl OverlayMode {
    fn next(self) -> Self {
        match self {
            OverlayMode::Main => OverlayMode::Debug,
            OverlayMode::Debug => OverlayMode::Sand,
            OverlayMode::Sand => OverlayMode::Main,
        }
    }

    fn url(self) -> &'static str {
        match self {
            OverlayMode::Main => "http://localhost:1420/index.html",
            OverlayMode::Debug => "http://localhost:1420/debug.html",
            OverlayMode::Sand => "http://localhost:1420/src-web/sand.html",
        }
    }
}

// App State
#[derive(Default)]
struct AppState {
    clickthrough_enabled: AtomicBool,
    current_mode: Mutex<OverlayMode>,
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

// Simple function to cycle through modes
fn cycle_overlay_mode(app: &tauri::AppHandle) -> Result<OverlayMode, Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let mut current_mode = state.current_mode.lock().unwrap();
    *current_mode = current_mode.next();
    let new_mode = *current_mode;
    drop(current_mode); // Release lock early

    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    window.navigate(new_mode.url().parse().unwrap())?;
    Ok(new_mode)
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::default())
        .setup(|app| {
            // Initialize WebSocket state
            let ws_state = WebSocketState::new();

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
                                    let _ = cycle_overlay_mode(&app_handle);
                                }
                            }
                        })
                        .build(),
                )?;

                app.global_shortcut().register(toggle_shortcut)?;
                app.global_shortcut().register(page_toggle_shortcut)?;
            }

            // Start the window polling thread first to populate initial windows
            let app_handle = app.handle().clone();
            let ws_state_for_polling = ws_state.clone();
            let ws_state_for_server = ws_state.clone();

            thread::spawn(move || {
                // Do an initial window poll to populate the state
                let current_windows = get_all_windows_with_focus();
                println!(
                    "🔍 Initial window poll found {} windows",
                    current_windows.len()
                );

                // Update WebSocket state with initial windows
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(async move {
                    ws_state_for_polling.update_windows(&current_windows).await;
                    println!(
                        "📦 WebSocket state initialized with {} windows",
                        current_windows.len()
                    );

                    // Now start the WebSocket server
                    println!("🚀 Starting WebSocket server...");
                    tokio::spawn(async move {
                        websocket::start_websocket_server(ws_state_for_server).await;
                    });

                    // Continue with the polling loop
                    window_polling_loop(app_handle, ws_state_for_polling);
                });
            });

            // Accessibility system is now command-based only

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            toggle_clickthrough,
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

// Modified polling loop to broadcast to WebSocket clients
fn window_polling_loop(app_handle: tauri::AppHandle, ws_state: WebSocketState) {
    let mut last_windows: Option<Vec<WindowInfo>> = None;

    loop {
        let loop_start = Instant::now();

        // Get fresh data from system
        let current_windows = get_all_windows_with_focus();

        // Check if data changed
        let windows_changed = last_windows.as_ref() != Some(&current_windows);

        // Only emit if something actually changed
        if windows_changed {
            // Update WebSocket state with current windows for matching
            let rt = tokio::runtime::Handle::try_current();
            if let Ok(handle) = rt {
                let ws_state_clone = ws_state.clone();
                let windows_clone = current_windows.clone();
                let app_handle_clone = app_handle.clone();
                handle.spawn(async move {
                    ws_state_clone.update_windows(&windows_clone).await;

                    // Create enhanced windows with client information
                    let enhanced_windows =
                        create_enhanced_windows(&windows_clone, &ws_state_clone).await;
                    let enhanced_payload = EnhancedWindowUpdatePayload {
                        windows: enhanced_windows,
                    };

                    // Emit enhanced payload to Tauri frontend
                    if let Err(_) =
                        app_handle_clone.emit("enhanced-window-update", &enhanced_payload)
                    {
                        // Silently ignore errors
                    }
                });
            }

            let payload = WindowUpdatePayload {
                windows: current_windows.clone(),
            };

            // NEW: Also broadcast to WebSocket clients
            ws_state.broadcast(&payload);

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

// Create enhanced windows with client information
async fn create_enhanced_windows(
    windows: &[WindowInfo],
    ws_state: &WebSocketState,
) -> Vec<EnhancedWindowInfo> {
    let clients = ws_state.clients.read().await;

    windows
        .iter()
        .map(|window| {
            // Check if this window has a connected client
            let client_id = if clients.contains_key(&window.id) {
                Some(window.id.clone()) // Use window ID as the client identifier
            } else {
                None
            };

            EnhancedWindowInfo {
                window: window.clone(),
                client_id,
            }
        })
        .collect()
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

#[derive(Debug, Serialize, Deserialize, Clone)]
struct EnhancedWindowUpdatePayload {
    windows: Vec<EnhancedWindowInfo>,
}

// Enhanced window info with client ID for display
#[derive(Debug, Serialize, Deserialize, Clone)]
struct EnhancedWindowInfo {
    #[serde(flatten)]
    window: WindowInfo,
    client_id: Option<String>, // Full client UUID
}
