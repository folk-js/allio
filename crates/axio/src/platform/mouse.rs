use crate::types::Point;

#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGEvent, CGEventSource, CGEventSourceStateID};

/// Get current mouse position on screen.
#[cfg(target_os = "macos")]
pub fn get_mouse_position() -> Option<Point> {
  let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)?;
  let event = CGEvent::new(Some(&source))?;
  let location = CGEvent::location(Some(&event));
  Some(Point::new(location.x, location.y))
}

#[cfg(not(target_os = "macos"))]
pub fn get_mouse_position() -> Option<Point> {
  None
}
