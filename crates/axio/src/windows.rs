//! Window enumeration and polling using x-win.
//! Also handles mouse position tracking in the same polling loop.

use crate::types::AXWindow;
use crate::window_manager::WindowManager;
use crate::WindowId;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, RwLock};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use core_graphics::display::{
  CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};
#[cfg(target_os = "macos")]
use core_graphics::event::CGEvent;
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

pub const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

const FILTERED_BUNDLE_IDS: &[&str] = &[
  "com.apple.screencaptureui",
  "com.apple.screenshot.launcher",
  "com.apple.ScreenContinuity",
];

static BUNDLE_ID_CACHE: Lazy<Mutex<HashMap<u32, Option<String>>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

/// Clean up bundle ID cache entries for PIDs that are no longer active.
#[cfg(target_os = "macos")]
fn cleanup_bundle_id_cache(active_pids: &HashSet<u32>) -> usize {
  let mut cache = BUNDLE_ID_CACHE.lock().unwrap();
  let initial_size = cache.len();
  cache.retain(|pid, _| active_pids.contains(pid));
  initial_size - cache.len()
}

#[cfg(not(target_os = "macos"))]
fn cleanup_bundle_id_cache(_active_pids: &HashSet<u32>) -> usize {
  0
}

/// Get the current size of the bundle ID cache (for diagnostics).
pub fn bundle_id_cache_size() -> usize {
  BUNDLE_ID_CACHE.lock().unwrap().len()
}

/// Last known window list from polling. Always available immediately.
static CURRENT_WINDOWS: Lazy<RwLock<Vec<AXWindow>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// Active window ID - the most recent valid focused window (preserved when focus goes to desktop)
static ACTIVE_WINDOW: Lazy<RwLock<Option<String>>> = Lazy::new(|| RwLock::new(None));

/// Get the last known window list. Returns immediately without polling.
pub fn get_current_windows() -> Vec<AXWindow> {
  CURRENT_WINDOWS.read().unwrap().clone()
}

/// Get the active window ID (most recent valid focus, preserved when desktop focused)
pub fn get_active_window() -> Option<String> {
  ACTIVE_WINDOW.read().unwrap().clone()
}

#[cfg(target_os = "macos")]
fn parse_bundle_id(info: &str) -> Option<String> {
  let eq_pos = info.rfind('=')?;
  let after_eq = &info[eq_pos + 1..];
  let start = after_eq.find('"')?;
  let end = after_eq[start + 1..].find('"')?;
  Some(after_eq[start + 1..start + 1 + end].to_string())
}

#[cfg(target_os = "macos")]
fn get_bundle_id(pid: u32) -> Option<String> {
  use std::process::Command;

  {
    let cache = BUNDLE_ID_CACHE.lock().unwrap();
    if let Some(cached) = cache.get(&pid) {
      return cached.clone();
    }
  }

  // TODO: Use native NSRunningApplication API instead of shelling out
  let bundle_id = Command::new("lsappinfo")
    .args(["info", "-only", "bundleid", &format!("{}", pid)])
    .output()
    .ok()
    .and_then(|output| String::from_utf8(output.stdout).ok())
    .and_then(|info| parse_bundle_id(&info));

  BUNDLE_ID_CACHE
    .lock()
    .unwrap()
    .insert(pid, bundle_id.clone());

  bundle_id
}

#[cfg(target_os = "macos")]
fn should_filter_process(pid: u32) -> bool {
  get_bundle_id(pid).map_or(false, |id| FILTERED_BUNDLE_IDS.contains(&id.as_str()))
}

#[cfg(not(target_os = "macos"))]
fn should_filter_process(_pid: u32) -> bool {
  false
}

#[cfg(target_os = "macos")]
pub fn get_main_screen_dimensions() -> (f64, f64) {
  unsafe {
    let display_id: CGDirectDisplayID = CGMainDisplayID();
    (
      CGDisplayPixelsWide(display_id) as f64,
      CGDisplayPixelsHigh(display_id) as f64,
    )
  }
}

#[cfg(not(target_os = "macos"))]
pub fn get_main_screen_dimensions() -> (f64, f64) {
  (1920.0, 1080.0)
}

// === Mouse Position ===

#[cfg(target_os = "macos")]
pub fn get_mouse_position() -> Option<(f64, f64)> {
  let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
  let event = CGEvent::new(source).ok()?;
  let location = event.location();
  Some((location.x, location.y))
}

#[cfg(not(target_os = "macos"))]
pub fn get_mouse_position() -> Option<(f64, f64)> {
  None
}

fn window_from_x_win(window: &x_win::WindowInfo) -> AXWindow {
  use crate::types::Bounds;
  AXWindow {
    id: window.id.clone(),
    title: window.title.clone(),
    app_name: window.app_name.clone(),
    bounds: Bounds {
      x: window.x,
      y: window.y,
      w: window.w,
      h: window.h,
    },
    focused: window.focused,
    process_id: window.process_id,
    z_index: window.z_index,
  }
}

#[derive(Clone, Default)]
pub struct WindowEnumOptions {
  /// PID to exclude. Its window position is used as coordinate offset.
  pub exclude_pid: Option<u32>,
  pub filter_fullscreen: bool,
  pub filter_offscreen: bool,
}

/// Returns None if exclude_pid is set but that window isn't found.
pub fn get_windows(options: &WindowEnumOptions) -> Option<Vec<AXWindow>> {
  use std::panic;

  let all_windows = match panic::catch_unwind(|| x_win::get_open_windows()) {
    Ok(Ok(windows)) => windows,
    _ => return Some(Vec::new()),
  };

  let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows.iter().find(|w| w.process_id == exclude_pid) {
      Some(overlay_window) => (overlay_window.x, overlay_window.y),
      None => return None,
    }
  } else {
    (0.0, 0.0)
  };

  let (screen_width, screen_height) = get_main_screen_dimensions();

  // Filter windows first, preserving x-win's z-order (front to back)
  let filtered: Vec<_> = all_windows
    .iter()
    .filter(|w| {
      if options.exclude_pid == Some(w.process_id) {
        return false;
      }
      if should_filter_process(w.process_id) {
        return false;
      }
      true
    })
    .collect();

  // Map to AXWindow (z_index already set by x-win, 0 = frontmost)
  let windows = filtered
    .iter()
    .map(|w| {
      let mut info = window_from_x_win(w);
      info.bounds.x -= offset_x;
      info.bounds.y -= offset_y;
      info
    })
    .filter(|w| {
      if options.filter_fullscreen {
        let is_fullscreen = w.bounds.x == 0.0
          && w.bounds.y == 0.0
          && w.bounds.w == screen_width
          && w.bounds.h == screen_height;
        if is_fullscreen {
          return false;
        }
      }
      if options.filter_offscreen && w.bounds.x > screen_width + 1.0 {
        return false;
      }
      true
    })
    .collect();

  Some(windows)
}

#[derive(Clone)]
pub struct PollingConfig {
  pub enum_options: WindowEnumOptions,
  pub interval_ms: u64,
}

impl Default for PollingConfig {
  fn default() -> Self {
    Self {
      enum_options: WindowEnumOptions::default(),
      interval_ms: DEFAULT_POLLING_INTERVAL_MS,
    }
  }
}

/// How often to run cleanup and diagnostics (in poll cycles).
/// At 8ms interval, 1250 cycles ≈ 10 seconds.
const CLEANUP_INTERVAL: u64 = 1250;

/// Runs in background thread, emits events via EventSink.
/// Handles both window enumeration and mouse position tracking.
pub fn start_polling(config: PollingConfig) {
  thread::spawn(move || {
    let mut last_windows: HashMap<String, AXWindow> = HashMap::new();
    let mut last_active_id: Option<String> = None;
    let mut last_focused_id: Option<String> = None;
    let mut last_mouse_pos: Option<(f64, f64)> = None;
    let mut poll_count: u64 = 0;

    loop {
      let loop_start = Instant::now();
      poll_count += 1;

      // Mouse position polling (very cheap, ~0.1ms)
      if let Some((x, y)) = get_mouse_position() {
        let changed = match last_mouse_pos {
          Some((lx, ly)) => (x - lx).abs() >= 1.0 || (y - ly).abs() >= 1.0,
          None => true,
        };
        if changed {
          last_mouse_pos = Some((x, y));
          crate::events::emit_mouse_position(x, y);
        }
      }

      if let Some(raw_windows) = get_windows(&config.enum_options) {
        // Update WindowManager - returns windows with preserved children/title
        let (managed_windows, _, _) = WindowManager::update_windows(raw_windows);

        // Build current window map from managed windows (preserves children)
        let mut current_windows: Vec<AXWindow> =
          managed_windows.iter().map(|m| m.info.clone()).collect();
        let current_map: HashMap<String, AXWindow> = current_windows
          .iter()
          .map(|w| (w.id.clone(), w.clone()))
          .collect();
        let current_ids: HashSet<&String> = current_map.keys().collect();
        let last_ids: HashSet<&String> = last_windows.keys().collect();

        // Compute depth_order (window IDs sorted by z_index, front to back)
        let depth_order: Vec<WindowId> = {
          let mut sorted = current_windows.clone();
          sorted.sort_by_key(|w| w.z_index);
          sorted.into_iter().map(|w| WindowId::new(w.id)).collect()
        };

        // Detect removed windows (emit before removal, include full data)
        for removed_id in last_ids.difference(&current_ids) {
          if let Some(window) = last_windows.get(*removed_id) {
            crate::events::emit_window_removed(window, &depth_order);
          }
        }

        // Detect added windows
        for added_id in current_ids.difference(&last_ids) {
          if let Some(window) = current_map.get(*added_id) {
            // Enable accessibility for Electron apps (Signal, Discord, etc.)
            // This must be done when the window is first discovered to give the
            // accessibility tree time to populate before we try to query elements
            crate::platform::macos::enable_accessibility_for_pid(window.process_id);
            crate::events::emit_window_added(window, &depth_order);
          }
        }

        // Detect changed windows (position, title, etc changed)
        for id in current_ids.intersection(&last_ids) {
          let current = current_map.get(*id).unwrap();
          let last = last_windows.get(*id).unwrap();
          if current != last {
            crate::events::emit_window_changed(current, &depth_order);
          }
        }

        // Find focused window and update active_window
        let focused_window = current_windows.iter_mut().find(|w| w.focused);
        let current_focused_id = focused_window.as_ref().map(|w| w.id.clone());

        // Track focus changes
        let focus_changed = current_focused_id != last_focused_id;
        if focus_changed {
          let window_id = current_focused_id
            .as_ref()
            .map(|id| WindowId::new(id.clone()));
          crate::events::emit_focus_changed(window_id.as_ref());
          last_focused_id = current_focused_id.clone();
        }

        // Update active_window: if a window has focus, it becomes active
        // If no window has focus (desktop), active_window is preserved
        if let Some(ref focused_id) = current_focused_id {
          let active_changed = last_active_id.as_ref() != Some(focused_id);
          if active_changed {
            *ACTIVE_WINDOW.write().unwrap() = Some(focused_id.clone());
            let window_id = WindowId::new(focused_id.clone());
            crate::events::emit_active_changed(&window_id);
            last_active_id = Some(focused_id.clone());
            // Note: Element discovery is now client-initiated via RPC.
            // This keeps the polling loop pure and fast.
          }
        }
        // Note: when focus goes to desktop (current_focused_id is None),
        // we preserve last_active_id - no event emitted, active window stays same

        // Update global state
        *CURRENT_WINDOWS.write().unwrap() = current_windows;
        last_windows = current_map;

        // Periodic cleanup for dead PIDs
        if poll_count % CLEANUP_INTERVAL == 0 {
          // Collect active PIDs from current windows
          let active_pids: HashSet<u32> = last_windows.values().map(|w| w.process_id).collect();

          // Clean up caches for dead PIDs
          let _bundle_cleaned = cleanup_bundle_id_cache(&active_pids);

          #[cfg(target_os = "macos")]
          let _observers_cleaned = crate::platform::macos::cleanup_dead_observers(&active_pids);
        }
      }

      let elapsed = loop_start.elapsed();
      let target = Duration::from_millis(config.interval_ms);
      if elapsed < target {
        thread::sleep(target - elapsed);
      }
    }
  });
}
