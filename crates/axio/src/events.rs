//! Trait-based event system decoupled from transport.

use crate::types::{AXElement, AXWindow, ElementId};

/// Implement to receive AXIO events.
pub trait EventSink: Send + Sync + 'static {
    // Window lifecycle
    fn on_window_opened(&self, window: &AXWindow);
    fn on_window_closed(&self, window_id: &str);
    fn on_window_updated(&self, window: &AXWindow);

    // Focus
    fn on_window_active(&self, window_id: Option<&str>);

    // Elements
    fn on_element_discovered(&self, element: &AXElement);
    fn on_element_updated(&self, element: &AXElement, changed: &[String]);
    fn on_element_destroyed(&self, element_id: &ElementId);

    // Input
    fn on_mouse_position(&self, x: f64, y: f64);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn on_window_opened(&self, _window: &AXWindow) {}
    fn on_window_closed(&self, _window_id: &str) {}
    fn on_window_updated(&self, _window: &AXWindow) {}
    fn on_window_active(&self, _window_id: Option<&str>) {}
    fn on_element_discovered(&self, _element: &AXElement) {}
    fn on_element_updated(&self, _element: &AXElement, _changed: &[String]) {}
    fn on_element_destroyed(&self, _element_id: &ElementId) {}
    fn on_mouse_position(&self, _x: f64, _y: f64) {}
}

static EVENT_SINK: std::sync::OnceLock<Box<dyn EventSink>> = std::sync::OnceLock::new();

fn sink() -> &'static dyn EventSink {
    EVENT_SINK.get_or_init(|| Box::new(NoopEventSink)).as_ref()
}

/// Set the event sink. Returns false if already set.
pub fn set_event_sink(new_sink: impl EventSink) -> bool {
    EVENT_SINK.set(Box::new(new_sink)).is_ok()
}

// Window events
pub(crate) fn emit_window_opened(window: &AXWindow) {
    sink().on_window_opened(window);
}

pub(crate) fn emit_window_closed(window_id: &str) {
    sink().on_window_closed(window_id);
}

pub(crate) fn emit_window_updated(window: &AXWindow) {
    sink().on_window_updated(window);
}

pub(crate) fn emit_window_active(window_id: Option<&str>) {
    sink().on_window_active(window_id);
}

// Element events
pub(crate) fn emit_element_discovered(element: &AXElement) {
    sink().on_element_discovered(element);
}

pub(crate) fn emit_element_updated(element: &AXElement, changed: &[String]) {
    sink().on_element_updated(element, changed);
}

pub(crate) fn emit_element_destroyed(element_id: &ElementId) {
    sink().on_element_destroyed(element_id);
}
