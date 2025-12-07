use crate::types::Event;

/// Implement to receive AXIO events.
/// Events notify clients when the Registry changes.
pub trait EventSink: Send + Sync + 'static {
  fn emit(&self, event: Event);
}

pub struct NoopEventSink;

impl EventSink for NoopEventSink {
  fn emit(&self, _event: Event) {}
}

static EVENT_SINK: std::sync::OnceLock<Box<dyn EventSink>> = std::sync::OnceLock::new();

fn sink() -> &'static dyn EventSink {
  EVENT_SINK.get_or_init(|| Box::new(NoopEventSink)).as_ref()
}

/// Set the event sink. Returns false if already set.
pub fn set_event_sink(new_sink: impl EventSink) -> bool {
  EVENT_SINK.set(Box::new(new_sink)).is_ok()
}

/// Emit an event to the configured sink (internal use only).
pub(crate) fn emit(event: Event) {
  sink().emit(event);
}
