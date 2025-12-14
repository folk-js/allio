#![allow(clippy::cast_precision_loss)]

use objc2_core_graphics::{CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID};

/// Get main screen dimensions (width, height).
pub(crate) fn get_main_screen_dimensions() -> (f64, f64) {
  let display_id = CGMainDisplayID();
  (
    CGDisplayPixelsWide(display_id) as f64,
    CGDisplayPixelsHigh(display_id) as f64,
  )
}
