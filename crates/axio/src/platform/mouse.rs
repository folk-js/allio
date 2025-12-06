//! Mouse position tracking.

use crate::types::Point;

#[cfg(target_os = "macos")]
use core_graphics::event::CGEvent;
#[cfg(target_os = "macos")]
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

/// Get current mouse position on screen.
#[cfg(target_os = "macos")]
pub fn get_mouse_position() -> Option<Point> {
  let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
  let event = CGEvent::new(source).ok()?;
  let location = event.location();
  Some(Point::new(location.x, location.y))
}

#[cfg(not(target_os = "macos"))]
pub fn get_mouse_position() -> Option<Point> {
  None
}
