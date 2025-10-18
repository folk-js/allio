/**
 * Global Mouse Position Tracking
 *
 * Tracks mouse position system-wide, even when the window is not focused.
 * Broadcasts position updates to connected WebSocket clients.
 */
use std::thread;
use std::time::Duration;

#[cfg(target_os = "macos")]
use core_graphics::event::CGEvent;
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use crate::websocket::WebSocketState;

/// Get current mouse position (macOS)
#[cfg(target_os = "macos")]
pub fn get_mouse_position() -> Option<(f64, f64)> {
    // Create an event source
    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;

    // Get mouse moved event to read current position
    let event = CGEvent::new(source).ok()?;
    let location = event.location();

    Some((location.x, location.y))
}

/// Get current mouse position (fallback for non-macOS)
#[cfg(not(target_os = "macos"))]
pub fn get_mouse_position() -> Option<(f64, f64)> {
    None
}

/// Start global mouse position tracking
/// Polls mouse position and broadcasts to WebSocket clients
pub fn start_mouse_tracking(ws_state: WebSocketState) {
    thread::spawn(move || {
        let mut last_position: Option<(f64, f64)> = None;

        loop {
            // Poll mouse position
            if let Some((x, y)) = get_mouse_position() {
                // Only broadcast if position changed (reduce noise)
                let position_changed = match last_position {
                    Some((last_x, last_y)) => {
                        // Broadcast if moved by at least 1 pixel
                        (x - last_x).abs() >= 1.0 || (y - last_y).abs() >= 1.0
                    }
                    None => true,
                };

                if position_changed {
                    last_position = Some((x, y));

                    // Broadcast to all connected clients
                    let message = serde_json::json!({
                        "event_type": "mouse_position",
                        "x": x,
                        "y": y,
                    });

                    if let Ok(json) = serde_json::to_string(&message) {
                        let _ = ws_state.sender.send(json);
                    }
                }
            }

            // Poll at ~120 Hz for smooth tracking
            thread::sleep(Duration::from_millis(8));
        }
    });
}
