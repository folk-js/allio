use crate::types::Point;
use objc2_core_graphics::{CGEvent, CGEventSource, CGEventSourceStateID};

/// Get current mouse position on screen.
pub(crate) fn get_mouse_position() -> Option<Point> {
  let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)?;
  let event = CGEvent::new(Some(&source))?;
  let location = CGEvent::location(Some(&event));
  Some(Point::new(location.x, location.y))
}
