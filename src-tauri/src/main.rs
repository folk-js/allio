// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
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

// macOS NSPanel support for non-focus-stealing overlay
#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, ManagerExt as _, PanelLevel, StyleMask, WebviewWindowExt as _};

mod mouse;

// Define panel class for macOS - this creates a non-activating floating panel
#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(AxioPanel {
        config: {
            // Panel can become key window (receive keyboard input when needed)
            can_become_key_window: true,
            // This is a floating panel (stays above regular windows)
            is_floating_panel: true
        }
    })
}

use axio::windows::{get_main_screen_dimensions, PollingConfig, WindowEnumOptions};
use axio_ws::WebSocketState;

// Check if running in dev mode
fn is_dev_mode() -> bool {
    // In dev mode, tauri runs with the dev server
    // Check if running from target/debug or target/release
    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new(""));
    exe_dir.ends_with("debug") || exe_dir.ends_with("release")
}

// Get overlay files - hardcoded list since we know what we build
fn get_overlay_files() -> Vec<String> {
    // These are the HTML files in src-web/overlays that get built to dist/
    vec![
        "axtrees.html".to_string(),
        "identifiers.html".to_string(),
        "ports.html".to_string(),
        "sand.html".to_string(),
        "windows-debug.html".to_string(),
    ]
}

fn get_overlay_url(filename: &str) -> String {
    if is_dev_mode() {
        // Dev mode: use vite dev server with full path
        format!("http://localhost:1420/src-web/overlays/{}", filename)
    } else {
        // Production: use tauri asset protocol
        format!("tauri://localhost/{}", filename)
    }
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
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_shell::init());

    // Initialize NSPanel plugin on macOS for non-focus-stealing overlay
    #[cfg(target_os = "macos")]
    {
        builder = builder.plugin(tauri_nspanel::init());
    }

    builder
        .manage(AppState::default())
        .setup(|app| {
            // Create broadcast channel for WebSocket events
            let (sender, _) = tokio::sync::broadcast::channel(1000);
            let sender = std::sync::Arc::new(sender);

            // Create custom RPC handler for app-specific methods (clickthrough)
            let app_handle = app.handle().clone();
            let custom_handler: axio_ws::CustomRpcHandler =
                std::sync::Arc::new(move |method, args| {
                    // Support both names: "set_passthrough" (preferred) and "set_clickthrough" (deprecated)
                    if method == "set_passthrough" || method == "set_clickthrough" {
                        let enabled = args["enabled"].as_bool().unwrap_or(false);

                        // On macOS, use the panel API for better control
                        // IMPORTANT: Panel operations MUST run on the main thread!
                        #[cfg(target_os = "macos")]
                        {
                            let handle = app_handle.clone();
                            let handle_inner = handle.clone();
                            let result = handle.run_on_main_thread(move || {
                                if let Ok(panel) = handle_inner.get_webview_panel("main") {
                                    panel.set_ignores_mouse_events(enabled);
                                    if enabled {
                                        // Passing through: resign key window so the underlying
                                        // app becomes key again (seamless transition back)
                                        panel.resign_key_window();
                                    } else {
                                        // Capturing: make panel key window so it receives
                                        // pointer events (works because we're non-activating)
                                        panel.make_key_window();
                                    }
                                }
                            });
                            return Some(match result {
                                Ok(()) => serde_json::json!({ "result": { "enabled": enabled } }),
                                Err(e) => serde_json::json!({ "error": e.to_string() }),
                            });
                        }

                        // On other platforms, use the window API
                        #[cfg(not(target_os = "macos"))]
                        {
                            let result = match app_handle.get_webview_window("main") {
                                Some(window) => window
                                    .set_ignore_cursor_events(enabled)
                                    .map_err(|e| e.to_string()),
                                None => Err("Main window not found".to_string()),
                            };
                            return Some(match result {
                                Ok(_) => serde_json::json!({ "result": { "enabled": enabled } }),
                                Err(e) => serde_json::json!({ "error": e }),
                            });
                        }
                    }
                    None // Not handled, fall through to axio::rpc
                });

            // Create WebSocket state (also serves as EventSink for axio)
            let ws_state = WebSocketState::new(sender).with_custom_handler(custom_handler);
            axio::set_event_sink(ws_state.clone());

            // Initialize AXIO (ElementRegistry, etc.)
            axio::api::initialize();

            let (screen_width, screen_height) = get_main_screen_dimensions();
            if let Some(window) = app.get_webview_window("main") {
                window
                    .set_size(tauri::LogicalSize::new(screen_width, screen_height))
                    .ok();
                window
                    .set_position(tauri::LogicalPosition::new(0.0, 0.0))
                    .ok();
                window.set_ignore_cursor_events(false).ok();

                // On macOS, convert the window to a non-activating NSPanel
                // This allows the overlay to receive clicks without stealing focus
                // from the app the user is working with
                #[cfg(target_os = "macos")]
                {
                    let panel = window
                        .to_panel::<AxioPanel>()
                        .expect("Failed to convert window to panel");

                    // Set the NonactivatingPanel style mask - this is the key to non-focus-stealing!
                    // This tells macOS this panel should not activate when clicked
                    let style = StyleMask::empty().nonactivating_panel();
                    panel.set_style_mask(style.into());

                    // Set panel to floating level (above regular windows but below screen savers)
                    panel.set_level(PanelLevel::Floating.into());
                    // Critical: Only become key window if no other window can (non-activating behavior)
                    panel.set_becomes_key_only_if_needed(true);
                    // Don't hide when app deactivates (we want overlay always visible)
                    panel.set_hides_on_deactivate(false);
                    // Make it a floating panel programmatically as well
                    panel.set_floating_panel(true);
                    // Show the panel
                    panel.show();
                }

                #[cfg(not(target_os = "macos"))]
                {
                    window.show().ok();
                }
            }

            // Set up global shortcuts
            #[cfg(desktop)]
            {
                let toggle_shortcut =
                    Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyE);
                let devtools_shortcut =
                    Shortcut::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyI);

                app.handle().plugin(
                    tauri_plugin_global_shortcut::Builder::new()
                        .with_handler(move |app_handle, shortcut, event| {
                            if event.state() == ShortcutState::Pressed {
                                if shortcut == &toggle_shortcut {
                                    let _ = toggle_clickthrough_internal(&app_handle);
                                } else if shortcut == &devtools_shortcut {
                                    // Toggle dev tools
                                    if let Some(window) = app_handle.get_webview_window("main") {
                                        if window.is_devtools_open() {
                                            window.close_devtools();
                                        } else {
                                            window.open_devtools();
                                        }
                                    }
                                }
                            }
                        })
                        .build(),
                )?;

                app.global_shortcut().register(toggle_shortcut)?;
                app.global_shortcut().register(devtools_shortcut)?;
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

            // Start window polling (events broadcast via EventSink -> WsEventSink)
            let current_pid = std::process::id();
            axio::start_polling(PollingConfig {
                enum_options: WindowEnumOptions {
                    exclude_pid: Some(current_pid),
                    filter_fullscreen: true,
                    filter_offscreen: true,
                },
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
