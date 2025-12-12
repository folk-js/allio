/*!
Window-related operations for macOS accessibility.

Handles:
- Getting window elements for a process
- Getting the root element for a window
- Finding elements at screen positions
- Enabling accessibility for Electron apps
- Fetching window handles by bounds matching
*/

#![allow(unsafe_code)]

use objc2_application_services::AXError;
use objc2_core_foundation::{CFBoolean, CFString};

use super::handles::ElementHandle;

use super::mapping::ax_role;
use super::util::app_element;

fn get_window_elements(pid: u32) -> Vec<ElementHandle> {
  let app_handle = ElementHandle::new(app_element(pid));
  let children = app_handle.get_children();

  children
    .into_iter()
    .filter(|child| child.get_string("AXRole").as_deref() == Some(ax_role::WINDOW))
    .collect()
}

/// Enable accessibility for Chromium/Electron apps.
pub(crate) fn enable_accessibility_for_pid(pid: crate::ProcessId) {
  let raw_pid = pid.0;
  let app_el = app_element(raw_pid);
  let attr_name = CFString::from_static_str("AXManualAccessibility");
  let value = CFBoolean::new(true);

  unsafe {
    let result = app_el.set_attribute_value(&attr_name, value);

    if result == AXError::Success {
      log::debug!("Enabled accessibility for PID {raw_pid}");
    } else if result != AXError::AttributeUnsupported {
      log::debug!("Failed to enable accessibility for PID {raw_pid} (error: {result:?})");
    }
  }
}

/// Fetch an element handle for a window by matching bounds.
pub(crate) fn fetch_window_handle(window: &crate::Window) -> Option<ElementHandle> {
  const MARGIN: f64 = 2.0;

  let window_elements = get_window_elements(window.process_id.0);

  if window_elements.is_empty() {
    return None;
  }

  for element in &window_elements {
    if let Some(element_bounds) = element.get_bounds() {
      if window.bounds.matches(&element_bounds, MARGIN) {
        return Some(element.clone());
      }
    }
  }

  if window_elements.len() == 1 {
    return window_elements.first().cloned();
  }

  None
}
