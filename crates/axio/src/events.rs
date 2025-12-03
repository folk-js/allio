//! Event System for AXIO
//!
//! Provides a trait-based event system that decouples the core from
//! any specific notification mechanism (WebSocket, channels, etc.)

use crate::types::{AXNode, ElementUpdate, WindowInfo};

/// Trait for receiving events from AXIO
///
/// Implement this trait to receive notifications about element changes,
/// window updates, etc. The implementation decides how to deliver these
/// (WebSocket broadcast, channel, callback, etc.)
pub trait EventSink: Send + Sync + 'static {
    /// Called when an element's value, label, etc. changes
    fn on_element_update(&self, update: ElementUpdate);

    /// Called when the window list changes
    fn on_window_update(&self, windows: &[WindowInfo]);

    /// Called when a focused window's accessibility tree root is available
    fn on_window_root(&self, window_id: &str, root: &AXNode);

    /// Called for mouse position updates (if tracking is enabled)
    fn on_mouse_position(&self, x: f64, y: f64);
}

/// A no-op event sink for when you don't need events
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn on_element_update(&self, _update: ElementUpdate) {}
    fn on_window_update(&self, _windows: &[WindowInfo]) {}
    fn on_window_root(&self, _window_id: &str, _root: &AXNode) {}
    fn on_mouse_position(&self, _x: f64, _y: f64) {}
}

/// Global event sink - set once at initialization
static EVENT_SINK: std::sync::OnceLock<Box<dyn EventSink>> = std::sync::OnceLock::new();

/// Initialize the event system with a sink
///
/// Must be called once at startup. Panics if called twice.
pub fn set_event_sink(sink: impl EventSink) {
    if EVENT_SINK.set(Box::new(sink)).is_err() {
        panic!("Event sink already initialized");
    }
}

/// Get the current event sink (or panic if not initialized)
pub(crate) fn event_sink() -> &'static dyn EventSink {
    EVENT_SINK
        .get()
        .map(|b| b.as_ref())
        .expect("Event sink not initialized - call axio::events::set_event_sink first")
}

/// Check if event sink is initialized
pub fn is_initialized() -> bool {
    EVENT_SINK.get().is_some()
}

// ============================================================================
// Convenience functions for emitting events
// ============================================================================

pub(crate) fn emit_element_update(update: ElementUpdate) {
    if let Some(sink) = EVENT_SINK.get() {
        sink.on_element_update(update);
    }
}

pub(crate) fn emit_window_update(windows: &[WindowInfo]) {
    if let Some(sink) = EVENT_SINK.get() {
        sink.on_window_update(windows);
    }
}

pub(crate) fn emit_window_root(window_id: &str, root: &AXNode) {
    if let Some(sink) = EVENT_SINK.get() {
        sink.on_window_root(window_id, root);
    }
}

pub(crate) fn emit_mouse_position(x: f64, y: f64) {
    if let Some(sink) = EVENT_SINK.get() {
        sink.on_mouse_position(x, y);
    }
}
