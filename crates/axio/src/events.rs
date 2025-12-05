//! Trait-based event system decoupled from transport.

use crate::types::{AXElement, AXWindow, ElementId, ServerEvent, TextRange, WindowId};

/// Implement to receive AXIO events.
/// Events notify clients when the Registry changes.
pub trait EventSink: Send + Sync + 'static {
    fn emit(&self, event: ServerEvent);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn emit(&self, _event: ServerEvent) {}
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
pub(crate) fn emit_window_added(window: &AXWindow, depth_order: &[WindowId]) {
    sink().emit(ServerEvent::WindowAdded {
        window: window.clone(),
        depth_order: depth_order.to_vec(),
    });
}

pub(crate) fn emit_window_changed(window: &AXWindow, depth_order: &[WindowId]) {
    sink().emit(ServerEvent::WindowChanged {
        window: window.clone(),
        depth_order: depth_order.to_vec(),
    });
}

pub(crate) fn emit_window_removed(window: &AXWindow, depth_order: &[WindowId]) {
    sink().emit(ServerEvent::WindowRemoved {
        window: window.clone(),
        depth_order: depth_order.to_vec(),
    });
}

// Focus events
pub(crate) fn emit_focus_changed(window_id: Option<&WindowId>) {
    sink().emit(ServerEvent::FocusChanged {
        window_id: window_id.cloned(),
    });
}

pub(crate) fn emit_active_changed(window_id: &WindowId) {
    sink().emit(ServerEvent::ActiveChanged {
        window_id: window_id.clone(),
    });
}

// Element events
pub(crate) fn emit_element_added(element: &AXElement) {
    sink().emit(ServerEvent::ElementAdded {
        element: element.clone(),
    });
}

pub(crate) fn emit_element_changed(element: &AXElement) {
    sink().emit(ServerEvent::ElementChanged {
        element: element.clone(),
    });
}

pub(crate) fn emit_element_removed(element: &AXElement) {
    sink().emit(ServerEvent::ElementRemoved {
        element: element.clone(),
    });
}

// Element focus (Tier 1)
pub(crate) fn emit_focus_element(
    window_id: &str,
    element_id: &ElementId,
    element: &AXElement,
    previous_element_id: Option<&ElementId>,
) {
    sink().emit(ServerEvent::FocusElement {
        window_id: window_id.to_string(),
        element_id: element_id.clone(),
        element: element.clone(),
        previous_element_id: previous_element_id.cloned(),
    });
}

// Selection (Tier 1)
pub(crate) fn emit_selection_changed(
    window_id: &str,
    element_id: &ElementId,
    text: &str,
    range: Option<&TextRange>,
) {
    sink().emit(ServerEvent::SelectionChanged {
        window_id: window_id.to_string(),
        element_id: element_id.clone(),
        text: text.to_string(),
        range: range.cloned(),
    });
}
