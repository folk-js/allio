//! Public API for AXIO operations.

use crate::element_registry::ElementRegistry;
use crate::types::{AXElement, AxioResult, ElementId};

/// Discover element at screen coordinates.
pub fn element_at(x: f64, y: f64) -> AxioResult<AXElement> {
    crate::platform::get_element_at_position(x, y)
}

/// Get cached element by ID.
pub fn get(element_id: &ElementId) -> AxioResult<AXElement> {
    ElementRegistry::get(element_id)
}

/// Get multiple cached elements by ID.
pub fn get_many(element_ids: &[ElementId]) -> Vec<AXElement> {
    ElementRegistry::get_many(element_ids)
}

/// Discover children of element (registers them, updates parent's children_ids).
pub fn children(element_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
    crate::platform::macos::discover_children(element_id, max_children)
}

/// Refresh elements from macOS (re-fetch attributes).
pub fn refresh(element_ids: &[ElementId]) -> AxioResult<Vec<AXElement>> {
    element_ids
        .iter()
        .map(|id| crate::platform::macos::refresh_element(id))
        .collect()
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
    ElementRegistry::initialize();
}
