//! Trait-based event system decoupled from transport (WebSocket, channels, etc.)

use crate::types::{AXNode, AXWindow, ElementUpdate};

/// Implement to receive AXIO events.
pub trait EventSink: Send + Sync + 'static {
    fn on_element_update(&self, update: ElementUpdate);
    fn on_window_update(&self, windows: &[AXWindow]);
    fn on_window_root(&self, window_id: &str, root: &AXNode);
    fn on_mouse_position(&self, x: f64, y: f64);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn on_element_update(&self, _update: ElementUpdate) {}
    fn on_window_update(&self, _windows: &[AXWindow]) {}
    fn on_window_root(&self, _window_id: &str, _root: &AXNode) {}
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

pub(crate) fn emit_element_update(update: ElementUpdate) {
    sink().on_element_update(update);
}

pub(crate) fn emit_window_update(windows: &[AXWindow]) {
    sink().on_window_update(windows);
}

pub(crate) fn emit_window_root(window_id: &str, root: &AXNode) {
    sink().on_window_root(window_id, root);
}
