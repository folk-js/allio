use crate::events::emit;
use crate::platform;
use crate::types::{AXWindow, Event, Point, ProcessId};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

/// Mutable state for polling iteration.
#[derive(Default)]
struct PollingState {
  last_mouse_pos: Option<Point>,
  poll_count: u64,
}

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

/// Handle to control polling.
///
/// Polling runs until this handle is dropped or `stop()` is called.
/// When dropped, polling is automatically stopped.
pub struct PollingHandle {
  inner: PollingImpl,
}

impl PollingHandle {
  /// Signal polling to stop.
  ///
  /// This is non-blocking. Polling will stop on its next iteration.
  pub fn stop(&self) {
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

  /// Check if polling is still running.
  pub fn is_running(&self) -> bool {
    match &self.inner {
      PollingImpl::Thread { thread, .. } => {
        thread.as_ref().map(|t| !t.is_finished()).unwrap_or(false)
      }
      #[cfg(target_os = "macos")]
      PollingImpl::DisplayLink(handle) => handle.is_running(),
    }
  }

  /// Wait for polling to finish.
  ///
  /// This will block until polling stops. Call `stop()` first if you want
  /// to ensure it terminates.
  pub fn join(mut self) {
    if let PollingImpl::Thread { thread, .. } = &mut self.inner {
      if let Some(t) = thread.take() {
        let _ = t.join();
      }
    }
    // DisplayLink stops automatically on drop
  }
}

impl Drop for PollingHandle {
  fn drop(&mut self) {
    self.stop();
    if let PollingImpl::Thread { thread, .. } = &mut self.inner {
      if let Some(t) = thread.take() {
        let _ = t.join();
      }
    }
  }
}

/// Poll for current windows. Returns None if exclude_pid window isn't found.
fn poll_windows(options: &PollingOptions) -> Option<Vec<AXWindow>> {
  let all_windows = platform::enumerate_windows();

  // Find offset from excluded window (e.g., our overlay)
  let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows.iter().find(|w| w.process_id.0 == exclude_pid.0) {
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

/// Window polling configuration.
#[derive(Clone)]
pub struct PollingOptions {
  /// PID to exclude. Its window position is used as coordinate offset.
  pub exclude_pid: Option<ProcessId>,
  /// Filter out fullscreen windows.
  pub filter_fullscreen: bool,
  /// Filter out offscreen windows.
  pub filter_offscreen: bool,
  /// Polling interval in milliseconds (ignored when `use_display_link` is true).
  pub interval_ms: u64,
  /// Use CVDisplayLink for display-synchronized polling (macOS only).
  /// When true, polling fires exactly once per display refresh (60Hz/120Hz).
  /// When false, uses a fixed interval timer.
  /// Ignored on non-macOS platforms.
  pub use_display_link: bool,
}

impl Default for PollingOptions {
  fn default() -> Self {
    Self {
      exclude_pid: None,
      filter_fullscreen: true,
      filter_offscreen: true,
      interval_ms: DEFAULT_POLLING_INTERVAL_MS,
      #[cfg(target_os = "macos")]
      use_display_link: false,
      #[cfg(not(target_os = "macos"))]
      use_display_link: false,
    }
  }
}

/// Start background polling for windows and mouse position.
///
/// Returns a [`PollingHandle`] that controls the polling lifetime.
/// Polling will stop when the handle is dropped or [`PollingHandle::stop`] is called.
///
/// On macOS with `use_display_link: true` (the default), polling is synchronized
/// to the display's refresh rate (60Hz, 120Hz, etc.) for optimal frame alignment.
///
/// # Example
///
/// ```ignore
/// let handle = axio::start_polling(PollingOptions::default());
/// // Polling runs until handle is dropped or stop() is called
/// handle.stop();
/// ```
pub fn start_polling(config: PollingOptions) -> PollingHandle {
  #[cfg(target_os = "macos")]
  if config.use_display_link {
    return start_display_synced_polling(config);
  }

  start_thread_polling(config)
}

/// Thread-based polling implementation.
fn start_thread_polling(config: PollingOptions) -> PollingHandle {
  let stop_signal = Arc::new(AtomicBool::new(false));
  let stop_signal_clone = Arc::clone(&stop_signal);

  // How often to run cleanup (in poll cycles). At 8ms, 1250 cycles ≈ 10 seconds.
  const CLEANUP_INTERVAL: u64 = 1250;

  let thread = thread::spawn(move || {
    let mut state = PollingState::default();

    while !stop_signal_clone.load(Ordering::SeqCst) {
      let loop_start = Instant::now();
      state.poll_count += 1;

      poll_iteration(&config, &mut state, CLEANUP_INTERVAL);

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
fn start_display_synced_polling(config: PollingOptions) -> PollingHandle {
  let state = Arc::new(Mutex::new(PollingState::default()));

  // At 60Hz, 600 cycles ≈ 10 seconds
  const CLEANUP_INTERVAL: u64 = 600;

  let handle = platform::start_display_link(move || {
    let mut state = state.lock();
    state.poll_count += 1;

    poll_iteration(&config, &mut state, CLEANUP_INTERVAL);
  })
  .expect("Failed to start display-synced polling");

  PollingHandle {
    inner: PollingImpl::DisplayLink(handle),
  }
}

/// Shared polling logic for both thread and display-link implementations.
fn poll_iteration(config: &PollingOptions, state: &mut PollingState, cleanup_interval: u64) {
  use crate::registry;

  // Mouse position polling
  if let Some(pos) = platform::get_mouse_position() {
    let changed = state
      .last_mouse_pos
      .is_none_or(|last| pos.moved_from(last, 1.0));
    if changed {
      state.last_mouse_pos = Some(pos);
      emit(Event::MousePosition(pos));
    }
  }

  // Window polling
  if let Some(raw_windows) = poll_windows(config) {
    // Update registry (handles window events internally)
    let added_pids = registry::update_windows(raw_windows.clone());

    // Enable accessibility for new windows
    for pid in added_pids {
      platform::enable_accessibility_for_pid(pid);
    }

    // Focus tracking - registry emits FocusChanged if value changed
    let focused_window_id = raw_windows.iter().find(|w| w.focused).map(|w| w.id);
    registry::set_focused_window(focused_window_id);

    // Periodic cleanup
    if state.poll_count % cleanup_interval == 0 {
      let active_pids: HashSet<ProcessId> = raw_windows.iter().map(|w| w.process_id).collect();
      let _observers_cleaned = registry::cleanup_dead_processes(&active_pids);
    }
  }
}
