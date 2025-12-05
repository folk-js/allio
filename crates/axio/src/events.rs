//! Trait-based event system decoupled from transport.

use crate::types::{AXElement, AXWindow, ElementId, TextRange, WindowId};

/// Implement to receive AXIO events.
/// Events notify clients when the Registry changes.
pub trait EventSink: Send + Sync + 'static {
    // Window lifecycle
    fn on_window_added(&self, window: &AXWindow);
    fn on_window_changed(&self, window: &AXWindow);
    fn on_window_removed(&self, window: &AXWindow);

    // Window focus (from polling)
    fn on_focus_changed(&self, window_id: Option<&WindowId>);
    fn on_active_changed(&self, window_id: &WindowId);

    // Elements
    fn on_element_added(&self, element: &AXElement);
    fn on_element_changed(&self, element: &AXElement);
    fn on_element_removed(&self, element: &AXElement);

    // Element focus (from Tier 1 app observer)
    fn on_focus_element(
        &self,
        window_id: &str,
        element_id: &ElementId,
        element: &AXElement,
        previous_element_id: Option<&ElementId>,
    );

    // Selection (from Tier 1 app observer)
    fn on_selection_changed(
        &self,
        window_id: &str,
        element_id: &ElementId,
        text: &str,
        range: Option<&TextRange>,
    );

    // Input
    fn on_mouse_position(&self, x: f64, y: f64);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
    fn on_window_added(&self, _window: &AXWindow) {}
    fn on_window_changed(&self, _window: &AXWindow) {}
    fn on_window_removed(&self, _window: &AXWindow) {}
    fn on_focus_changed(&self, _window_id: Option<&WindowId>) {}
    fn on_active_changed(&self, _window_id: &WindowId) {}
    fn on_element_added(&self, _element: &AXElement) {}
    fn on_element_changed(&self, _element: &AXElement) {}
    fn on_element_removed(&self, _element: &AXElement) {}
    fn on_focus_element(
        &self,
        _window_id: &str,
        _element_id: &ElementId,
        _element: &AXElement,
        _previous_element_id: Option<&ElementId>,
    ) {
    }
    fn on_selection_changed(
        &self,
        _window_id: &str,
        _element_id: &ElementId,
        _text: &str,
        _range: Option<&TextRange>,
    ) {
    }
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
pub(crate) fn emit_window_added(window: &AXWindow) {
    sink().on_window_added(window);
}

pub(crate) fn emit_window_changed(window: &AXWindow) {
    sink().on_window_changed(window);
}

pub(crate) fn emit_window_removed(window: &AXWindow) {
    sink().on_window_removed(window);
}

// Focus events
pub(crate) fn emit_focus_changed(window_id: Option<&WindowId>) {
    sink().on_focus_changed(window_id);
}

pub(crate) fn emit_active_changed(window_id: &WindowId) {
    sink().on_active_changed(window_id);
}

// Element events
pub(crate) fn emit_element_added(element: &AXElement) {
    sink().on_element_added(element);
}

pub(crate) fn emit_element_changed(element: &AXElement) {
    sink().on_element_changed(element);
}

pub(crate) fn emit_element_removed(element: &AXElement) {
    sink().on_element_removed(element);
}

// Element focus (Tier 1)
pub(crate) fn emit_focus_element(
    window_id: &str,
    element_id: &ElementId,
    element: &AXElement,
    previous_element_id: Option<&ElementId>,
) {
    sink().on_focus_element(window_id, element_id, element, previous_element_id);
}

// Selection (Tier 1)
pub(crate) fn emit_selection_changed(
    window_id: &str,
    element_id: &ElementId,
    text: &str,
    range: Option<&TextRange>,
) {
    sink().on_selection_changed(window_id, element_id, text, range);
}
