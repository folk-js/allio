// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
  path::{Path, PathBuf},
  sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
  },
  thread,
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

use allio::Allio;
use allio_ws::WebSocketState;

mod backstage;
#[cfg(target_os = "macos")]
mod menubar;
mod pointer;
mod shaders;
#[cfg(target_os = "macos")]
mod spaces;

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(AllioPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })
}

struct AppState {
  clickthrough_enabled: AtomicBool,
  current_overlay: Mutex<String>,
  /// Guards against menu updates during tray event handling.
  /// The muda crate can crash if the menu is replaced while it's accessing menu items.
  tray_event_active: AtomicBool,
}

impl Default for AppState {
  fn default() -> Self {
    Self {
      clickthrough_enabled: AtomicBool::new(false),
      current_overlay: Mutex::new(String::new()),
      tray_event_active: AtomicBool::new(false),
    }
  }
}

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

const DEFAULT_OVERLAYS: &[&str] = &[
  "axtrees.html",
  "graph.html",
  "identifiers.html",
  "ports.html",
  "query.html",
  "spreadsheet.html",
  "sand.html",
  "windows-debug.html",
  "shader.html",
  "aura.html",
  "xray.html",
  "lava.html",
  "blobs.html",
  "lens.html",
  "magnet.html",
  "cuts.html",
  "warp.html",
  "transform.html",
  "light.html",
  "stickies.html",
];

fn get_overlay_files() -> Vec<String> {
  if is_dev_mode() {
    return DEFAULT_OVERLAYS.iter().map(|s| (*s).to_string()).collect();
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
    overlays = DEFAULT_OVERLAYS.iter().map(|s| (*s).to_string()).collect();
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

    // Only update menu if not icon-only mode (macOS builds its menus on each click)
    if !icon_only && !cfg!(target_os = "macos") {
      let menu = build_tray_menu(app, overlay_files, &current_overlay, passthrough_enabled)?;
      tray.set_menu(Some(menu))?;
    }
  } else {
    let icon = get_tray_icon(passthrough_enabled)
      .unwrap_or_else(|| app.default_window_icon().unwrap().clone());

    // On macOS the menus are native and built on each click (see `menubar`).
    #[cfg(target_os = "macos")]
    {
      TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .on_tray_icon_event(menubar::on_event)
        .build(app)?;
      return Ok(());
    }

    // Create new tray (first time setup)
    #[cfg(not(target_os = "macos"))]
    let menu = build_tray_menu(app, overlay_files, &current_overlay, passthrough_enabled)?;
    #[cfg(not(target_os = "macos"))]
    TrayIconBuilder::with_id("main-tray")
      .menu(&menu)
      .icon(icon)
      .on_menu_event(handle_tray_event)
      .build(app)?;
  }

  Ok(())
}

#[cfg(not(target_os = "macos"))]
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
      if let Some(path) = result.and_then(|p| p.as_path().map(std::path::Path::to_path_buf)) {
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

/// Feeds window geometry to shaders that bind `windows`/`focused`, and keeps it current.
fn start_window_binding(allio: &Allio) -> std::sync::Arc<shaders::Shaders> {
  let source = {
    let allio = allio.clone();
    Box::new(move || {
      // Shaders draw what is on screen: only windows that are here.
      let mut windows: Vec<_> = allio
        .all_windows()
        .into_iter()
        .filter(|w| w.presence == allio::Presence::Here)
        .collect();
      windows.sort_by_key(|w| w.z_index);
      let focused = allio
        .focused_window()
        .and_then(|id| windows.iter().position(|w| w.id == id));
      shaders::Windows {
        ids: windows.iter().map(|w| w.id.0).collect(),
        rects: windows
          .iter()
          .map(|w| {
            [
              w.bounds.x as f32,
              w.bounds.y as f32,
              w.bounds.w as f32,
              w.bounds.h as f32,
            ]
          })
          .collect(),
        focused,
      }
    })
  };
  let shaders = std::sync::Arc::new(shaders::Shaders::new(source));

  let mut events = allio.subscribe();
  let bound = shaders.clone();
  thread::spawn(move || loop {
    match events.recv_blocking() {
      Ok(
        allio::Event::WindowAdded { .. }
        | allio::Event::WindowChanged { .. }
        | allio::Event::WindowRemoved { .. }
        | allio::Event::FocusWindow { .. },
      ) => bound.windows_changed(),
      Ok(_) | Err(async_broadcast::RecvError::Overflowed(_)) => {}
      Err(async_broadcast::RecvError::Closed) => break,
    }
  });
  shaders
}

fn create_rpc_handler(
  app_handle: AppHandle,
  shaders: std::sync::Arc<shaders::Shaders>,
  pointers: std::sync::Arc<pointer::Pointers>,
  backstages: std::sync::Arc<backstage::Backstages>,
  #[cfg(target_os = "macos")] spaces: std::sync::Arc<spaces::Spaces>,
) -> (allio_ws::CustomRpcHandler, allio_ws::DisconnectHandler) {
  let last_state = std::sync::Arc::new(AtomicBool::new(true));

  let on_disconnect: allio_ws::DisconnectHandler = {
    let shaders = shaders.clone();
    let pointers = pointers.clone();
    let backstages = backstages.clone();
    #[cfg(target_os = "macos")]
    let spaces = spaces.clone();
    std::sync::Arc::new(move |conn| {
      shaders.disconnected(conn);
      pointers.disconnected(conn);
      backstages.disconnected(conn);
      #[cfg(target_os = "macos")]
      spaces.disconnected(conn);
    })
  };

  let handler: allio_ws::CustomRpcHandler = std::sync::Arc::new(move |conn, method, args| {
    if let Some(response) = shaders.handle(conn, method, args) {
      return Some(response);
    }
    if let Some(response) = pointers.handle(conn, method, args) {
      return Some(response);
    }
    if let Some(response) = backstages.handle(conn, method, args) {
      return Some(response);
    }
    #[cfg(target_os = "macos")]
    if let Some(response) = spaces.handle(conn, method, args) {
      return Some(response);
    }
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
  });

  (handler, on_disconnect)
}

fn setup_main_window(app: &tauri::App, allio: &Allio) -> Result<(), Box<dyn std::error::Error>> {
  let (width, height) = allio.screen_size();
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
  let panel = window.to_panel::<AllioPanel>()?;

  panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
  panel.set_level(PanelLevel::Floating.into());
  panel.set_becomes_key_only_if_needed(true);
  panel.set_hides_on_deactivate(false);
  panel.set_floating_panel(true);
  panel.set_has_shadow(false);
  panel.set_ignores_mouse_events(true);
  // On every Space, full-screen ones included: the page is one document shown wherever you are
  // (UI belonging to a particular Space is shown there by `spaces`).
  panel.set_collection_behavior(
    tauri_nspanel::CollectionBehavior::new()
      .can_join_all_spaces()
      .full_screen_auxiliary()
      .into(),
  );
  // WebKit throttles pages in windows it thinks are hidden; on full-screen Spaces it can think
  // so of this one.
  let _ = window.with_webview(|webview| unsafe {
    let wk: &objc2::runtime::AnyObject = &*webview.inner().cast();
    let selector = objc2::sel!(_setWindowOcclusionDetectionEnabled:);
    if objc2::msg_send![wk, respondsToSelector: selector] {
      let _: () = objc2::msg_send![wk, _setWindowOcclusionDetectionEnabled: false];
    }
  });
  panel.show();

  Ok(())
}

fn setup_shortcuts(
  app: &tauri::App,
  pointers: std::sync::Arc<pointer::Pointers>,
) -> Result<(), Box<dyn std::error::Error>> {
  let toggle = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::KeyE);
  // Escape hatch: give the pointer back if a pointer field makes it unusable.
  let release = Shortcut::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Escape);
  let devtools = Shortcut::new(Some(Modifiers::SUPER | Modifiers::ALT), Code::KeyI);

  app.handle().plugin(
    tauri_plugin_global_shortcut::Builder::new()
      .with_handler(move |app, shortcut, event| {
        if event.state() != ShortcutState::Pressed {
          return;
        }

        if shortcut == &release {
          pointers.release();
        } else if shortcut == &toggle {
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
  app.global_shortcut().register(release)?;

  Ok(())
}

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
      // Create Allio instance (polling starts automatically)
      let allio = match Allio::builder().exclude_pid(std::process::id()).build() {
        Ok(a) => a,
        Err(_) => {
          eprintln!("[allio] ⚠️  Accessibility permissions NOT granted!");
          eprintln!("[allio]    Go to System Preferences > Privacy & Security > Accessibility");
          std::process::exit(1);
        }
      };

      // WebSocket setup
      let shaders = start_window_binding(&allio);
      let pointers = std::sync::Arc::new(pointer::Pointers::new());
      let backstages = std::sync::Arc::new(backstage::Backstages::default());
      #[cfg(target_os = "macos")]
      app.manage(menubar::Services {
        shaders: shaders.clone(),
        pointers: pointers.clone(),
        backstages: backstages.clone(),
      });
      let (rpc_handler, on_disconnect) = create_rpc_handler(
        app.handle().clone(),
        shaders,
        pointers.clone(),
        backstages,
        #[cfg(target_os = "macos")]
        spaces::Spaces::new(app.handle().clone(), allio.clone()),
      );
      let ws_state = WebSocketState::new(allio.clone())
        .with_custom_handler(rpc_handler)
        .with_disconnect_handler(on_disconnect);

      // Window setup
      setup_main_window(app, &allio)?;

      // Shortcuts
      #[cfg(desktop)]
      setup_shortcuts(app, pointers)?;

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

      // Start WebSocket server (allio polling already running)
      let ws = ws_state.clone();
      thread::spawn(move || {
        tokio::runtime::Runtime::new()
          .expect("Failed to create runtime")
          .block_on(allio_ws::start_server(ws));
      });

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
