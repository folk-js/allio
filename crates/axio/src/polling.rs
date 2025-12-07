use crate::events::emit;
use crate::platform;
use crate::types::{AXWindow, Event, Point, ProcessId, WindowId};
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

pub const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

/// Poll for current windows. Returns None if exclude_pid window isn't found.
fn poll_windows(options: &PollingOptions) -> Option<Vec<AXWindow>> {
  let all_windows = platform::enumerate_windows();

  // Find offset from excluded window (e.g., our overlay)
  let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows
      .iter()
      .find(|w| w.process_id.as_u32() == exclude_pid.as_u32())
    {
      Some(overlay_window) => (overlay_window.bounds.x, overlay_window.bounds.y),
      None => return None,
    }
  } else {
    (0.0, 0.0)
  };

  let (screen_width, screen_height) = platform::get_main_screen_dimensions();

  let windows: Vec<AXWindow> = all_windows
    .into_iter()
    .filter(|w| {
      // Exclude our own window
      if options
        .exclude_pid
        .map_or(false, |pid| w.process_id.as_u32() == pid.as_u32())
      {
        return false;
      }
      true
    })
    .map(|mut w| {
      w.bounds.x -= offset_x;
      w.bounds.y -= offset_y;
      w
    })
    .filter(|w| {
      // Filter fullscreen windows
      if options.filter_fullscreen {
        let is_fullscreen = w.bounds.x == 0.0
          && w.bounds.y == 0.0
          && w.bounds.w == screen_width
          && w.bounds.h == screen_height;
        if is_fullscreen {
          return false;
        }
      }
      // Filter offscreen windows
      if options.filter_offscreen && w.bounds.x > screen_width + 1.0 {
        return false;
      }
      true
    })
    .collect();

  Some(windows)
}

/// Window polling filters and interval.
#[derive(Clone)]
pub struct PollingOptions {
  /// PID to exclude. Its window position is used as coordinate offset.
  pub exclude_pid: Option<ProcessId>,
  pub filter_fullscreen: bool,
  pub filter_offscreen: bool,
  pub interval_ms: u64,
}

impl Default for PollingOptions {
  fn default() -> Self {
    Self {
      exclude_pid: None,
      filter_fullscreen: true,
      filter_offscreen: true,
      interval_ms: DEFAULT_POLLING_INTERVAL_MS,
    }
  }
}

/// How often to run cleanup (in poll cycles). At 8ms, 1250 cycles ≈ 10 seconds.
const CLEANUP_INTERVAL: u64 = 1250;

/// Start background polling for windows and mouse position.
pub fn start_polling(config: PollingOptions) {
  use crate::window_registry;

  thread::spawn(move || {
    let mut last_active_id: Option<WindowId> = None;
    let mut last_focused_id: Option<WindowId> = None;
    let mut last_mouse_pos: Option<Point> = None;
    let mut poll_count: u64 = 0;

    loop {
      let loop_start = Instant::now();
      poll_count += 1;

      // Mouse position polling
      if let Some(pos) = platform::get_mouse_position() {
        let changed = last_mouse_pos.map_or(true, |last| pos.moved_from(last, 1.0));
        if changed {
          last_mouse_pos = Some(pos);
          emit(Event::MousePosition(pos));
        }
      }

      if let Some(raw_windows) = poll_windows(&config) {
        let result = window_registry::update(raw_windows);

        // Emit events for removed windows
        for removed_id in &result.removed {
          emit(Event::WindowRemoved {
            window_id: removed_id.clone(),
            depth_order: result.depth_order.clone(),
          });
        }

        // Emit events for added windows
        for added_id in &result.added {
          if let Some(window) = window_registry::get_window(added_id) {
            platform::enable_accessibility_for_pid(window.process_id);
            emit(Event::WindowAdded {
              window: window.clone(),
              depth_order: result.depth_order.clone(),
            });
          }
        }

        // Emit events for changed windows
        for changed_id in &result.changed {
          if let Some(window) = window_registry::get_window(changed_id) {
            emit(Event::WindowChanged {
              window: window.clone(),
              depth_order: result.depth_order.clone(),
            });
          }
        }

        // Focus tracking
        let windows = window_registry::get_windows();
        let focused_window = windows.iter().find(|w| w.focused);
        let current_focused_id = focused_window.map(|w| w.id.clone());

        if current_focused_id != last_focused_id {
          emit(Event::FocusChanged {
            window_id: current_focused_id.clone(),
          });
          last_focused_id = current_focused_id.clone();
        }

        // Active window tracking
        if let Some(ref focused_id) = current_focused_id {
          if last_active_id.as_ref() != Some(focused_id) {
            window_registry::set_active(Some(focused_id.clone()));
            emit(Event::ActiveChanged {
              window_id: focused_id.clone(),
            });
            last_active_id = Some(focused_id.clone());
          }
        }

        // Periodic cleanup
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
