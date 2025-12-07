//! Focus and selection handling for macOS accessibility.
//!
//! This module handles:
//! - Focus change notifications
//! - Selection change notifications
//! - Current focus/selection queries
//! - Window ID lookup for elements

use objc2_application_services::{AXUIElement, AXValueType};
use objc2_core_foundation::{CFRange, CFRetained, CFString};
use std::ffi::c_void;
use std::ptr::NonNull;

use crate::events::emit;
use crate::platform::handles::ElementHandle;
use crate::types::{Event, WindowId};

use super::element::build_element_from_handle;
use super::util::app_element;

// =============================================================================
// Focus Change Handling
// =============================================================================

/// Check if a role should be auto-watched when focused.
pub fn should_auto_watch(role: &crate::accessibility::Role) -> bool {
  role.auto_watch_on_focus() || role.is_writable()
}

/// Handle focus change notification from the unified callback.
pub fn handle_app_focus_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);
  let window_id = match get_window_id_for_handle(&handle, pid) {
    Some(id) => id,
    None => {
      // Expected when desktop is focused or window not yet tracked
      log::debug!("FocusChanged: no window_id found for PID {}, skipping", pid);
      return;
    }
  };

  let Some(ax_element) = build_element_from_handle(handle, &window_id, pid, None) else {
    log::warn!("FocusChanged: element build failed for PID {}", pid);
    return;
  };

  let new_is_watchable = should_auto_watch(&ax_element.role);

  // Update focus in registry, get previous
  let previous_element_id = crate::registry::set_process_focus(pid, ax_element.id);
  let same_element = previous_element_id.as_ref() == Some(&ax_element.id);

  // Auto-watch/unwatch based on role
  if !same_element {
    if let Some(ref prev_id) = previous_element_id {
      // Check if previous was watchable before unwatching
      if let Ok(prev_elem) = crate::registry::get_element(prev_id) {
        if should_auto_watch(&prev_elem.role) {
          crate::registry::unwatch_element(prev_id);
        }
      }
    }

    if new_is_watchable {
      let _ = crate::registry::watch_element(&ax_element.id);
    }
  }

  emit(Event::FocusElement {
    element: ax_element,
    previous_element_id,
  });
}

// =============================================================================
// Selection Change Handling
// =============================================================================

/// Handle selection change notification from the unified callback.
pub fn handle_app_selection_changed(pid: u32, element: CFRetained<AXUIElement>) {
  let handle = ElementHandle::new(element);
  let window_id = match get_window_id_for_handle(&handle, pid) {
    Some(id) => id,
    None => {
      // Expected when desktop is focused or window not yet tracked
      log::debug!(
        "SelectionChanged: no window_id found for PID {}, skipping",
        pid
      );
      return;
    }
  };

  let Some(ax_element) = build_element_from_handle(handle.clone(), &window_id, pid, None) else {
    log::warn!("SelectionChanged: element build failed for PID {}", pid);
    return;
  };

  let selected_text = handle.get_string("AXSelectedText").unwrap_or_default();
  let range = if selected_text.is_empty() {
    None
  } else {
    get_selected_text_range(&handle)
  };

  emit(Event::SelectionChanged {
    window_id,
    element_id: ax_element.id,
    text: selected_text,
    range,
  });
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
      NonNull::new(&mut range as *mut _ as *mut c_void)?,
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

// =============================================================================
// Current Focus Query
// =============================================================================

/// Query the currently focused element and selection for an app.
pub fn get_current_focus(
  pid: u32,
) -> (
  Option<crate::types::AXElement>,
  Option<crate::types::Selection>,
) {
  // Create ElementHandle for app element
  let app_handle = ElementHandle::new(app_element(pid));

  // Use safe ElementHandle method to get focused element
  let Some(focused_handle) = app_handle.get_element("AXFocusedUIElement") else {
    return (None, None);
  };

  let window_id = match get_window_id_for_handle(&focused_handle, pid) {
    Some(id) => id,
    None => return (None, None),
  };

  let Some(element) = build_element_from_handle(focused_handle.clone(), &window_id, pid, None)
  else {
    return (None, None); // Element was previously destroyed
  };

  // Get selection using handle method
  let selection =
    get_selection_from_handle(&focused_handle).map(|(text, range)| crate::types::Selection {
      element_id: element.id,
      text,
      range,
    });

  (Some(element), selection)
}

// =============================================================================
// Window ID Lookup
// =============================================================================

/// Get window ID for an ElementHandle using hash-based lookup.
/// First checks if element is already registered, then falls back to focused window.
pub fn get_window_id_for_handle(handle: &ElementHandle, pid: u32) -> Option<WindowId> {
  // First: check if element is already registered (by hash)
  let element_hash = super::element::element_hash(handle);
  if let Some(element) = crate::registry::get_element_by_hash(element_hash) {
    return Some(element.window_id);
  }

  // Fallback: use the currently focused window for this PID
  // This works because focus/selection events only come from the focused app
  crate::registry::get_focused_window_for_pid(pid)
}

/// Get selected text and range from an element handle.
fn get_selection_from_handle(
  handle: &ElementHandle,
) -> Option<(String, Option<crate::types::TextRange>)> {
  let selected_text = handle.get_string("AXSelectedText")?;
  if selected_text.is_empty() {
    return None;
  }
  // TODO: Parse AXSelectedTextRange if needed
  Some((selected_text, None))
}
