//! Screen utilities
//!
//! Cross-platform utilities for screen dimensions and mouse position.

use crate::platform;
use crate::types::Point;

/// Get main screen dimensions (width, height).
pub fn dimensions() -> (f64, f64) {
  platform::get_main_screen_dimensions()
}

/// Get current mouse position on screen.
pub fn mouse_position() -> Option<Point> {
  platform::get_mouse_position()
}

