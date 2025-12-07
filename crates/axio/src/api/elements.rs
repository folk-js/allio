//! Element operations
//!
//! All functions for discovering, querying, and interacting with UI elements.

use crate::element_registry::ElementRegistry;
use crate::platform;
use crate::types::{AXElement, AxioResult, ElementId, Selection};

/// Discover element at screen coordinates.
pub fn at(x: f64, y: f64) -> AxioResult<AXElement> {
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

/// Get currently focused element and selection for a given PID.
pub fn focus(pid: u32) -> (Option<AXElement>, Option<Selection>) {
  platform::get_current_focus(pid)
}

/// Get all elements in the registry (for sync).
pub fn all() -> Vec<AXElement> {
  ElementRegistry::get_all()
}
