/*!
Internal polling implementation.

Handles background polling for windows and mouse position.
Consumers don't interact with this directly - polling is owned by `Axio`.

Two modes are supported:
- Thread-based polling: Polls at a fixed interval (default 8ms)
- Event-driven (macOS): Uses SkyLight window server notifications for instant updates
*/

use crate::core::Axio;
use crate::platform::{CurrentPlatform, DisplayLinkHandle, Platform};
use crate::types::{AXWindow, ProcessId};
use log::error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use crate::platform::macos::window_events::{self, WindowEventsHandle};

const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

/// Internal implementation of the polling handle.
enum PollingImpl {
  /// Thread-based polling with fixed interval
  Thread {
    stop_signal: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
  },
  /// Event-driven via SkyLight window server notifications (macOS only).
  /// Uses display link only for mouse position at vsync cadence.
  #[cfg(target_os = "macos")]
  WindowEvents {
    events_handle: WindowEventsHandle,
    /// Display link for mouse position polling (windows are event-driven)
    mouse_display_link: Option<DisplayLinkHandle>,
  },
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
      PollingImpl::WindowEvents {
        events_handle,
        mouse_display_link,
      } => {
        events_handle.stop();
        if let Some(dl) = mouse_display_link {
          dl.stop();
        }
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
  let all_windows = CurrentPlatform::fetch_windows(None);

  let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows.iter().find(|w| w.process_id.0 == exclude_pid.0) {
      Some(overlay_window) => (overlay_window.bounds.x, overlay_window.bounds.y),
      None => return None,
    }
  } else {
    (0.0, 0.0)
  };

  let (screen_width, screen_height) = CurrentPlatform::fetch_screen_size();

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
  /// Only used when `use_display_link` is false.
  pub interval_ms: u64,
  /// Use event-driven window updates via SkyLight (macOS only).
  /// When true, window updates are received instantly from the Window Server
  /// instead of being discovered via polling. Mouse position still polls at vsync.
  /// Default: true.
  pub use_display_link: bool,
}

impl Default for AxioOptions {
  fn default() -> Self {
    Self {
      exclude_pid: None,
      filter_fullscreen: true,
      filter_offscreen: true,
      interval_ms: DEFAULT_POLLING_INTERVAL_MS,
      use_display_link: true,
    }
  }
}

/// Start background polling. Internal - called by Axio::new().
pub(crate) fn start_polling(axio: Axio, config: AxioOptions) -> PollingHandle {
  #[cfg(target_os = "macos")]
  if config.use_display_link {
    return start_event_driven(axio, config);
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

/// Event-driven implementation using SkyLight window server notifications (macOS only).
///
/// Window updates come via SkyLight notifications (instant, no polling).
/// Mouse position still uses display link for vsync-synchronized updates.
#[cfg(target_os = "macos")]
fn start_event_driven(axio: Axio, config: AxioOptions) -> PollingHandle {
  // Start SkyLight event listening for window updates
  let events_handle = window_events::start_window_events(axio.clone(), config);

  // Use display link for mouse position polling (synced to vsync)
  let mouse_display_link = CurrentPlatform::start_display_link({
    let axio = axio.clone();
    move || {
      let pos = CurrentPlatform::fetch_mouse_position();
      axio.sync_mouse(pos);
    }
  });

  if mouse_display_link.is_none() {
    error!("Failed to start display link for mouse polling");
  }

  PollingHandle {
    inner: PollingImpl::WindowEvents {
      events_handle,
      mouse_display_link,
    },
  }
}

/// Shared polling logic - performs a full sync of windows and mouse.
fn poll_iteration(axio: &Axio, config: &AxioOptions) {
  // Mouse position polling
  let pos = CurrentPlatform::fetch_mouse_position();
  axio.sync_mouse(pos);

  if let Some(windows) = poll_windows(config) {
    // Sync windows (handles add/update/remove + events + process creation)
    axio.sync_windows(windows.clone());

    // Focus tracking
    let focused_window_id = windows.iter().find(|w| w.focused).map(|w| w.id);
    axio.sync_focused_window(focused_window_id);
  }
}

/// Perform a poll iteration. Exposed for event-driven mode to trigger full syncs.
pub(crate) fn do_poll_iteration(axio: &Axio, config: &AxioOptions) {
  if let Some(windows) = poll_windows(config) {
    // Sync windows (handles add/update/remove + events + process creation)
    axio.sync_windows(windows.clone());

    // Focus tracking
    let focused_window_id = windows.iter().find(|w| w.focused).map(|w| w.id);
    axio.sync_focused_window(focused_window_id);
  }
}
