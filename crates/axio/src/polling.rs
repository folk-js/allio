/*!
Internal polling implementation.

Handles background polling for windows and mouse position.
Consumers don't interact with this directly - polling is owned by `Axio`.
*/

use crate::core::Axio;
use crate::platform::{CurrentPlatform, DisplayLinkHandle, Platform};
use crate::types::{AXWindow, ProcessId};
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
  DisplayLink(DisplayLinkHandle),
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

/// Result of polling for windows.
pub(crate) struct PollWindowsResult {
  /// Windows to sync (may be empty if we should skip this poll).
  pub windows: Vec<AXWindow>,
  /// If true, skip window removal this poll (transitional state detected).
  pub skip_removal: bool,
}

/// Poll for current windows.
fn poll_windows(options: &AxioOptions) -> PollWindowsResult {
  let all_windows = CurrentPlatform::fetch_windows(None);

  // Get offset from exclude_pid window (typically the overlay app)
  let (offset_x, offset_y, overlay_missing) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows.iter().find(|w| w.process_id.0 == exclude_pid.0) {
      Some(overlay_window) => (overlay_window.bounds.x, overlay_window.bounds.y, false),
      None => {
        // Overlay not visible - we're likely in a different space/fullscreen app
        log::trace!(
          "Overlay window (PID {}) not found - likely in different space, skipping window removal",
          exclude_pid.0
        );
        (0.0, 0.0, true)
      }
    }
  } else {
    (0.0, 0.0, false)
  };

  let (screen_width, screen_height) = CurrentPlatform::fetch_screen_size();

  // Check if ANY window is offscreen (indicates space transition)
  let has_offscreen_windows = all_windows.iter().any(|w| {
    let adjusted_x = w.bounds.x - offset_x;
    adjusted_x > screen_width + 1.0
  });

  // Skip window removal if we're in a transitional state
  let skip_removal = overlay_missing || has_offscreen_windows;

  if has_offscreen_windows && !overlay_missing {
    log::trace!("Offscreen windows detected - likely in space transition, skipping window removal");
  }

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

  PollWindowsResult {
    windows,
    skip_removal,
  }
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
    if let Some(handle) = try_start_display_synced_polling(axio.clone(), config) {
      return handle;
    }
    log::warn!("Display link unavailable, falling back to thread-based polling");
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

/// Try to start display-synchronized polling (macOS only).
/// Returns None if display link is unavailable.
#[cfg(target_os = "macos")]
fn try_start_display_synced_polling(axio: Axio, config: AxioOptions) -> Option<PollingHandle> {
  let handle = CurrentPlatform::start_display_link(move || {
    poll_iteration(&axio, &config);
  })?;

  Some(PollingHandle {
    inner: PollingImpl::DisplayLink(handle),
  })
}

/// Shared polling logic for both thread and display-link implementations.
fn poll_iteration(axio: &Axio, config: &AxioOptions) {
  // Mouse position polling
  let pos = CurrentPlatform::fetch_mouse_position();
  axio.sync_mouse(pos);

  let poll_result = poll_windows(config);

  // Focus tracking (extract before moving windows)
  let focused_window_id = poll_result.windows.iter().find(|w| w.focused).map(|w| w.id);

  // Sync windows (handles add/update/remove + events + process creation)
  // Skip removal if we're in a transitional state (space switching, etc.)
  axio.sync_windows(poll_result.windows, poll_result.skip_removal);

  axio.sync_focused_window(focused_window_id);
}
