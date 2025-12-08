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

/// Fetch and register children of element.
pub fn children(element_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  platform::children(element_id, max_children)
}

/// Fetch and register parent of element (None if element is root).
pub fn parent(element_id: &ElementId) -> AxioResult<Option<AXElement>> {
  platform::parent(element_id)
}

/// Refresh element from platform (re-fetch attributes).
pub fn refresh(element_id: &ElementId) -> AxioResult<AXElement> {
  platform::refresh_element(element_id)
}

/// Write a typed value to an element.
pub fn write(element_id: &ElementId, value: &crate::accessibility::Value) -> AxioResult<()> {
  registry::write_element_value(element_id, value)
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
