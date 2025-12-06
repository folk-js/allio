use crate::platform;
use crate::types::AXWindow;
use crate::WindowId;
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

const FILTERED_BUNDLE_IDS: &[&str] = &[
  "com.apple.screencaptureui",
  "com.apple.screenshot.launcher",
  "com.apple.ScreenContinuity",
];

/// Check if a window should be filtered based on its bundle ID.
fn should_filter_bundle_id(bundle_id: Option<&str>) -> bool {
  bundle_id.map_or(false, |id| FILTERED_BUNDLE_IDS.contains(&id))
}

fn window_from_x_win(window: &x_win::WindowInfo) -> AXWindow {
  use crate::types::{Bounds, ProcessId};
  AXWindow {
    id: WindowId::new(window.id.clone()),
    title: window.title.clone(),
    app_name: window.app_name.clone(),
    bounds: Bounds {
      x: window.x,
      y: window.y,
      w: window.w,
      h: window.h,
    },
    focused: window.focused,
    process_id: ProcessId::new(window.process_id),
    z_index: window.z_index,
  }
}

use crate::types::ProcessId;

#[derive(Clone, Default)]
pub struct WindowEnumOptions {
  /// PID to exclude. Its window position is used as coordinate offset.
  pub exclude_pid: Option<ProcessId>,
  pub filter_fullscreen: bool,
  pub filter_offscreen: bool,
}

/// Poll x-win for current windows. Returns None if exclude_pid window isn't found.
fn poll_windows(options: &WindowEnumOptions) -> Option<Vec<AXWindow>> {
  use std::panic;

  let all_windows = match panic::catch_unwind(|| x_win::get_open_windows()) {
    Ok(Ok(windows)) => windows,
    _ => return Some(Vec::new()),
  };

  let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows
      .iter()
      .find(|w| w.process_id == exclude_pid.as_u32())
    {
      Some(overlay_window) => (overlay_window.x, overlay_window.y),
      None => return None,
    }
  } else {
    (0.0, 0.0)
  };

  let (screen_width, screen_height) = platform::get_main_screen_dimensions();

  // Filter windows first, preserving x-win's z-order (front to back)
  let filtered: Vec<_> = all_windows
    .iter()
    .filter(|w| {
      if options
        .exclude_pid
        .map_or(false, |pid| w.process_id == pid.as_u32())
      {
        return false;
      }
      if should_filter_bundle_id(w.bundle_id.as_deref()) {
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
  use crate::types::Point;
  use crate::window_registry;

  thread::spawn(move || {
    let mut last_active_id: Option<WindowId> = None;
    let mut last_focused_id: Option<WindowId> = None;
    let mut last_mouse_pos: Option<Point> = None;
    let mut poll_count: u64 = 0;

    loop {
      let loop_start = Instant::now();
      poll_count += 1;

      // Mouse position polling (very cheap, ~0.1ms)
      if let Some(pos) = platform::get_mouse_position() {
        let changed = last_mouse_pos.map_or(true, |last| pos.moved_from(last, 1.0));
        if changed {
          last_mouse_pos = Some(pos);
          crate::events::emit_mouse_position(pos);
        }
      }

      if let Some(raw_windows) = poll_windows(&config.enum_options) {
        // Update registry - handles add/remove/change detection internally
        let result = window_registry::update(raw_windows);

        // Emit events for removed windows
        for removed_id in &result.removed {
          crate::events::emit_window_removed(removed_id, &result.depth_order);
        }

        // Emit events for added windows
        for added_id in &result.added {
          if let Some(window) = window_registry::get_window(added_id) {
            // Enable accessibility for Electron apps
            platform::enable_accessibility_for_pid(window.process_id);
            crate::events::emit_window_added(&window, &result.depth_order);
          }
        }

        // Emit events for changed windows
        for changed_id in &result.changed {
          if let Some(window) = window_registry::get_window(changed_id) {
            crate::events::emit_window_changed(&window, &result.depth_order);
          }
        }

        // Get current windows for focus tracking
        let windows = window_registry::get_windows();
        let focused_window = windows.iter().find(|w| w.focused);
        let current_focused_id = focused_window.map(|w| w.id.clone());

        // Track focus changes
        let focus_changed = current_focused_id != last_focused_id;
        if focus_changed {
          crate::events::emit_focus_changed(current_focused_id.as_ref());
          last_focused_id = current_focused_id.clone();
        }

        // Update active window
        if let Some(ref focused_id) = current_focused_id {
          let active_changed = last_active_id.as_ref() != Some(focused_id);
          if active_changed {
            window_registry::set_active(Some(focused_id.clone()));
            crate::events::emit_active_changed(focused_id);
            last_active_id = Some(focused_id.clone());
          }
        }

        // Periodic cleanup for dead PIDs
        if poll_count % CLEANUP_INTERVAL == 0 {
          let active_pids: HashSet<ProcessId> = windows.iter().map(|w| w.process_id).collect();
          let _observers_cleaned = platform::cleanup_dead_observers(&active_pids);
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
