// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
  path::{Path, PathBuf},
  sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
  },
  thread,
  time::Duration,
};
use tauri::{
  image::Image,
  menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
  tray::TrayIconBuilder,
  AppHandle, Manager,
};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, ManagerExt as _, PanelLevel, StyleMask, WebviewWindowExt as _};

use axio::{PollingHandle, PollingOptions};
use axio_ws::WebSocketState;

// ============================================================================
// macOS Panel Configuration
// ============================================================================

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(AxioPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

// ============================================================================
// App State
// ============================================================================

struct AppState {
  clickthrough_enabled: AtomicBool,
  current_overlay: Mutex<String>,
  /// Handle to control the polling thread. Stops polling when dropped.
  polling_handle: Mutex<Option<PollingHandle>>,
  /// Guards against menu updates during tray event handling.
  /// The muda crate can crash if the menu is replaced while it's accessing menu items.
  tray_event_active: AtomicBool,
}

impl Default for AppState {
  fn default() -> Self {
    Self {
      clickthrough_enabled: AtomicBool::new(false),
      current_overlay: Mutex::new(String::new()),
      polling_handle: Mutex::new(None),
      tray_event_active: AtomicBool::new(false),
    }
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

fn is_dev_mode() -> bool {
  let exe_path = std::env::current_exe().unwrap_or_default();
  let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new(""));
  exe_dir.ends_with("debug") || exe_dir.ends_with("release")
}

fn get_main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, &'static str> {
  app
    .get_webview_window("main")
    .ok_or("Main window not found")
}

fn get_overlay_url(filename: &str) -> String {
  if is_dev_mode() {
    format!("http://localhost:1420/src-web/overlays/{filename}")
  } else {
    format!("tauri://localhost/{filename}")
  }
}

// ============================================================================
// Overlay Discovery
// ============================================================================

const DEFAULT_OVERLAYS: &[&str] = &[
  "axtrees.html",
  "identifiers.html",
  "ports.html",
  "sand.html",
  "windows-debug.html",
];

fn get_overlay_files() -> Vec<String> {
  if is_dev_mode() {
    return DEFAULT_OVERLAYS.iter().map(|s| s.to_string()).collect();
  }

  let mut overlays: Vec<String> = get_dist_directory()
    .and_then(|dir| std::fs::read_dir(dir).ok())
    .map(|entries| {
      entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|name| name.ends_with(".html"))
        .collect()
    })
    .unwrap_or_default();

  if overlays.is_empty() {
    overlays = DEFAULT_OVERLAYS.iter().map(|s| s.to_string()).collect();
  }
  overlays.sort();
  overlays
}

fn get_dist_directory() -> Option<PathBuf> {
  let exe_path = std::env::current_exe().ok()?;
  let exe_dir = exe_path.parent()?;

  #[cfg(target_os = "macos")]
  return exe_dir.parent().map(|p| p.join("Resources"));

  #[cfg(not(target_os = "macos"))]
  return Some(exe_dir.to_path_buf());
}

// ============================================================================
// Tray Icon Management
// ============================================================================

fn get_icon_path(passthrough: bool) -> PathBuf {
  let icon_name = if passthrough {
    "32x32-passthrough.png"
  } else {
    "32x32.png"
  };

  if is_dev_mode() {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("icons")
      .join(icon_name)
  } else {
    let exe_path = std::env::current_exe().unwrap_or_default();
    let exe_dir = exe_path.parent().unwrap_or(std::path::Path::new(""));

    #[cfg(target_os = "macos")]
    let icons_dir = exe_dir
      .parent()
      .map(|p| p.join("Resources/icons"))
      .unwrap_or_default();

    #[cfg(not(target_os = "macos"))]
    let icons_dir = exe_dir.join("icons");

    icons_dir.join(icon_name)
  }
}

fn get_tray_icon(passthrough: bool) -> Option<Image<'static>> {
  Image::from_path(get_icon_path(passthrough)).ok()
}

// ============================================================================
// Tray Menu
// ============================================================================

fn build_tray_menu(
  app: &AppHandle,
  overlay_files: &[String],
  current_overlay: &str,
  passthrough_enabled: bool,
) -> Result<tauri::menu::Menu<tauri::Wry>, Box<dyn std::error::Error>> {
  let mut menu = MenuBuilder::new(app);

  // Overlay items
  if overlay_files.is_empty() {
    menu = menu.item(
      &MenuItemBuilder::new("No overlays found")
        .id("no_overlays")
        .enabled(false)
        .build(app)?,
    );
  } else {
    for filename in overlay_files {
      let display_name = filename.trim_end_matches(".html");
      let item = CheckMenuItemBuilder::new(display_name)
        .id(filename)
        .checked(current_overlay == filename)
        .build(app)?;
      menu = menu.item(&item);
    }
  }

  menu = menu.item(&PredefinedMenuItem::separator(app)?);

  // Load options
  menu = menu.item(
    &MenuItemBuilder::new("Load URL...")
      .id("load_url")
      .build(app)?,
  );
  menu = menu.item(
    &MenuItemBuilder::new("Load File...")
      .id("load_file")
      .build(app)?,
  );

  menu = menu.item(&PredefinedMenuItem::separator(app)?);

  // Passthrough toggle
  let passthrough_text = if passthrough_enabled {
    "Disable Passthrough"
  } else {
    "Enable Passthrough"
  };
  menu = menu.item(
    &MenuItemBuilder::new(passthrough_text)
      .id("toggle_passthrough")
      .build(app)?,
  );

  menu = menu.item(&PredefinedMenuItem::separator(app)?);
  menu = menu.item(&MenuItemBuilder::new("Quit").id("quit").build(app)?);

  menu.build().map_err(Into::into)
}

fn build_or_update_tray(
  app: &AppHandle,
  overlay_files: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
  build_or_update_tray_inner(app, overlay_files, false)
}

/// Update tray, optionally icon-only (safer during potential menu interactions).
fn build_or_update_tray_inner(
  app: &AppHandle,
  overlay_files: &[String],
  icon_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
  let state = app.state::<AppState>();

  // Safety: Skip menu updates if a tray event is being processed.
  // The muda crate can crash (use-after-free) if we replace the menu
  // while it's still accessing the old menu items internally.
  if !icon_only && state.tray_event_active.load(Ordering::SeqCst) {
    return Ok(());
  }

  let current_overlay = state.current_overlay.lock().unwrap().clone();
  let passthrough_enabled = state.clickthrough_enabled.load(Ordering::Relaxed);

  if let Some(tray) = app.tray_by_id("main-tray") {
    // Always safe to update icon
    if let Some(icon) = get_tray_icon(passthrough_enabled) {
      let _ = tray.set_icon(Some(icon));
    }

    // Only update menu if not icon-only mode
    if !icon_only {
      let menu = build_tray_menu(app, overlay_files, &current_overlay, passthrough_enabled)?;
      tray.set_menu(Some(menu))?;
    }
  } else {
    // Create new tray (first time setup)
    let menu = build_tray_menu(app, overlay_files, &current_overlay, passthrough_enabled)?;
    let icon = get_tray_icon(passthrough_enabled)
      .unwrap_or_else(|| app.default_window_icon().unwrap().clone());

    TrayIconBuilder::with_id("main-tray")
      .menu(&menu)
      .icon(icon)
      .on_menu_event(handle_tray_event)
      .build(app)?;
  }

  Ok(())
}

fn handle_tray_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
  let id = event.id().0.clone();
  let handle = app.clone();

  // Mark that we're handling a tray event - this blocks menu rebuilds.
  // The muda crate crashes if the menu is replaced while it's still
  // accessing the old menu item's String data internally.
  let state = app.state::<AppState>();
  state.tray_event_active.store(true, Ordering::SeqCst);

  // IMPORTANT: Defer execution to avoid use-after-free.
  // We spawn a thread, sleep briefly to let muda finish its internal cleanup,
  // then dispatch to main thread to handle the event.
  thread::spawn(move || {
    // Wait for muda to finish accessing the old menu items.
    // This delay is critical - without it, muda may still be iterating
    // over menu items when we clear the tray_event_active flag.
    thread::sleep(Duration::from_millis(50));

    let app = handle.clone();
    let _ = handle.run_on_main_thread(move || {
      // Re-enable menu updates now that muda has finished
      app
        .state::<AppState>()
        .tray_event_active
        .store(false, Ordering::SeqCst);

      match id.as_str() {
        "toggle_passthrough" => {
          let _ = toggle_passthrough(&app);
        }
        "load_url" => show_url_dialog(&app),
        "load_file" => show_file_dialog(&app),
        "quit" => app.exit(0),
        "no_overlays" => {}
        id => {
          let _ = switch_overlay(&app, id);
        }
      }
    });
  });
}

// ============================================================================
// Core Actions
// ============================================================================

fn toggle_passthrough(app: &AppHandle) -> Result<bool, Box<dyn std::error::Error>> {
  let state = app.state::<AppState>();
  let window = get_main_window(app)?;

  let was_enabled = state.clickthrough_enabled.load(Ordering::Relaxed);
  let now_enabled = !was_enabled;

  window.set_ignore_cursor_events(now_enabled)?;
  state
    .clickthrough_enabled
    .store(now_enabled, Ordering::Relaxed);

  build_or_update_tray(app, &get_overlay_files())?;
  Ok(now_enabled)
}

fn switch_overlay(app: &AppHandle, filename: &str) -> Result<(), Box<dyn std::error::Error>> {
  let state = app.state::<AppState>();
  *state.current_overlay.lock().unwrap() = filename.to_string();

  let window = get_main_window(app)?;
  window.navigate(
    get_overlay_url(filename)
      .parse()
      .expect("Invalid overlay URL"),
  )?;

  build_or_update_tray(app, &get_overlay_files())?;
  Ok(())
}

fn show_url_dialog(app: &AppHandle) {
  // Disable passthrough so user can interact with the dialog
  let state = app.state::<AppState>();
  if state.clickthrough_enabled.load(Ordering::Relaxed) {
    let _ = toggle_passthrough(app);
  }

  if let Ok(window) = get_main_window(app) {
    let url = get_overlay_url("url-input.html");
    let _ = window.navigate(
      url
        .parse()
        .unwrap_or_else(|_| "about:blank".parse().unwrap()),
    );
  }
}

fn show_file_dialog(app: &AppHandle) {
  use tauri_plugin_dialog::DialogExt;

  let app_clone = app.clone();
  app
    .dialog()
    .file()
    .add_filter("HTML Files", &["html", "htm"])
    .add_filter("All Files", &["*"])
    .pick_file(move |result| {
      if let Some(path) = result.and_then(|p| p.as_path().map(|p| p.to_path_buf())) {
        let _ = load_file(&app_clone, &path);
      }
    });
}

fn load_file(app: &AppHandle, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
  let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
  *app.state::<AppState>().current_overlay.lock().unwrap() = format!("file: {file_name}");

  let url = format!("file://{}", path.display());
  get_main_window(app)?.navigate(url.parse()?)?;
  Ok(())
}

// ============================================================================
// WebSocket RPC Handler
// ============================================================================

fn create_rpc_handler(app_handle: AppHandle) -> axio_ws::CustomRpcHandler {
  let last_state = std::sync::Arc::new(AtomicBool::new(true));

  std::sync::Arc::new(move |method, args| {
    if method != "set_passthrough" && method != "set_clickthrough" {
      return None;
    }

    let enabled = args["enabled"].as_bool().unwrap_or(false);

    // Skip if no change
    if last_state.swap(enabled, Ordering::SeqCst) == enabled {
      return Some(serde_json::json!({ "result": { "enabled": enabled, "changed": false } }));
    }

    // Update AppState so tray reflects the change
    app_handle
      .state::<AppState>()
      .clickthrough_enabled
      .store(enabled, Ordering::Relaxed);

    #[cfg(target_os = "macos")]
    {
      let handle = app_handle.clone();
      thread::spawn(move || {
        let h = handle.clone();
        let _ = handle.run_on_main_thread(move || {
          if let Ok(panel) = h.get_webview_panel("main") {
            panel.set_ignores_mouse_events(enabled);
            if enabled {
              panel.resign_key_window();
            } else {
              panel.make_key_window();
            }
          }
          // Use icon-only update to avoid rebuilding the menu.
          // This is much safer during potential tray interactions.
          // The menu text ("Enable/Disable Passthrough") will be updated
          // next time the user actually clicks on the tray.
          let _ = build_or_update_tray_inner(&h, &get_overlay_files(), true);
        });
      });
      Some(serde_json::json!({ "result": { "enabled": enabled, "changed": true } }))
    }

    #[cfg(not(target_os = "macos"))]
    {
      let result = app_handle
        .get_webview_window("main")
        .ok_or("Window not found")
        .and_then(|w| w.set_ignore_cursor_events(enabled).map_err(|_| "Failed"));

      // Use icon-only update to avoid rebuilding the menu during potential interactions
      let _ = build_or_update_tray_inner(&app_handle, &get_overlay_files(), true);

      Some(match result {
        Ok(_) => serde_json::json!({ "result": { "enabled": enabled } }),
        Err(e) => serde_json::json!({ "error": e }),
      })
    }
  })
}

// ============================================================================
// Window Setup
// ============================================================================

fn setup_main_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
  let (width, height) = axio::screen::dimensions();
  let window = app
    .get_webview_window("main")
    .ok_or("Main window not found")?;

  window.set_size(tauri::LogicalSize::new(width, height))?;
  window.set_position(tauri::LogicalPosition::new(0.0, 0.0))?;
  window.set_ignore_cursor_events(true)?;

  #[cfg(target_os = "macos")]
  setup_macos_panel(&window)?;

  #[cfg(not(target_os = "macos"))]
  window.show()?;

  Ok(())
}

#[cfg(target_os = "macos")]
fn setup_macos_panel(window: &tauri::WebviewWindow) -> Result<(), Box<dyn std::error::Error>> {
  let panel = window.to_panel::<AxioPanel>()?;

  panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
  panel.set_level(PanelLevel::Floating.into());
  panel.set_becomes_key_only_if_needed(true);
  panel.set_hides_on_deactivate(false);
  panel.set_floating_panel(true);
  panel.set_ignores_mouse_events(true);
  panel.show();

  Ok(())
}

fn setup_shortcuts(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
  let toggle = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyE);
  let devtools = Shortcut::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyI);

  app.handle().plugin(
    tauri_plugin_global_shortcut::Builder::new()
      .with_handler(move |app, shortcut, event| {
        if event.state() != ShortcutState::Pressed {
          return;
        }

        if shortcut == &toggle {
          let _ = toggle_passthrough(app);
        } else if shortcut == &devtools {
          if let Some(w) = app.get_webview_window("main") {
            if w.is_devtools_open() {
              w.close_devtools();
            } else {
              w.open_devtools();
            }
          }
        }
      })
      .build(),
  )?;

  app.global_shortcut().register(toggle)?;
  app.global_shortcut().register(devtools)?;

  Ok(())
}

// ============================================================================
// Main Entry Point
// ============================================================================

fn main() {
  // Initialize logging: RUST_LOG=debug cargo tauri dev
  env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

  let mut builder = tauri::Builder::default()
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_dialog::init());

  #[cfg(target_os = "macos")]
  {
    builder = builder.plugin(tauri_nspanel::init());
  }

  builder
    .manage(AppState::default())
    .setup(|app| {
      // WebSocket setup
      let (sender, _) = tokio::sync::broadcast::channel(1000);
      let ws_state = WebSocketState::new(std::sync::Arc::new(sender))
        .with_custom_handler(create_rpc_handler(app.handle().clone()));
      axio::set_event_sink(ws_state.clone());
      if !axio::verify_permissions() {
        eprintln!("[axio] ⚠️  Accessibility permissions NOT granted!");
        eprintln!("[axio]    Go to System Preferences > Privacy & Security > Accessibility");
      }

      // Window setup
      setup_main_window(app)?;

      // Shortcuts
      #[cfg(desktop)]
      setup_shortcuts(app)?;

      // Tray setup
      let overlays = get_overlay_files();
      build_or_update_tray(app.handle(), &overlays)?;

      // Load first overlay
      if let Some(first) = overlays.first() {
        *app.state::<AppState>().current_overlay.lock().unwrap() = first.clone();
        if let Some(w) = app.get_webview_window("main") {
          w.navigate(get_overlay_url(first).parse().expect("Invalid URL"))?;
        }
      }

      // Start polling (handles windows + mouse position in one loop)
      let polling_handle = axio::start_polling(PollingOptions {
        exclude_pid: Some(axio::ProcessId::from(std::process::id())),
        ..PollingOptions::default()
      });
      *app.state::<AppState>().polling_handle.lock().unwrap() = Some(polling_handle);

      let ws = ws_state.clone();
      thread::spawn(move || {
        tokio::runtime::Runtime::new()
          .expect("Failed to create runtime")
          .block_on(axio_ws::start_ws_server(ws));
      });

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
