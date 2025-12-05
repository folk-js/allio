//! Global mouse position tracking.

use axio::{EventSink, ServerEvent};
use axio_ws::WebSocketState;
use std::thread;
use std::time::Duration;

#[cfg(target_os = "macos")]
use core_graphics::event::CGEvent;
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

#[cfg(target_os = "macos")]
pub fn get_mouse_position() -> Option<(f64, f64)> {
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let location = event.location();
    Some((location.x, location.y))
}

#[cfg(not(target_os = "macos"))]
pub fn get_mouse_position() -> Option<(f64, f64)> {
    None
}

/// Polls mouse position and broadcasts via EventSink.
pub fn start_mouse_tracking(ws_state: WebSocketState) {
    thread::spawn(move || {
        let mut last_position: Option<(f64, f64)> = None;

        loop {
            if let Some((x, y)) = get_mouse_position() {
                let changed = match last_position {
                    Some((lx, ly)) => (x - lx).abs() >= 1.0 || (y - ly).abs() >= 1.0,
                    None => true,
                };

                if changed {
                    last_position = Some((x, y));
                    ws_state.emit(ServerEvent::MousePosition { x, y });
                }
            }

            thread::sleep(Duration::from_millis(8));
        }
    });
}
