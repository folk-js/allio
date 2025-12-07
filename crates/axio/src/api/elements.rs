/*! Element operations

All functions for discovering, querying, and interacting with UI elements.
*/

use crate::platform;
use crate::registry;
use crate::types::{AXElement, AxioResult, ElementId, Selection};

/// Discover element at screen coordinates.
pub fn at(x: f64, y: f64) -> AxioResult<AXElement> {
  platform::get_element_at_position(x, y)
}

/// Get cached element by ID.
pub fn get(element_id: &ElementId) -> AxioResult<AXElement> {
  registry::get_element(element_id)
}

/// Get multiple cached elements by ID.
pub fn get_many(element_ids: &[ElementId]) -> Vec<AXElement> {
  registry::get_elements(element_ids)
}

/// Discover children of element (registers them, updates parent's children).
pub fn children(element_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  platform::discover_children(element_id, max_children)
}

/// Refresh element from platform (re-fetch attributes).
pub fn refresh(element_id: &ElementId) -> AxioResult<AXElement> {
  platform::refresh_element(element_id)
}

/// Write text to an element.
pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
  registry::write_element_value(element_id, text)
}

/// Click an element.
pub fn click(element_id: &ElementId) -> AxioResult<()> {
  registry::click_element(element_id)
}

/// Watch element for changes.
pub fn watch(element_id: &ElementId) -> AxioResult<()> {
  registry::watch_element(element_id)
}

/// Stop watching element.
pub fn unwatch(element_id: &ElementId) {
  registry::unwatch_element(element_id)
}

/// Get currently focused element and selection for a given PID.
pub fn focus(pid: u32) -> (Option<AXElement>, Option<Selection>) {
  platform::get_current_focus(pid)
}

/// Get all elements in the registry (for sync).
pub fn all() -> Vec<AXElement> {
  registry::get_all_elements()
}
