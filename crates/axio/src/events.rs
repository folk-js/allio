/*!
Event emission for AXIO.

# Singleton Design

EVENT_SINK is a global `OnceLock` initialized once at startup. This design was chosen because:

1. **Write-once**: The sink is set once during initialization and never changes.
   `OnceLock` enforces this at the type level.

2. **No callback risk**: The WebSocket event sink broadcasts to clients over the network.
   Clients cannot synchronously call back into the registry, so there's no deadlock risk
   when emitting events while holding the registry lock.

3. **Observer callbacks**: macOS accessibility observer callbacks fire on the main thread
   and need to emit events. Global access is simpler than threading sinks through C callbacks.

# Alternative Designs

- **Registry-owned sink**: Registry could own the sink, but then events would need to be
  collected and emitted after releasing the lock, adding complexity.

- **Return events from functions**: Pure functional approach where functions return events
  instead of emitting them. Requires changing all call sites and still needs a place to
  actually emit (which would be another global or passed parameter).

- **Dependency injection**: Pass `Arc<dyn EventSink>` to all functions. Cleaner but
  invasive change and doesn't simplify macOS callback handling.
*/

use crate::types::Event;

/// Implement to receive AXIO events.
/// Events notify clients when the Registry changes.
pub trait EventSink: Send + Sync + 'static {
  fn emit(&self, event: Event);
}

/// No-op event sink for testing or when events aren't needed.
pub struct NoopEventSink;

impl EventSink for NoopEventSink {
  fn emit(&self, _event: Event) {}
}

/// Global event sink. Set once at startup, never changes.
static EVENT_SINK: std::sync::OnceLock<Box<dyn EventSink>> = std::sync::OnceLock::new();

fn sink() -> &'static dyn EventSink {
  EVENT_SINK.get_or_init(|| Box::new(NoopEventSink)).as_ref()
}

/// Set the event sink. Returns false if already set.
/// Call this once during application initialization.
pub fn set_event_sink(new_sink: impl EventSink) -> bool {
  EVENT_SINK.set(Box::new(new_sink)).is_ok()
}

/// Emit an event to all listeners.
/// Called from registry when state changes.
pub(crate) fn emit(event: Event) {
  sink().emit(event);
}
