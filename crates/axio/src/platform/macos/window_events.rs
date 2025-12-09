/*!
SkyLight window event registration and handling.

Provides event-driven window updates as an alternative to polling.
When a window moves/resizes/creates/destroys, we get notified immediately
by the Window Server instead of discovering it via polling.
*/

#![allow(unsafe_code)]

use super::skylight::{
  self, CGSConnectionID, CGSWindowID, EVENT_WINDOW_CLOSE, EVENT_WINDOW_CREATE,
  EVENT_WINDOW_DESTROY, EVENT_WINDOW_HIDE, EVENT_WINDOW_MOVE, EVENT_WINDOW_REORDER,
  EVENT_WINDOW_RESIZE, EVENT_WINDOW_UNHIDE,
};
use crate::core::Axio;
use crate::platform::{CurrentPlatform, Platform};
use crate::polling::AxioOptions;
use crate::types::WindowId;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

// ============================================================================
// Global State
// ============================================================================

/// Global callback data - SkyLight callbacks don't support per-registration context,
/// so we need to store our state globally.
static CALLBACK_DATA: OnceLock<Arc<CallbackData>> = OnceLock::new();

struct CallbackData {
  axio: Axio,
  options: AxioOptions,
  cid: CGSConnectionID,
  stop_signal: Arc<AtomicBool>,
  /// Windows we're tracking (need to request notifications for new ones)
  tracked_windows: Mutex<HashSet<CGSWindowID>>,
}

// ============================================================================
// Public API
// ============================================================================

/// Handle to stop window event listening.
pub(crate) struct WindowEventsHandle {
  stop_signal: Arc<AtomicBool>,
}

impl WindowEventsHandle {
  pub(crate) fn stop(&self) {
    self.stop_signal.store(true, Ordering::SeqCst);
  }
}

impl Drop for WindowEventsHandle {
  fn drop(&mut self) {
    self.stop();
  }
}

/// Start listening for window server events.
/// Returns a handle that stops listening when dropped.
///
/// NOTE: Only one WindowEventsHandle can be active at a time due to global state.
pub(crate) fn start_window_events(axio: Axio, options: AxioOptions) -> WindowEventsHandle {
  let cid = skylight::main_connection_id();
  let stop_signal = Arc::new(AtomicBool::new(false));

  let callback_data = Arc::new(CallbackData {
    axio,
    options,
    cid,
    stop_signal: Arc::clone(&stop_signal),
    tracked_windows: Mutex::new(HashSet::new()),
  });

  // Store in global (only works once per process)
  drop(CALLBACK_DATA.set(callback_data));

  // Register for events
  register_event_handlers(cid);

  // Do an initial sync to populate tracked windows
  initial_sync();

  log::info!("SkyLight window events started (cid={})", cid);

  WindowEventsHandle { stop_signal }
}

// ============================================================================
// Event Registration
// ============================================================================

fn register_event_handlers(cid: CGSConnectionID) {
  let cid_ptr = cid as isize as *mut c_void;

  unsafe {
    // Window modification events
    skylight::SLSRegisterNotifyProc(
      window_modify_handler as *const c_void,
      EVENT_WINDOW_MOVE,
      cid_ptr,
    );
    skylight::SLSRegisterNotifyProc(
      window_modify_handler as *const c_void,
      EVENT_WINDOW_RESIZE,
      cid_ptr,
    );
    skylight::SLSRegisterNotifyProc(
      window_modify_handler as *const c_void,
      EVENT_WINDOW_REORDER,
      cid_ptr,
    );
    skylight::SLSRegisterNotifyProc(
      window_modify_handler as *const c_void,
      EVENT_WINDOW_HIDE,
      cid_ptr,
    );
    skylight::SLSRegisterNotifyProc(
      window_modify_handler as *const c_void,
      EVENT_WINDOW_UNHIDE,
      cid_ptr,
    );
    skylight::SLSRegisterNotifyProc(
      window_modify_handler as *const c_void,
      EVENT_WINDOW_CLOSE,
      cid_ptr,
    );

    // Window creation/destruction
    skylight::SLSRegisterNotifyProc(
      window_spawn_handler as *const c_void,
      EVENT_WINDOW_CREATE,
      cid_ptr,
    );
    skylight::SLSRegisterNotifyProc(
      window_spawn_handler as *const c_void,
      EVENT_WINDOW_DESTROY,
      cid_ptr,
    );
  }

  log::debug!("SkyLight window event handlers registered");
}

/// Initial window sync - discovers existing windows and requests notifications.
fn initial_sync() {
  let Some(data) = CALLBACK_DATA.get() else {
    return;
  };

  let windows = CurrentPlatform::fetch_windows(None);
  let window_ids: Vec<CGSWindowID> = windows.iter().map(|w| w.id.0).collect();

  // Request notifications for all windows
  skylight::request_notifications_for_windows(data.cid, &window_ids);

  // Track them
  {
    let mut tracked = data.tracked_windows.lock();
    tracked.extend(window_ids.iter().copied());
  }

  // Do initial sync via polling machinery
  do_full_sync(data);

  log::debug!(
    "Initial sync complete: tracking {} windows",
    window_ids.len()
  );
}

// ============================================================================
// Event Handlers
// ============================================================================

/// Handler for window modify events (move, resize, etc.)
/// Called from Window Server thread.
///
/// NOTE: Using unaligned read for safety - SkyLight data may not be aligned.
unsafe extern "C" fn window_modify_handler(
  event: u32,
  wid_ptr: *const u32,
  _data_len: usize,
  cid: CGSConnectionID,
) {
  let start = std::time::Instant::now();

  if wid_ptr.is_null() {
    return;
  }

  let wid = std::ptr::read_unaligned(wid_ptr);

  let Some(data) = CALLBACK_DATA.get() else {
    return;
  };

  if data.stop_signal.load(Ordering::SeqCst) {
    return;
  }

  // Skip our own windows
  if let Some(exclude_pid) = data.options.exclude_pid {
    if let Some(pid) = skylight::get_window_pid(cid, wid) {
      if pid as u32 == exclude_pid.0 {
        return;
      }
    }
  }

  match event {
    EVENT_WINDOW_MOVE | EVENT_WINDOW_RESIZE => {
      // FAST PATH: Just update this window's bounds via SkyLight
      // This avoids the expensive CGWindowListCopyWindowInfo call
      match skylight::get_window_bounds(cid, wid) {
        Some(bounds) => {
          let window_id = WindowId(wid);
          let new_bounds = crate::types::Bounds {
            x: bounds.origin.x,
            y: bounds.origin.y,
            w: bounds.size.width,
            h: bounds.size.height,
          };

          // Update just this window's bounds in the state
          let updated = data.axio.update_window_bounds(window_id, new_bounds);
          eprintln!(
            "[SkyLight] {} wid={} fast_path updated={} bounds=({:.0},{:.0} {:.0}x{:.0})",
            event_name(event),
            wid,
            updated,
            new_bounds.x,
            new_bounds.y,
            new_bounds.w,
            new_bounds.h
          );

          if !updated {
            // Window not in state yet - do full sync to add it
            eprintln!("[SkyLight] Window not in state, falling back to full sync");
            do_full_sync(data);
          }
        }
        None => {
          // SLSGetWindowBounds failed - fall back to full sync
          eprintln!(
            "[SkyLight] {} wid={} SLSGetWindowBounds failed",
            event_name(event),
            wid
          );
          do_full_sync(data);
        }
      }
    }
    EVENT_WINDOW_REORDER | EVENT_WINDOW_UNHIDE => {
      // These need full sync for now (order/visibility changed)
      do_full_sync(data);
    }
    EVENT_WINDOW_HIDE | EVENT_WINDOW_CLOSE => {
      // Window might be hidden/closing - check and potentially remove
      if !skylight::is_window_visible(cid, wid) {
        do_full_sync(data);
      }
    }
    _ => {}
  }
}

/// Handler for window create/destroy events.
/// Called from Window Server thread.
///
/// NOTE: SkyLight passes raw bytes that may not be aligned, so we must use
/// unaligned reads to avoid undefined behavior on ARM64.
unsafe extern "C" fn window_spawn_handler(
  event: u32,
  data_ptr: *const u8,
  data_len: usize,
  cid: CGSConnectionID,
) {
  if data_ptr.is_null() || data_len < 12 {
    return;
  }

  // WindowSpawnData layout: { sid: u64, wid: u32 }
  // Read unaligned to handle arbitrary data alignment from SkyLight
  let wid = std::ptr::read_unaligned(data_ptr.add(8) as *const u32);

  let Some(data) = CALLBACK_DATA.get() else {
    return;
  };

  if data.stop_signal.load(Ordering::SeqCst) {
    return;
  }

  // Skip our own windows
  if let Some(exclude_pid) = data.options.exclude_pid {
    if let Some(pid) = skylight::get_window_pid(cid, wid) {
      if pid as u32 == exclude_pid.0 {
        return;
      }
    }
  }

  log::trace!("Window spawn event {}: wid={}", event_name(event), wid);

  match event {
    EVENT_WINDOW_CREATE => {
      // New window - request notifications and track
      skylight::request_notifications_for_windows(data.cid, &[wid]);
      data.tracked_windows.lock().insert(wid);

      // Full sync to pick up the new window
      do_full_sync(data);
    }
    EVENT_WINDOW_DESTROY => {
      data.tracked_windows.lock().remove(&wid);

      // Remove from Axio state
      let window_id = WindowId(wid);
      data.axio.write(|state| {
        state.remove_window(window_id);
      });
    }
    _ => {}
  }
}

// ============================================================================
// Helpers
// ============================================================================

/// Perform a full window sync using the polling infrastructure.
fn do_full_sync(data: &CallbackData) {
  crate::polling::do_poll_iteration(&data.axio, &data.options);
}

/// Get human-readable event name for logging.
fn event_name(event: u32) -> &'static str {
  match event {
    EVENT_WINDOW_MOVE => "MOVE",
    EVENT_WINDOW_RESIZE => "RESIZE",
    EVENT_WINDOW_REORDER => "REORDER",
    EVENT_WINDOW_HIDE => "HIDE",
    EVENT_WINDOW_UNHIDE => "UNHIDE",
    EVENT_WINDOW_CLOSE => "CLOSE",
    EVENT_WINDOW_CREATE => "CREATE",
    EVENT_WINDOW_DESTROY => "DESTROY",
    _ => "UNKNOWN",
  }
}
