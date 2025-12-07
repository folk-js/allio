//! Window-related operations for macOS accessibility.
//!
//! This module handles:
//! - Getting window elements for a process
//! - Getting the root element for a window
//! - Finding elements at screen positions
//! - Enabling accessibility for Electron apps
//! - Fetching window handles by bounds matching

use objc2_application_services::AXError;
use objc2_core_foundation::{CFBoolean, CFString};

use crate::platform::handles::ElementHandle;
use crate::types::{AXElement, AxioError, AxioResult, WindowId};

use super::element::build_element_from_handle;
use super::mapping::ax_role;
use super::util::app_element;

// =============================================================================
// Window Elements
// =============================================================================

/// Get all window ElementHandles for a given PID.
pub fn get_window_elements(pid: u32) -> AxioResult<Vec<ElementHandle>> {
  let app_handle = ElementHandle::new(app_element(pid));
  let children = app_handle.get_children();

  let windows = children
    .into_iter()
    .filter(|child| child.get_string("AXRole").as_deref() == Some(ax_role::WINDOW))
    .collect();

  Ok(windows)
}

/// Get the root element for a window.
pub fn get_window_root(window_id: &WindowId) -> AxioResult<AXElement> {
  let (window, handle) = crate::registry::get_window_with_handle(window_id)
    .ok_or_else(|| AxioError::WindowNotFound(*window_id))?;

  let window_handle =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {window_id} has no AX element")))?;

  // Clone handle for safe method use
  build_element_from_handle(window_handle.clone(), window_id, window.process_id.0, None)
    .ok_or_else(|| AxioError::Internal("Window root element was previously destroyed".to_string()))
}

// =============================================================================
// Element at Position
// =============================================================================

/// Get the accessibility element at a specific screen position.
pub fn get_element_at_position(x: f64, y: f64) -> AxioResult<AXElement> {
  let window = crate::registry::find_window_at_point(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No tracked window found at position ({x}, {y})"))
  })?;

  let window_id = window.id;
  let pid = window.process_id.0;

  // Use safe ElementHandle method
  let app_handle = ElementHandle::new(app_element(pid));
  let element_handle = app_handle.element_at_position(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No element found at ({x}, {y}) in app {pid}"))
  })?;

  build_element_from_handle(element_handle, &window_id, pid, None).ok_or_else(|| {
    AxioError::AccessibilityError(format!("Element at ({x}, {y}) was previously destroyed"))
  })
}

// =============================================================================
// Accessibility Enablement
// =============================================================================

/// Enable accessibility for an Electron app.
pub fn enable_accessibility_for_pid(pid: crate::ProcessId) {
  let raw_pid = pid.0;
  let app_el = app_element(raw_pid);
  let attr_name = CFString::from_static_str("AXManualAccessibility");
  let value = CFBoolean::new(true);

  unsafe {
    let result = app_el.set_attribute_value(&attr_name, value);

    if result == AXError::Success {
      log::debug!("Enabled accessibility for PID {raw_pid}");
    } else if result != AXError::AttributeUnsupported {
      log::warn!("Failed to enable accessibility for PID {raw_pid} (error: {result:?})");
    }
  }
}

// =============================================================================
// Window Handle Fetching
// =============================================================================

/// Fetch an element handle for a window by matching bounds.
pub fn fetch_window_handle(window: &crate::AXWindow) -> Option<ElementHandle> {
  let window_elements = get_window_elements(window.process_id.0).ok()?;

  if window_elements.is_empty() {
    return None;
  }

  const MARGIN: f64 = 2.0;

  for element in window_elements.iter() {
    if let Some(element_bounds) = element.get_bounds() {
      if window.bounds.matches(&element_bounds, MARGIN) {
        return Some(element.clone());
      }
    }
  }

  // Fallback: use only element if there's just one
  if window_elements.len() == 1 {
    return Some(window_elements[0].clone());
  }

  None
}

