/*!
AXIO - Accessibility I/O Layer
```ignore
use axio::{elements, windows, AXElement};

let all_windows = windows::all();
let element = elements::at(100.0, 200.0)?;
let children = elements::children(&element.id, 100)?;
```
*/

// Internal modules
mod events;
mod platform;
mod polling;
mod registry;

pub mod accessibility;

mod types;
pub use types::*;

pub use events::subscribe;
pub use polling::{PollingHandle, PollingOptions};

/// Check if accessibility permissions are granted.
pub fn verify_permissions() -> bool {
  platform::check_accessibility_permissions()
}

/// Get a snapshot of the current registry state for sync.
pub fn snapshot() -> Snapshot {
  registry::snapshot()
}

/// Start background polling for windows and mouse position.
///
/// Returns a [`PollingHandle`] that controls the polling lifetime.
/// Polling will stop when the handle is dropped or [`PollingHandle::stop`] is called.
pub fn start_polling(config: PollingOptions) -> PollingHandle {
  polling::start_polling(config)
}

/// Element operations: discovering, querying, and interacting with UI elements.
pub mod elements {
  use crate::platform;
  use crate::registry;
  use crate::types::{AXElement, AxioResult, ElementId, WindowId};

  /// Discover element at screen coordinates.
  pub fn at(x: f64, y: f64) -> AxioResult<AXElement> {
    platform::get_element_at_position(x, y)
  }

  /// Get cached element by ID. Returns None if not in registry.
  pub fn get(element_id: ElementId) -> Option<AXElement> {
    registry::get_element(element_id)
  }

  /// Get multiple cached elements by ID.
  pub fn get_many(element_ids: &[ElementId]) -> Vec<AXElement> {
    registry::get_elements(element_ids)
  }

  /// Fetch and register children of element.
  pub fn children(element_id: ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
    platform::children(element_id, max_children)
  }

  /// Fetch and register parent of element (None if element is root).
  pub fn parent(element_id: ElementId) -> AxioResult<Option<AXElement>> {
    platform::parent(element_id)
  }

  /// Refresh element from platform (re-fetch attributes).
  pub fn refresh(element_id: ElementId) -> AxioResult<AXElement> {
    platform::refresh_element(element_id)
  }

  /// Write a typed value to an element.
  pub fn write(element_id: ElementId, value: &crate::accessibility::Value) -> AxioResult<()> {
    registry::write_element_value(element_id, value)
  }

  /// Click an element.
  pub fn click(element_id: ElementId) -> AxioResult<()> {
    registry::click_element(element_id)
  }

  /// Watch element for changes.
  pub fn watch(element_id: ElementId) -> AxioResult<()> {
    registry::watch_element(element_id)
  }

  /// Stop watching element.
  pub fn unwatch(element_id: ElementId) -> AxioResult<()> {
    registry::unwatch_element(element_id)
  }

  /// Get the root element for a window.
  pub fn window(window_id: WindowId) -> AxioResult<AXElement> {
    platform::get_window_root(window_id)
  }

  /// Get all elements in the registry.
  pub fn all() -> Vec<AXElement> {
    registry::get_all_elements()
  }
}

/// Window operations: querying windows and their state.
pub mod windows {
  use crate::platform;
  use crate::registry;
  use crate::types::{AXElement, AXWindow, AxioResult, TextSelection, WindowId};

  /// Get all current windows.
  pub fn all() -> Vec<AXWindow> {
    registry::get_windows()
  }

  /// Get a specific window by ID.
  pub fn get(window_id: WindowId) -> Option<AXWindow> {
    registry::get_window(window_id)
  }

  /// Get the focused window ID (None if desktop is focused).
  pub fn focused_id() -> Option<WindowId> {
    registry::get_focused_window()
  }

  /// Get window IDs in depth order (front to back).
  pub fn depth_order() -> Vec<WindowId> {
    registry::get_depth_order()
  }

  /// Get screen dimensions (width, height) - the coordinate space windows exist in.
  pub fn screen_size() -> (f64, f64) {
    platform::get_main_screen_dimensions()
  }

  /// Get currently focused element and text selection for a window.
  pub fn focus(window_id: WindowId) -> AxioResult<(Option<AXElement>, Option<TextSelection>)> {
    let window = registry::get_window(window_id)
      .ok_or_else(|| crate::types::AxioError::WindowNotFound(window_id))?;
    Ok(platform::get_current_focus(window.process_id.0))
  }
}
