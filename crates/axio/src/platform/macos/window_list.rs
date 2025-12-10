/*! Window enumeration for macOS.

Uses `CGWindowListCopyWindowInfo` to enumerate on-screen windows.
*/

#![allow(unsafe_code)]
#![allow(
  clippy::cast_possible_truncation,
  clippy::cast_sign_loss,
  clippy::cast_possible_wrap
)]

use super::cf_utils::{
  get_cf_boolean, get_cf_number, get_cf_string, get_cf_window_bounds, retain_cf_dictionary,
};
use crate::types::{Window, Bounds, ProcessId, WindowId};
use objc2_app_kit::NSRunningApplication;
use objc2_core_foundation::{CFArray, CFDictionary};
use objc2_core_graphics::{kCGNullWindowID, CGWindowListCopyWindowInfo, CGWindowListOption};

/// Bundle IDs to always filter out (system UI).
const FILTERED_BUNDLE_IDS: &[&str] = &[
  "com.apple.dock",
  "com.apple.screencaptureui",
  "com.apple.screenshot.launcher",
  "com.apple.ScreenContinuity",
];

/// Enumerate all on-screen windows.
/// Returns windows in z-order (frontmost first).
/// Filters out system UI windows.
pub(crate) fn enumerate_windows() -> Vec<Window> {
  // IMPORTANT: Wrap in autorelease pool to prevent memory leaks.
  objc2::rc::autoreleasepool(|_pool| enumerate_windows_inner())
}

fn enumerate_windows_inner() -> Vec<Window> {
  let mut windows = Vec::new();
  // Track which PIDs we've already seen a window for (to mark only frontmost as focused)
  let mut seen_active_pid: Option<u32> = None;

  let option = CGWindowListOption::OptionOnScreenOnly
    | CGWindowListOption::ExcludeDesktopElements
    | CGWindowListOption::OptionIncludingWindow;

  let Some(window_list_info) = CGWindowListCopyWindowInfo(option, kCGNullWindowID) else {
    return windows;
  };

  let windows_count = CFArray::count(&window_list_info);

  for idx in 0..windows_count {
    let window_cf_dictionary_ref =
      unsafe { CFArray::value_at_index(&window_list_info, idx).cast::<CFDictionary>() };

    let Some(dict) = retain_cf_dictionary(window_cf_dictionary_ref) else {
      continue;
    };

    if !get_cf_boolean(&dict, "kCGWindowIsOnscreen") {
      continue;
    }

    let window_layer = get_cf_number(&dict, "kCGWindowLayer");
    if !(0..=100).contains(&window_layer) {
      continue;
    }

    // Must have valid bounds
    let Some(cg_bounds) = get_cf_window_bounds(&dict) else {
      continue;
    };

    if cg_bounds.size.height < 50.0 || cg_bounds.size.width < 50.0 {
      continue;
    }

    // Must have valid PID
    let process_id = get_cf_number(&dict, "kCGWindowOwnerPID");
    if process_id == 0 {
      continue;
    }

    let Some(app) = get_running_application(process_id as u32) else {
      continue;
    };

    if let Some(bundle_id) = get_bundle_identifier(app) {
      if FILTERED_BUNDLE_IDS.contains(&bundle_id.as_str()) {
        continue;
      }
    }

    let app_is_active = app.isActive();

    // Only mark the FIRST (frontmost) window of the active app as focused.
    // CGWindowListCopyWindowInfo returns windows in z-order, so the first
    // window we see from an active app is the focused one.
    let focused = if app_is_active && seen_active_pid.is_none() {
      seen_active_pid = Some(process_id as u32);
      true
    } else {
      false
    };

    let app_name = get_cf_string(&dict, "kCGWindowOwnerName");
    let title = get_cf_string(&dict, "kCGWindowName");
    let id = get_cf_number(&dict, "kCGWindowNumber");
    let z_index = windows.len() as u32;

    windows.push(Window {
      id: WindowId::from(id as u32),
      title,
      app_name,
      bounds: Bounds {
        x: cg_bounds.origin.x,
        y: cg_bounds.origin.y,
        w: cg_bounds.size.width,
        h: cg_bounds.size.height,
      },
      focused,
      process_id: ProcessId::from(process_id as u32),
      z_index,
    });
  }

  windows
}

fn get_bundle_identifier(app: &NSRunningApplication) -> Option<String> {
  app.bundleIdentifier().map(|s| s.to_string())
}

fn get_running_application(process_id: u32) -> Option<&'static NSRunningApplication> {
  let app: *mut NSRunningApplication = unsafe {
    objc2::msg_send![
        objc2::class!(NSRunningApplication),
        runningApplicationWithProcessIdentifier: process_id as i32
    ]
  };
  if app.is_null() {
    None
  } else {
    Some(unsafe { &*app })
  }
}
