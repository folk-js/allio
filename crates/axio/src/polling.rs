/*!
Internal polling implementation.

Handles background polling for windows and mouse position.
Consumers don't interact with this directly - polling is owned by `Axio`.
*/

use crate::core::Axio;
use crate::platform;
use crate::types::{AXWindow, ProcessId};
use log::error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

/// Internal implementation of the polling handle.
enum PollingImpl {
  /// Thread-based polling with fixed interval
  Thread {
    stop_signal: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
  },
  /// Display-synchronized polling (macOS only)
  #[cfg(target_os = "macos")]
  DisplayLink(platform::DisplayLinkHandle),
}

/// Internal handle to control polling lifetime.
/// Polling stops when this is dropped.
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

/// Poll for current windows. Returns None if `exclude_pid` window isn't found.
fn poll_windows(options: &AxioOptions) -> Option<Vec<AXWindow>> {
  let all_windows = platform::fetch_windows();

  let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows.iter().find(|w| w.process_id.0 == exclude_pid.0) {
      Some(overlay_window) => (overlay_window.bounds.x, overlay_window.bounds.y),
      None => return None,
    }
  } else {
    (0.0, 0.0)
  };

  let (screen_width, screen_height) = platform::fetch_screen_size();

  let windows: Vec<AXWindow> = all_windows
    .into_iter()
    .filter(|w| {
      // Exclude our own window
      if options
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
      if options.filter_fullscreen && w.bounds.matches_size_at_origin(screen_width, screen_height) {
        return false;
      }
      if options.filter_offscreen && w.bounds.x > screen_width + 1.0 {
        return false;
      }
      true
    })
    .collect();

  Some(windows)
}

/// Configuration options for Axio.
#[derive(Debug, Clone, Copy)]
pub struct AxioOptions {
  /// PID to exclude from tracking. Its window position is used as coordinate offset.
  /// Typically set to your own app's PID for overlay applications.
  pub exclude_pid: Option<ProcessId>,
  /// Filter out fullscreen windows. Default: true.
  pub filter_fullscreen: bool,
  /// Filter out offscreen windows. Default: true.
  pub filter_offscreen: bool,
  /// Polling interval in milliseconds. Default: 8ms (~120fps).
  /// Ignored when `use_display_link` is true.
  pub interval_ms: u64,
  /// Use `CVDisplayLink` for display-synchronized polling (macOS only, experimental).
  /// When true, polling fires exactly once per display refresh (60Hz/120Hz).
  /// Default: false (use fixed interval timer instead).
  pub use_display_link: bool,
}

impl Default for AxioOptions {
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

/// Start background polling. Internal - called by Axio::new().
pub(crate) fn start_polling(axio: Axio, config: AxioOptions) -> PollingHandle {
  #[cfg(target_os = "macos")]
  if config.use_display_link {
    return start_display_synced_polling(axio, config);
  }

  start_thread_polling(axio, config)
}

/// Thread-based polling implementation.
fn start_thread_polling(axio: Axio, config: AxioOptions) -> PollingHandle {
  let stop_signal = Arc::new(AtomicBool::new(false));
  let stop_signal_clone = Arc::clone(&stop_signal);

  let thread = thread::spawn(move || {
    while !stop_signal_clone.load(Ordering::SeqCst) {
      let loop_start = Instant::now();

      poll_iteration(&axio, &config);

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

/// Display-synchronized polling implementation (macOS only).
#[cfg(target_os = "macos")]
fn start_display_synced_polling(axio: Axio, config: AxioOptions) -> PollingHandle {
  let handle = match platform::start_display_link(move || {
    poll_iteration(&axio, &config);
  }) {
    Some(h) => h,
    None => {
      error!("Failed to start display-synced polling");
      std::process::exit(1);
    }
  };

  PollingHandle {
    inner: PollingImpl::DisplayLink(handle),
  }
}

/// Shared polling logic for both thread and display-link implementations.
fn poll_iteration(axio: &Axio, config: &AxioOptions) {
  // Mouse position polling - axio handles dedup and event emission
  if let Some(pos) = platform::fetch_mouse_position() {
    axio.update_mouse_position(pos);
  }

  if let Some(raw_windows) = poll_windows(config) {
    let added_pids = axio.update_windows(raw_windows.clone());

    // Enable accessibility for new windows
    for pid in added_pids {
      platform::enable_accessibility_for_pid(pid.0);
    }

    // Focus tracking - axio emits FocusWindow if value changed
    let focused_window_id = raw_windows.iter().find(|w| w.focused).map(|w| w.id);
    axio.set_focused_window(focused_window_id);
  }
}
