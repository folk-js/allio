//! AXIO Public API
//!
//! This module provides the clean public interface for AXIO operations.
//! It wraps the internal complexity of ElementRegistry, WindowManager, etc.
//!
//! Design goals:
//! - Clear, idiomatic Rust API
//! - Type-safe (uses ElementId, WindowId, AxioError)
//! - Suitable for extraction into a standalone crate

#![allow(dead_code)] // API surface - not all functions used internally yet

use crate::element_registry::ElementRegistry;
use crate::types::{AXNode, AxioError, AxioResult, ElementId};

// ============================================================================
// Public API
// ============================================================================

/// Get an accessibility element at a screen position (hit test)
///
/// Returns the deepest element at the given coordinates.
/// The returned element has minimal fields populated (id, role, bounds).
///
/// # Example
/// ```ignore
/// let element = axio::element_at(100.0, 200.0)?;
/// println!("Element role: {:?}", element.role);
/// ```
pub fn element_at(x: f64, y: f64) -> AxioResult<AXNode> {
    crate::platform::get_element_at_position(x, y).map_err(|e| AxioError::AccessibilityError(e))
}

/// Get the accessibility tree rooted at an element
///
/// Returns the element with its children populated up to `max_depth`.
/// Use `max_depth = 0` for just the element, `1` for immediate children, etc.
///
/// # Arguments
/// * `element_id` - The element to get the tree for
/// * `max_depth` - Maximum depth to traverse (0 = just this element)
/// * `max_children` - Maximum children per level (prevents huge trees)
pub fn tree(
    element_id: &ElementId,
    max_depth: usize,
    max_children: usize,
) -> AxioResult<Vec<AXNode>> {
    crate::platform::macos::get_children_by_element_id(element_id.as_str(), max_depth, max_children)
        .map_err(|e| AxioError::AccessibilityError(e))
}

/// Watch an element for changes
///
/// Subscribes to accessibility notifications for the element.
/// When changes occur, they'll be broadcast through the event system.
///
/// # Notifications watched
/// - Text fields: value changes
/// - Windows: title changes
/// - All elements: destruction
pub fn watch(element_id: &ElementId) -> AxioResult<()> {
    ElementRegistry::watch(element_id).map_err(|e| AxioError::ObserverError(e))
}

/// Stop watching an element for changes
pub fn unwatch(element_id: &ElementId) {
    ElementRegistry::unwatch(element_id)
}

/// Write text to an element (for text fields)
///
/// Only works on writable elements (text fields, text areas, etc.)
pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
    ElementRegistry::write(element_id, text).map_err(|e| AxioError::AccessibilityError(e))
}

/// Click an element
///
/// Performs a click action on the element.
pub fn click(element_id: &ElementId) -> AxioResult<()> {
    crate::platform::click_element_by_id(element_id.as_str())
        .map_err(|e| AxioError::AccessibilityError(e))
}

// ============================================================================
// Window Operations (delegated to windows module for now)
// ============================================================================

// Note: Window operations are currently handled through the polling loop
// and WebSocket state. A future refactor could move them here for consistency.

// ============================================================================
// Initialization
// ============================================================================

/// Initialize the AXIO system
///
/// Must be called once at startup. Events are emitted via the EventSink
/// (set via `axio::set_event_sink`).
pub fn initialize() {
    ElementRegistry::initialize();
}
