/*!
SkyLight framework private API bindings.

SkyLight (formerly CGS/CoreGraphicsServices) provides direct access to
the macOS Window Server. These are undocumented private APIs that may
change between macOS versions.

Reference: https://github.com/FelixKratz/JankyBorders
*/

#![allow(unsafe_code)]
#![allow(non_camel_case_types)]
#![allow(dead_code)]

use objc2_core_foundation::CGRect;
use std::ffi::c_void;

/// Window Server connection ID.
pub(crate) type CGSConnectionID = i32;

/// Window ID (same as CGWindowID).
pub(crate) type CGSWindowID = u32;

/// Space ID (desktop/workspace).
pub(crate) type CGSSpaceID = u64;

// ============================================================================
// Event Constants (from JankyBorders/src/events.h)
// ============================================================================

pub(crate) const EVENT_WINDOW_UPDATE: u32 = 723;
pub(crate) const EVENT_WINDOW_CLOSE: u32 = 804;
pub(crate) const EVENT_WINDOW_MOVE: u32 = 806;
pub(crate) const EVENT_WINDOW_RESIZE: u32 = 807;
pub(crate) const EVENT_WINDOW_REORDER: u32 = 808;
pub(crate) const EVENT_WINDOW_LEVEL: u32 = 811;
pub(crate) const EVENT_WINDOW_UNHIDE: u32 = 815;
pub(crate) const EVENT_WINDOW_HIDE: u32 = 816;
pub(crate) const EVENT_WINDOW_TITLE: u32 = 1322;
pub(crate) const EVENT_WINDOW_CREATE: u32 = 1325;
pub(crate) const EVENT_WINDOW_DESTROY: u32 = 1326;
pub(crate) const EVENT_SPACE_CHANGE: u32 = 1401;
pub(crate) const EVENT_FRONT_CHANGE: u32 = 1508;

/// Data payload for window create/destroy events.
#[repr(C)]
pub(crate) struct WindowSpawnData {
  pub sid: CGSSpaceID,
  pub wid: CGSWindowID,
}

// ============================================================================
// SkyLight FFI Declarations
// ============================================================================

#[link(name = "SkyLight", kind = "framework")]
extern "C" {
  /// Get the main window server connection ID.
  pub(crate) fn SLSMainConnectionID() -> CGSConnectionID;

  /// Register a callback for window server events.
  /// handler: Function pointer to callback
  /// event: Event type constant (EVENT_WINDOW_*)
  /// context: User data passed to callback (typically connection ID)
  pub(crate) fn SLSRegisterNotifyProc(
    handler: *const c_void,
    event: u32,
    context: *mut c_void,
  ) -> i32;

  /// Request notifications for specific windows.
  /// Required to receive per-window events like move/resize.
  pub(crate) fn SLSRequestNotificationsForWindows(
    cid: CGSConnectionID,
    window_list: *const CGSWindowID,
    window_count: i32,
  ) -> i32;

  /// Get window bounds directly from Window Server.
  pub(crate) fn SLSGetWindowBounds(
    cid: CGSConnectionID,
    wid: CGSWindowID,
    frame_out: *mut CGRect,
  ) -> i32;

  /// Get the connection ID that owns a window.
  pub(crate) fn SLSGetWindowOwner(
    cid: CGSConnectionID,
    wid: CGSWindowID,
    owner_cid_out: *mut CGSConnectionID,
  ) -> i32;

  /// Get the PID for a connection ID.
  pub(crate) fn SLSConnectionGetPID(cid: CGSConnectionID, pid_out: *mut i32) -> i32;

  /// Check if a window is visible (ordered in).
  pub(crate) fn SLSWindowIsOrderedIn(
    cid: CGSConnectionID,
    wid: CGSWindowID,
    shown_out: *mut bool,
  ) -> i32;
}

// ============================================================================
// Safe Wrappers
// ============================================================================

/// Get the main window server connection.
pub(crate) fn main_connection_id() -> CGSConnectionID {
  unsafe { SLSMainConnectionID() }
}

/// Get window bounds from Window Server.
pub(crate) fn get_window_bounds(cid: CGSConnectionID, wid: CGSWindowID) -> Option<CGRect> {
  let mut frame = CGRect::default();
  let result = unsafe { SLSGetWindowBounds(cid, wid, &mut frame) };
  if result == 0 {
    Some(frame)
  } else {
    None
  }
}

/// Get the PID that owns a window.
pub(crate) fn get_window_pid(cid: CGSConnectionID, wid: CGSWindowID) -> Option<i32> {
  let mut owner_cid: CGSConnectionID = 0;
  let result = unsafe { SLSGetWindowOwner(cid, wid, &mut owner_cid) };
  if result != 0 {
    return None;
  }

  let mut pid: i32 = 0;
  let result = unsafe { SLSConnectionGetPID(owner_cid, &mut pid) };
  if result == 0 {
    Some(pid)
  } else {
    None
  }
}

/// Request to receive notifications for a list of windows.
pub(crate) fn request_notifications_for_windows(cid: CGSConnectionID, windows: &[CGSWindowID]) {
  if windows.is_empty() {
    return;
  }
  unsafe {
    SLSRequestNotificationsForWindows(cid, windows.as_ptr(), windows.len() as i32);
  }
}

/// Check if a window is visible.
pub(crate) fn is_window_visible(cid: CGSConnectionID, wid: CGSWindowID) -> bool {
  let mut shown = false;
  let result = unsafe { SLSWindowIsOrderedIn(cid, wid, &mut shown) };
  result == 0 && shown
}
