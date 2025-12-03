// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
};
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

mod mouse;

use axio::windows::{get_main_screen_dimensions, PollingConfig, WindowEnumOptions};
use axio_ws::WebSocketState;

// Dynamic overlay handling
fn get_overlay_files() -> Vec<String> {
    let exe_path = std::env::current_exe().expect("Failed to get current executable path");
    let exe_dir = exe_path
        .parent()
        .expect("Executable path has no parent directory");

    // In development: target/debug -> go up 2 levels to project root (workspace layout)
    // In production: executable location varies
    let project_root = if exe_dir.ends_with("debug") || exe_dir.ends_with("release") {
        exe_dir
            .parent() // target/
            .and_then(|p| p.parent()) // project root
            .expect("Failed to find project root from debug/release directory")
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

// Helper function to get main window
fn get_main_window(
    app: &tauri::AppHandle,
) -> Result<tauri::WebviewWindow, Box<dyn std::error::Error>> {
    app.get_webview_window("main")
        .ok_or("Main window not found".into())
}

// App State
#[derive(Default)]
struct AppState {
    clickthrough_enabled: AtomicBool,
    current_overlay: Mutex<String>,
}

// Consolidated clickthrough toggle logic
fn toggle_clickthrough_internal(
    app: &tauri::AppHandle,
) -> Result<bool, Box<dyn std::error::Error>> {
    let state = app.state::<AppState>();
    let window = get_main_window(app)?;

    let current_ignore = state.clickthrough_enabled.load(Ordering::Relaxed);
    let new_ignore = !current_ignore;

    window.set_ignore_cursor_events(new_ignore)?;
    state
        .clickthrough_enabled
        .store(new_ignore, Ordering::Relaxed);

    Ok(new_ignore)
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

    let window = get_main_window(app)?;
    let url = get_overlay_url(filename);
    window.navigate(url.parse().expect(&format!("Invalid overlay URL: {}", url)))?;
    Ok(())
}

// Build tray menu with overlay options
fn build_overlay_tray(
    app: &tauri::AppHandle,
    overlay_files: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
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
            let menu_item = MenuItemBuilder::new(filename).id(filename).build(app)?;
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
                    let _ = toggle_clickthrough_internal(&app_handle);
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
            // Create broadcast channel for element updates
            let (sender, _) = tokio::sync::broadcast::channel(1000);
            let sender = std::sync::Arc::new(sender);

            // Create clickthrough callback that captures the app handle
            let app_handle = app.handle().clone();
            let clickthrough_callback: axio_ws::ClickthroughCallback =
                std::sync::Arc::new(move |enabled| {
                    use tauri::Manager;
                    match app_handle.get_webview_window("main") {
                        Some(window) => window
                            .set_ignore_cursor_events(enabled)
                            .map_err(|e| e.to_string()),
                        None => Err("Main window not found".to_string()),
                    }
                });

            // Initialize WebSocket state with clickthrough support
            let ws_state =
                WebSocketState::new(sender.clone()).with_clickthrough(clickthrough_callback);

            // Initialize ElementRegistry with broadcast sender (for element update events)
            axio::element_registry::ElementRegistry::initialize(sender.clone());

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
                                    let _ = toggle_clickthrough_internal(&app_handle);
                                }
                            }
                        })
                        .build(),
                )?;

                app.global_shortcut().register(toggle_shortcut)?;
            }

            // Get overlay files once and reuse
            let overlay_files = get_overlay_files();

            // Build overlay tray
            build_overlay_tray(&app.handle(), &overlay_files)?;

            // Load the first overlay if any exist
            if let Some(first_overlay) = overlay_files.first() {
                let _ = switch_overlay(&app.handle(), first_overlay);
            }

            // Start global mouse tracking (for automatic clickthrough)
            mouse::start_mouse_tracking(ws_state.clone());

            // Start window polling (broadcasts to WebSocket via sender)
            let current_pid = std::process::id();
            let ws_state_for_polling = ws_state.clone();
            axio::start_polling(PollingConfig {
                enum_options: WindowEnumOptions {
                    exclude_pid: Some(current_pid),
                    filter_fullscreen: true,
                    filter_offscreen: true,
                },
                broadcast_sender: Some(sender.clone()),
                // Update WebSocketState's cached windows for initial client connections
                on_windows_changed: Some(Box::new(move |windows, _, _| {
                    ws_state_for_polling.update_windows(windows);
                })),
                ..Default::default()
            });

            // Start WebSocket server
            let ws_state_clone = ws_state.clone();
            thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
                rt.block_on(async move {
                    axio_ws::start_ws_server(ws_state_clone).await;
                });
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
