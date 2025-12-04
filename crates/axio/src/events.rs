//! Trait-based event system decoupled from transport.

use crate::types::{AXElement, AXWindow, ElementId};

/// Implement to receive AXIO events.
pub trait EventSink: Send + Sync + 'static {
    /// Window list changed
    fn on_window_update(&self, windows: &[AXWindow]);
    /// Elements discovered or updated
    fn on_elements(&self, elements: &[AXElement]);
    /// Element destroyed
    fn on_element_destroyed(&self, element_id: &ElementId);
    /// Mouse position update
    fn on_mouse_position(&self, x: f64, y: f64);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn on_window_update(&self, _windows: &[AXWindow]) {}
    fn on_elements(&self, _elements: &[AXElement]) {}
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

pub(crate) fn emit_window_update(windows: &[AXWindow]) {
    sink().on_window_update(windows);
}

pub(crate) fn emit_elements(elements: Vec<AXElement>) {
    sink().on_elements(&elements);
}

pub(crate) fn emit_element_destroyed(element_id: &ElementId) {
    sink().on_element_destroyed(element_id);
}
