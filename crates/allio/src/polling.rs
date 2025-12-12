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

fn poll_windows(options: &PollingConfig) -> PollWindowsResult {
  let all_windows = CurrentPlatform::fetch_windows(None);

  let (offset_x, offset_y, overlay_missing) = if let Some(exclude_pid) = options.exclude_pid {
    match all_windows.iter().find(|w| w.process_id.0 == exclude_pid.0) {
      Some(overlay_window) => (overlay_window.bounds.x, overlay_window.bounds.y, false),
      None => (0.0, 0.0, true),
    }
  } else {
    (0.0, 0.0, false)
  };

  let (screen_width, screen_height) = CurrentPlatform::fetch_screen_size();

  // Offscreen windows indicate space transition
  let has_offscreen_windows = all_windows.iter().any(|w| {
    let adjusted_x = w.bounds.x - offset_x;
    adjusted_x > screen_width + 1.0
  });

  let skip_removal = overlay_missing || has_offscreen_windows;

  let windows: Vec<Window> = all_windows
    .into_iter()
    .filter(|w| {
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
