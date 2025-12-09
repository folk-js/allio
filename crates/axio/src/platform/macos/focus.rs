/*!
Focus and selection handling for macOS accessibility.

Handles:
- Focus change notifications (builds element, delegates to registry)
- Selection change notifications (builds element, delegates to registry)
- Current focus/selection queries
- Window ID lookup for elements
*/

#![allow(unsafe_code)]
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use objc2_application_services::{AXUIElement, AXValueType};
use objc2_core_foundation::{CFRange, CFRetained, CFString};
use std::ffi::c_void;
use std::ptr::NonNull;

use super::handles::ElementHandle;
use crate::types::WindowId;

use super::element::build_element_from_handle;
use super::util::app_element;

/// Handle focus change notification from callback.
/// Builds the element and delegates to registry for state update and event emission.
pub(super) fn handle_app_focus_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);

  let Some(window_id) = get_window_id_for_handle(&handle, pid) else {
    // Expected when desktop is focused or window not yet tracked
    log::debug!("FocusChanged: no window_id found for PID {pid}, skipping");
    return;
  };

  let Some(ax_element) = build_element_from_handle(handle, window_id, pid, None) else {
    log::warn!("FocusChanged: element build failed for PID {pid}");
    return;
  };

  // Only process focus for elements that self-identify as focused.
  // macOS sends AXFocusedUIElementChanged for intermediate elements during click propagation,
  // but only the actual target has focused=true.
  if ax_element.focused != Some(true) {
    return;
  }

  // Registry handles state update, auto-watch, and event emission
  crate::registry::update_focus(pid, ax_element);
}

/// Handle selection change notification from the unified callback.
/// Builds the element and delegates to registry for state update and event emission.
pub(super) fn handle_app_selection_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);

  let Some(window_id) = get_window_id_for_handle(&handle, pid) else {
    // Expected when desktop is focused or window not yet tracked
    log::debug!("SelectionChanged: no window_id found for PID {pid}, skipping");
    return;
  };

  let Some(ax_element) = build_element_from_handle(handle.clone(), window_id, pid, None) else {
    log::warn!("SelectionChanged: element build failed for PID {pid}");
    return;
  };

  let selected_text = handle.get_string("AXSelectedText").unwrap_or_default();
  let range = if selected_text.is_empty() {
    None
  } else {
    get_selected_text_range(&handle)
  };

  // Registry handles state update and event emission
  crate::registry::update_selection(pid, window_id, ax_element.id, selected_text, range);
}

/// Query the currently focused element and selection for an app.
pub(crate) fn get_current_focus(
  pid: u32,
) -> (
  Option<crate::types::AXElement>,
  Option<crate::types::Selection>,
) {
  let app_handle = ElementHandle::new(app_element(pid));

  let Some(focused_handle) = app_handle.get_element("AXFocusedUIElement") else {
    return (None, None);
  };

  let Some(window_id) = get_window_id_for_handle(&focused_handle, pid) else {
    return (None, None);
  };

  let Some(element) = build_element_from_handle(focused_handle.clone(), window_id, pid, None)
  else {
    return (None, None);
  };

  let selection =
    get_selection_from_handle(&focused_handle).map(|(text, range)| crate::types::Selection {
      element_id: element.id,
      text,
      range,
    });

  (Some(element), selection)
}

/// Get window ID for an `ElementHandle` using hash-based lookup.
/// First checks if element is already registered, then falls back to focused window.
fn get_window_id_for_handle(handle: &ElementHandle, pid: u32) -> Option<WindowId> {
  // First: check if element is already registered (by hash)
  let element_hash = super::element::element_hash(handle);
  if let Some(element) = crate::registry::get_element_by_hash(element_hash) {
    return Some(element.window_id);
  }

  // Fallback: use the currently focused window for this PID
  // This works because focus/selection events only come from the focused app
  crate::registry::get_focused_window_for_pid(pid)
}

/// Get the selected text range from an element handle.
fn get_selected_text_range(handle: &ElementHandle) -> Option<crate::types::TextRange> {
  use objc2_application_services::AXValue as AXValueRef;

  let attr_name = CFString::from_static_str("AXSelectedTextRange");
  let value = handle.get_raw_attr_internal(&attr_name)?;

  let ax_value = value.downcast_ref::<AXValueRef>()?;

  unsafe {
    let mut range = CFRange {
      location: 0,
      length: 0,
    };
    if ax_value.value(
      AXValueType::CFRange,
      NonNull::new((&raw mut range).cast::<c_void>())?,
    ) {
      Some(crate::types::TextRange {
        start: range.location as u32,
        length: range.length as u32,
      })
    } else {
      None
    }
  }
}

/// Get selected text and range from an element handle.
fn get_selection_from_handle(
  handle: &ElementHandle,
) -> Option<(String, Option<crate::types::TextRange>)> {
  let selected_text = handle.get_string("AXSelectedText")?;
  if selected_text.is_empty() {
    return None;
  }
  let range = get_selected_text_range(handle);
  Some((selected_text, range))
}
