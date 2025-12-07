use crate::element_registry::ElementRegistry;
use crate::platform;
use crate::types::{AXElement, AxioResult, ElementId, Selection, WindowId};

/// Discover element at screen coordinates.
pub fn element_at(x: f64, y: f64) -> AxioResult<AXElement> {
  platform::get_element_at_position(x, y)
}

/// Get cached element by ID.
pub fn get(element_id: &ElementId) -> AxioResult<AXElement> {
  ElementRegistry::get(element_id)
}

/// Get multiple cached elements by ID.
pub fn get_many(element_ids: &[ElementId]) -> Vec<AXElement> {
  ElementRegistry::get_many(element_ids)
}

/// Discover children of element (registers them, updates parent's children).
pub fn children(element_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  platform::discover_children(element_id, max_children)
}

/// Refresh element from platform (re-fetch attributes).
pub fn refresh(element_id: &ElementId) -> AxioResult<AXElement> {
  platform::refresh_element(element_id)
}

/// Get the root element for a window.
/// This is the accessibility element representing the window itself.
pub fn window_root(window_id: &WindowId) -> AxioResult<AXElement> {
  platform::get_window_root(window_id)
}

/// Write text to an element.
pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
  ElementRegistry::write(element_id, text)
}

/// Click an element.
pub fn click(element_id: &ElementId) -> AxioResult<()> {
  ElementRegistry::click(element_id)
}

/// Watch element for changes.
pub fn watch(element_id: &ElementId) -> AxioResult<()> {
  ElementRegistry::watch(element_id)
}

/// Stop watching element.
pub fn unwatch(element_id: &ElementId) {
  ElementRegistry::unwatch(element_id)
}

/// Initialize the AXIO system.
pub fn initialize() {
  // Check accessibility permissions and warn if not granted
  platform::verify_accessibility_permissions();
}

/// Get currently focused element and selection for a given PID.
/// Returns (focused_element, selection).
pub fn get_current_focus(pid: u32) -> (Option<AXElement>, Option<Selection>) {
  platform::get_current_focus(pid)
}
