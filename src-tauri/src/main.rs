// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    panic::{self},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
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

// Dynamic overlay handling
fn get_overlay_files() -> Vec<String> {
    let exe_path = std::env::current_exe().unwrap();
    let exe_dir = exe_path.parent().unwrap();

    // In development: src-tauri/target/debug -> go up 3 levels to project root
    // In production: executable location varies
    let project_root = if exe_dir.ends_with("debug") || exe_dir.ends_with("release") {
        exe_dir
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
    } else {
        exe_dir
    };

    let overlays_path = project_root.join("src-web").join("overlays");
    let mut overlays = Vec::new();

    if overlays_path.exists() {
        if let Ok(entries) = fs::read_dir(&overlays_path) {
            for entry in entries.flatten() {
                if let Some(file_name) = entry.file_name().to_str() {
                    if file_name.ends_with(".html") {
                        overlays.push(file_name.to_string());
                    }
                }
            }
        }
    }

    overlays.sort();
    overlays
}

fn get_overlay_url(filename: &str) -> String {
    format!("http://localhost:1420/src-web/overlays/{}", filename)
}

// App State
#[derive(Default)]
struct AppState {
    clickthrough_enabled: AtomicBool,
    current_overlay: Mutex<String>,
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

// Function to switch to a specific overlay
fn switch_overlay(
    app: &tauri::AppHandle,
    filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let mut current_overlay = state.current_overlay.lock().unwrap();
    *current_overlay = filename.to_string();
    drop(current_overlay);

    let window = app
        .get_webview_window("main")
        .ok_or("Main window not found")?;

    let url = get_overlay_url(filename);
    window.navigate(url.parse().unwrap())?;
    Ok(())
}

// Build tray menu with overlay options
fn build_overlay_tray(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let overlay_files = get_overlay_files();
    let mut menu_builder = MenuBuilder::new(app);

    // Add overlay options
    if overlay_files.is_empty() {
        let no_overlays_item = MenuItemBuilder::new("No overlays found")
            .id("no_overlays")
            .enabled(false)
            .build(app)?;
        menu_builder = menu_builder.item(&no_overlays_item);
    } else {
        for filename in overlay_files {
            let menu_item = MenuItemBuilder::new(&filename).id(&filename).build(app)?;
            menu_builder = menu_builder.item(&menu_item);
        }
    }

    // Add separator
    let separator = PredefinedMenuItem::separator(app)?;
    menu_builder = menu_builder.item(&separator);

    // Add toggle clickthrough option
    let state = app.state::<AppState>();
    let clickthrough_enabled = state.clickthrough_enabled.load(Ordering::Relaxed);
    let clickthrough_text = if clickthrough_enabled {
        "🔓 Disable Clickthrough"
    } else {
        "🔒 Enable Clickthrough"
    };

    let toggle_clickthrough_item = MenuItemBuilder::new(clickthrough_text)
        .id("toggle_clickthrough")
        .build(app)?;
    menu_builder = menu_builder.item(&toggle_clickthrough_item);

    let menu = menu_builder.build()?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .icon(app.default_window_icon().unwrap().clone())
        .on_menu_event(move |app_handle, event| {
            let event_id = event.id().0.clone();
            match event_id.as_str() {
                "toggle_clickthrough" => {
                    let _ = toggle_clickthrough_rust(app_handle.clone());
                    // Note: Menu text will update on next app restart
                }
                "no_overlays" => {
                    // Do nothing for disabled item
                }
                _ => {
                    // Handle overlay selection
                    let _ = switch_overlay(&app_handle, &event_id);
                }
            }
        })
        .build(app)?;

    Ok(())
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

                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |app_handle, shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                if shortcut == &toggle_shortcut {
                                    let _ = toggle_clickthrough_rust(app_handle.clone());
                                }
                            }
                        })
                        .build(),
                )?;

                app.global_shortcut().register(toggle_shortcut)?;
            }

            // Build overlay tray
            build_overlay_tray(&app.handle())?;

            // Load the first overlay if any exist
            let overlay_files = get_overlay_files();
            if let Some(first_overlay) = overlay_files.first() {
                let _ = switch_overlay(&app.handle(), first_overlay);
            }

            // Start the window polling thread first to populate initial windows
            let app_handle = app.handle().clone();
            let ws_state_for_polling = ws_state.clone();
            let ws_state_for_server = ws_state.clone();

            thread::spawn(move || {
                // Do an initial window poll to populate the state
                let current_windows = get_all_windows_with_focus();

                // Update WebSocket state with initial windows
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(async move {
                    ws_state_for_polling.update_windows(&current_windows).await;

                    // Start the WebSocket server
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
    let connected_windows = ws_state.connected_windows.read().await;

    windows
        .iter()
        .map(|window| {
            // Check if this window has a connected client
            let client_id = if connected_windows.contains(&window.id) {
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

// Enhanced window info with client connection status
#[derive(Debug, Serialize, Deserialize, Clone)]
struct EnhancedWindowInfo {
    #[serde(flatten)]
    window: WindowInfo,
    client_id: Option<String>, // Window ID if client is connected
}
