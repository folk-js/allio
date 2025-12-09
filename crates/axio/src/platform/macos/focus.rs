/*!
Focus and selection handling for macOS accessibility.

Handles:
- Focus change notifications (builds element, delegates to Axio)
- Selection change notifications (builds element, delegates to Axio)
- Window ID lookup for elements
*/

#![allow(unsafe_code)]
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use objc2_application_services::AXUIElement;
use objc2_core_foundation::CFRetained;

use super::handles::ElementHandle;
use crate::core::Axio;
use crate::platform::element_ops;
use crate::types::WindowId;

use super::element::element_hash;

/// Handle focus change notification from callback.
/// Builds the element and delegates to Axio for state update and event emission.
pub(super) fn handle_app_focus_changed(axio: &Axio, pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);

  let Some(window_id) = get_window_id_for_handle(axio, &handle, pid) else {
    log::debug!("FocusChanged: no window_id found for PID {pid}, skipping");
    return;
  };

  let Some(ax_element) =
    element_ops::build_and_register_element(axio, handle, window_id, pid, None)
  else {
    log::warn!("FocusChanged: element build failed for PID {pid}");
    return;
  };

  // Only process focus for elements that self-identify as focused.
  if ax_element.focused != Some(true) {
    return;
  }

  axio.on_focus_changed(pid, ax_element);
}

/// Handle selection change notification from the unified callback.
/// Builds the element and delegates to Axio for state update and event emission.
pub(super) fn handle_app_selection_changed(
  axio: &Axio,
  pid: u32,
  element: CFRetained<AXUIElement>,
) {
  let handle = ElementHandle::new(element);

  let Some(window_id) = get_window_id_for_handle(axio, &handle, pid) else {
    log::debug!("SelectionChanged: no window_id found for PID {pid}, skipping");
    return;
  };

  let Some(ax_element) =
    element_ops::build_and_register_element(axio, handle.clone(), window_id, pid, None)
  else {
    log::warn!("SelectionChanged: element build failed for PID {pid}");
    return;
  };

  let selected_text = handle.get_string("AXSelectedText").unwrap_or_default();
  let range = if selected_text.is_empty() {
    None
  } else {
    get_selected_text_range(&handle)
  };

  axio.on_selection_changed(pid, window_id, ax_element.id, selected_text, range);
}

/// Get window ID for an `ElementHandle` using hash-based lookup.
fn get_window_id_for_handle(axio: &Axio, handle: &ElementHandle, pid: u32) -> Option<WindowId> {
  let hash = element_hash(handle);
  if let Some(element) = axio.get_element_by_hash(hash) {
    return Some(element.window_id);
  }
  axio.get_focused_window_for_pid(pid)
}

/// Get selected text and range from an element handle.
pub(super) fn get_selection_from_handle(handle: &ElementHandle) -> Option<(String, Option<(u32, u32)>)> {
  let selected_text = handle.get_string("AXSelectedText")?;
  if selected_text.is_empty() {
    return None;
  }
  let range = get_selected_text_range(handle);
  Some((selected_text, range))
}

/// Get the selected text range from an element handle.
fn get_selected_text_range(handle: &ElementHandle) -> Option<(u32, u32)> {
  use objc2_application_services::{AXValue as AXValueRef, AXValueType};
  use objc2_core_foundation::{CFRange, CFString};
  use std::ffi::c_void;
  use std::ptr::NonNull;

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
      let start = range.location as u32;
      let end = (range.location + range.length) as u32;
      Some((start, end))
    } else {
      None
    }
  }
}
