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
use crate::core::Axio;
use crate::types::{AXElement, AxioError, AxioResult, WindowId};

use super::element::build_and_register_element;
use super::mapping::ax_role;
use super::util::app_element;

/// Get all window `ElementHandles` for a given PID.
fn get_window_elements(pid: u32) -> Vec<ElementHandle> {
  let app_handle = ElementHandle::new(app_element(pid));
  let children = app_handle.get_children();

  children
    .into_iter()
    .filter(|child| child.get_string("AXRole").as_deref() == Some(ax_role::WINDOW))
    .collect()
}

/// Get the root element for a window.
pub(crate) fn get_window_root(axio: &Axio, window_id: WindowId) -> AxioResult<AXElement> {
  let (window, handle) = axio
    .get_window_with_handle(window_id)
    .ok_or(AxioError::WindowNotFound(window_id))?;

  let window_handle =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {window_id} has no AX element")))?;

  build_and_register_element(axio, window_handle, window_id, window.process_id.0, None)
    .ok_or_else(|| AxioError::Internal("Window root element was previously destroyed".to_string()))
}

/// Retry delays (in ms) for Chromium/Electron lazy accessibility initialization.
///
/// Chromium/Electron apps lazily build their accessibility spatial index on a per-region
/// basis. The first hit test at any coordinate triggers async initialization of that region,
/// returning a window-sized fallback container. Subsequent queries return the actual element.
///
/// We retry with increasing delays to give Chromium time to process:
/// - Attempt 0: Immediate (often returns fallback)
/// - Attempt 1: 10ms delay (usually sufficient for Chromium to initialize)
/// - Attempt 2: 25ms delay
const HIT_TEST_RETRY_DELAYS_MS: [u64; 3] = [0, 10, 25];

/// Get the accessibility element at a specific screen position.
pub(crate) fn get_element_at_position(axio: &Axio, x: f64, y: f64) -> AxioResult<AXElement> {
  const MAX_DEPTH: u8 = 10;

  let window = axio.find_window_at_point(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No tracked window found at position ({x}, {y})"))
  })?;

  let window_id = window.id;
  let pid = window.process_id.0;

  let app_handle = ElementHandle::new(app_element(pid));

  let mut element_handle = None;
  let mut fallback_container = None;

  for &delay_ms in &HIT_TEST_RETRY_DELAYS_MS {
    if delay_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    let Some(hit) = app_handle.element_at_position(x, y) else {
      continue;
    };

    let attrs = hit.get_attributes(None);
    let is_fallback_container = attrs.role.as_deref() == Some("AXGroup")
      && attrs
        .bounds
        .as_ref()
        .is_some_and(|b| b.matches(&window.bounds, 0.0));

    if is_fallback_container {
      fallback_container = Some(hit);
      continue;
    }

    element_handle = Some(hit);
    break;
  }

  // Use real element if found, otherwise fall back to container (better than nothing)
  let mut element_handle = element_handle.or(fallback_container).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No element found at ({x}, {y}) in app {pid}"))
  })?;

  // Try recursive hit testing - drill down through nested containers
  let raw_attrs = element_handle.get_attributes(None);
  for _ in 1..=MAX_DEPTH {
    let Some(deeper) = element_handle.element_at_position(x, y) else {
      break;
    };

    let deeper_attrs = deeper.get_attributes(None);
    let same_element = deeper_attrs.bounds == raw_attrs.bounds
      && deeper_attrs.role == raw_attrs.role
      && deeper_attrs.title == raw_attrs.title;

    if same_element {
      break;
    }

    element_handle = deeper;
  }

  build_and_register_element(axio, element_handle, window_id, pid, None).ok_or_else(|| {
    AxioError::AccessibilityError(format!("Element at ({x}, {y}) was previously destroyed"))
  })
}

/// Enable accessibility (mostly for Chromium/Electron apps)
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

/// Fetch an element handle for a window by matching bounds
/// TODO: find a way to not do this...
pub(crate) fn fetch_window_handle(window: &crate::AXWindow) -> Option<ElementHandle> {
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

  // Fallback: use only element if there's just one
  if window_elements.len() == 1 {
    return window_elements.first().cloned();
  }

  None
}
