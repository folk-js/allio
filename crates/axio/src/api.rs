//! Public API for AXIO operations.

use crate::element_registry::ElementRegistry;
use crate::types::{AXNode, AxioResult, ElementId};

/// Get the deepest accessibility element at screen coordinates.
pub fn element_at(x: f64, y: f64) -> AxioResult<AXNode> {
    crate::platform::get_element_at_position(x, y)
}

/// Get the accessibility tree rooted at an element.
/// `max_depth`: 0 = just this element, 1 = immediate children, etc.
pub fn tree(
    element_id: &ElementId,
    max_depth: usize,
    max_children: usize,
) -> AxioResult<Vec<AXNode>> {
    crate::platform::macos::get_children_by_element_id(&element_id.0, max_depth, max_children)
}

/// Watch an element for changes (value, title, destruction).
pub fn watch(element_id: &ElementId) -> AxioResult<()> {
    ElementRegistry::watch(element_id)
}

/// Stop watching an element.
pub fn unwatch(element_id: &ElementId) {
    ElementRegistry::unwatch(element_id)
}

/// Write text to an element. Only works on text fields.
pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
    ElementRegistry::write(element_id, text)
}

/// Click an element.
pub fn click(element_id: &ElementId) -> AxioResult<()> {
    crate::platform::click_element_by_id(&element_id.0)
}

/// Initialize the AXIO system. Must be called once at startup.
pub fn initialize() {
    ElementRegistry::initialize();
}
