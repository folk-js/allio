/*!
Internal polling implementation.

Handles background polling for windows and mouse position.
Consumers don't interact with this directly - polling is owned by `Allio`.
*/

use crate::core::Allio;
use crate::platform::{CurrentPlatform, DisplayLinkHandle, Platform};
use crate::types::{ProcessId, Window};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

enum PollingImpl {
  Thread {
    stop_signal: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
  },
  #[cfg(target_os = "macos")]
  DisplayLink(DisplayLinkHandle),
}

/// Handle to control polling lifetime. Stops on drop.
pub(crate) struct PollingHandle {
  inner: PollingImpl,
}

impl std::fmt::Debug for PollingHandle {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("PollingHandle").finish_non_exhaustive()
  }
}

impl PollingHandle {
  fn stop(&self) {
    match &self.inner {
      PollingImpl::Thread { stop_signal, .. } => {
        stop_signal.store(true, Ordering::SeqCst);
      }
      #[cfg(target_os = "macos")]
      PollingImpl::DisplayLink(handle) => {
        handle.stop();
      }
    }
  }
}

impl Drop for PollingHandle {
  fn drop(&mut self) {
    self.stop();
    if let PollingImpl::Thread { thread, .. } = &mut self.inner {
      if let Some(t) = thread.take() {
        drop(t.join());
      }
    }
  }
}

pub(crate) struct PollWindowsResult {
  pub windows: Vec<Window>,
  pub skip_removal: bool,
}

/// Compute offset from excluded PID's window position.
/// Returns (`offset_x`, `offset_y`, `overlay_missing`).
fn compute_offset(windows: &[Window], exclude_pid: Option<ProcessId>) -> (f64, f64, bool) {
  if let Some(exclude_pid) = exclude_pid {
    match windows.iter().find(|w| w.process_id.0 == exclude_pid.0) {
      Some(overlay_window) => (overlay_window.bounds.x, overlay_window.bounds.y, false),
      None => (0.0, 0.0, true),
    }
  } else {
    (0.0, 0.0, false)
  }
}

/// Check if any windows are offscreen (indicating space transition).
fn has_offscreen_windows(windows: &[Window], offset_x: f64, screen_width: f64) -> bool {
  windows.iter().any(|w| {
    let adjusted_x = w.bounds.x - offset_x;
    adjusted_x > screen_width + 1.0
  })
}

/// Filter and transform windows based on config.
/// This is the core filtering logic, extracted for testability.
fn filter_windows(
  all_windows: Vec<Window>,
  config: &PollingConfig,
  screen_width: f64,
  screen_height: f64,
) -> PollWindowsResult {
  let (offset_x, offset_y, overlay_missing) = compute_offset(&all_windows, config.exclude_pid);
  let offscreen = has_offscreen_windows(&all_windows, offset_x, screen_width);
  let skip_removal = overlay_missing || offscreen;

  let windows: Vec<Window> = all_windows
    .into_iter()
    .filter(|w| {
      if config
        .exclude_pid
        .is_some_and(|pid| w.process_id.0 == pid.0)
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
      if config.filter_fullscreen && w.bounds.matches_size_at_origin(screen_width, screen_height) {
        return false;
      }
      if config.filter_offscreen && w.bounds.x > screen_width + 1.0 {
        return false;
      }
      true
    })
    .collect();

  PollWindowsResult {
    windows,
    skip_removal,
  }
}

fn poll_windows(options: &PollingConfig) -> PollWindowsResult {
  let all_windows = CurrentPlatform::fetch_windows(None);
  let (screen_width, screen_height) = CurrentPlatform::fetch_screen_size();
  filter_windows(all_windows, options, screen_width, screen_height)
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct PollingConfig {
  pub(crate) exclude_pid: Option<ProcessId>,
  pub(crate) filter_fullscreen: bool,
  pub(crate) filter_offscreen: bool,
  pub(crate) interval_ms: u64,
  pub(crate) use_display_link: bool,
}

impl Default for PollingConfig {
  fn default() -> Self {
    Self {
      exclude_pid: None,
      filter_fullscreen: true,
      filter_offscreen: true,
      interval_ms: DEFAULT_POLLING_INTERVAL_MS,
      use_display_link: false,
    }
  }
}

pub(crate) fn start_polling(allio: Allio, config: PollingConfig) -> PollingHandle {
  #[cfg(target_os = "macos")]
  if config.use_display_link {
    if let Some(handle) = try_start_display_synced_polling(allio.clone(), config) {
      return handle;
    }
    log::warn!("Display link unavailable, falling back to thread-based polling");
  }

  start_thread_polling(allio, config)
}

fn start_thread_polling(allio: Allio, config: PollingConfig) -> PollingHandle {
  let stop_signal = Arc::new(AtomicBool::new(false));
  let stop_signal_clone = Arc::clone(&stop_signal);

  let thread = thread::spawn(move || {
    while !stop_signal_clone.load(Ordering::SeqCst) {
      let loop_start = Instant::now();

      poll_iteration(&allio, &config);

      let elapsed = loop_start.elapsed();
      let target = Duration::from_millis(config.interval_ms);
      if elapsed < target {
        thread::sleep(target - elapsed);
      }
    }
  });

  PollingHandle {
    inner: PollingImpl::Thread {
      stop_signal,
      thread: Some(thread),
    },
  }
}

#[cfg(target_os = "macos")]
fn try_start_display_synced_polling(allio: Allio, config: PollingConfig) -> Option<PollingHandle> {
  let handle = CurrentPlatform::start_display_link(move || {
    poll_iteration(&allio, &config);
  })?;

  Some(PollingHandle {
    inner: PollingImpl::DisplayLink(handle),
  })
}

fn poll_iteration(allio: &Allio, config: &PollingConfig) {
  let pos = CurrentPlatform::fetch_mouse_position();
  allio.sync_mouse(pos);

  let poll_result = poll_windows(config);
  let focused_window_id = poll_result.windows.iter().find(|w| w.focused).map(|w| w.id);

  allio.sync_windows(poll_result.windows, poll_result.skip_removal);
  allio.sync_focused_window(focused_window_id);
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::{Bounds, WindowId};

  /// Helper to create a test window.
  fn make_window(id: u32, pid: u32, x: f64, y: f64, w: f64, h: f64) -> Window {
    Window {
      id: WindowId(id),
      title: format!("Window {id}"),
      app_name: format!("App {pid}"),
      bounds: Bounds { x, y, w, h },
      focused: false,
      process_id: ProcessId(pid),
      z_index: id,
    }
  }

  mod compute_offset_tests {
    use super::*;

    #[test]
    fn no_exclude_pid_returns_zero_offset() {
      let windows = vec![make_window(1, 100, 50.0, 50.0, 800.0, 600.0)];
      let (x, y, missing) = compute_offset(&windows, None);
      assert_eq!(x, 0.0);
      assert_eq!(y, 0.0);
      assert!(!missing);
    }

    #[test]
    fn exclude_pid_found_returns_window_position() {
      let windows = vec![
        make_window(1, 100, 10.0, 20.0, 800.0, 600.0),
        make_window(2, 200, 50.0, 50.0, 400.0, 300.0),
      ];
      let (x, y, missing) = compute_offset(&windows, Some(ProcessId(100)));
      assert_eq!(x, 10.0, "should use window 1's x position");
      assert_eq!(y, 20.0, "should use window 1's y position");
      assert!(!missing);
    }

    #[test]
    fn exclude_pid_not_found_returns_zero_with_missing_flag() {
      let windows = vec![make_window(1, 100, 50.0, 50.0, 800.0, 600.0)];
      let (x, y, missing) = compute_offset(&windows, Some(ProcessId(999)));
      assert_eq!(x, 0.0);
      assert_eq!(y, 0.0);
      assert!(missing, "should flag overlay as missing");
    }
  }

  mod has_offscreen_windows_tests {
    use super::*;

    #[test]
    fn no_offscreen_windows() {
      let windows = vec![
        make_window(1, 100, 0.0, 0.0, 800.0, 600.0),
        make_window(2, 200, 100.0, 100.0, 400.0, 300.0),
      ];
      assert!(!has_offscreen_windows(&windows, 0.0, 1920.0));
    }

    #[test]
    fn window_at_screen_edge_not_offscreen() {
      let windows = vec![make_window(1, 100, 1920.0, 0.0, 800.0, 600.0)];
      // adjusted_x = 1920.0, not > 1921.0
      assert!(!has_offscreen_windows(&windows, 0.0, 1920.0));
    }

    #[test]
    fn window_beyond_screen_is_offscreen() {
      let windows = vec![make_window(1, 100, 1922.0, 0.0, 800.0, 600.0)];
      // adjusted_x = 1922.0 > 1921.0
      assert!(has_offscreen_windows(&windows, 0.0, 1920.0));
    }

    #[test]
    fn offset_adjusts_calculation() {
      let windows = vec![make_window(1, 100, 2000.0, 0.0, 800.0, 600.0)];
      // adjusted_x = 2000.0 - 100.0 = 1900.0, not > 1921.0
      assert!(!has_offscreen_windows(&windows, 100.0, 1920.0));
    }
  }

  mod filter_windows_tests {
    use super::*;

    fn default_config() -> PollingConfig {
      PollingConfig::default()
    }

    #[test]
    fn empty_windows_returns_empty() {
      let result = filter_windows(vec![], &default_config(), 1920.0, 1080.0);
      assert!(result.windows.is_empty());
      assert!(!result.skip_removal);
    }

    #[test]
    fn excludes_pid_from_result() {
      let windows = vec![
        make_window(1, 100, 0.0, 0.0, 800.0, 600.0),
        make_window(2, 200, 100.0, 100.0, 400.0, 300.0),
      ];
      let config = PollingConfig {
        exclude_pid: Some(ProcessId(100)),
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);
      assert_eq!(result.windows.len(), 1);
      assert_eq!(result.windows[0].id.0, 2);
    }

    #[test]
    fn applies_offset_from_excluded_window() {
      let windows = vec![
        make_window(1, 100, 10.0, 20.0, 800.0, 600.0), // Overlay
        make_window(2, 200, 110.0, 120.0, 400.0, 300.0), // Regular window
      ];
      let config = PollingConfig {
        exclude_pid: Some(ProcessId(100)),
        filter_fullscreen: false,
        filter_offscreen: false,
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);

      assert_eq!(result.windows.len(), 1);
      // Window 2 should have offset applied: 110-10=100, 120-20=100
      assert_eq!(result.windows[0].bounds.x, 100.0);
      assert_eq!(result.windows[0].bounds.y, 100.0);
    }

    #[test]
    fn filters_fullscreen_windows() {
      let windows = vec![
        make_window(1, 100, 0.0, 0.0, 1920.0, 1080.0), // Fullscreen
        make_window(2, 200, 100.0, 100.0, 400.0, 300.0), // Normal
      ];
      let config = PollingConfig {
        filter_fullscreen: true,
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);

      assert_eq!(result.windows.len(), 1);
      assert_eq!(result.windows[0].id.0, 2);
    }

    #[test]
    fn does_not_filter_fullscreen_when_disabled() {
      let windows = vec![make_window(1, 100, 0.0, 0.0, 1920.0, 1080.0)];
      let config = PollingConfig {
        filter_fullscreen: false,
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);
      assert_eq!(result.windows.len(), 1);
    }

    #[test]
    fn filters_offscreen_windows() {
      let windows = vec![
        make_window(1, 100, 2000.0, 0.0, 800.0, 600.0), // Offscreen
        make_window(2, 200, 100.0, 100.0, 400.0, 300.0), // Normal
      ];
      let config = PollingConfig {
        filter_offscreen: true,
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);

      assert_eq!(result.windows.len(), 1);
      assert_eq!(result.windows[0].id.0, 2);
    }

    #[test]
    fn skip_removal_when_overlay_missing() {
      let windows = vec![make_window(1, 100, 0.0, 0.0, 800.0, 600.0)];
      let config = PollingConfig {
        exclude_pid: Some(ProcessId(999)), // PID not in windows
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);
      assert!(result.skip_removal, "should skip removal when overlay missing");
    }

    #[test]
    fn skip_removal_when_offscreen_detected() {
      let windows = vec![
        make_window(1, 100, 0.0, 0.0, 800.0, 600.0),
        make_window(2, 200, 3000.0, 0.0, 800.0, 600.0), // Far offscreen
      ];
      let config = PollingConfig {
        filter_offscreen: false, // Don't filter, but still detect
        ..default_config()
      };
      let result = filter_windows(windows, &config, 1920.0, 1080.0);
      assert!(result.skip_removal, "should skip removal during space transition");
    }

    #[test]
    fn no_skip_removal_in_normal_state() {
      let windows = vec![
        make_window(1, 100, 0.0, 0.0, 800.0, 600.0),
        make_window(2, 200, 100.0, 100.0, 400.0, 300.0),
      ];
      let result = filter_windows(windows, &default_config(), 1920.0, 1080.0);
      assert!(!result.skip_removal);
    }
  }

  mod polling_config_tests {
    use super::*;

    #[test]
    fn default_config_values() {
      let config = PollingConfig::default();
      assert_eq!(config.exclude_pid, None);
      assert!(config.filter_fullscreen);
      assert!(config.filter_offscreen);
      assert_eq!(config.interval_ms, DEFAULT_POLLING_INTERVAL_MS);
      assert!(!config.use_display_link);
    }
  }
}
