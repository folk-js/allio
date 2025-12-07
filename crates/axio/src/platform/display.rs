#[cfg(target_os = "macos")]
use objc2_core_graphics::{CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID};

/// Get main screen dimensions (width, height).
#[cfg(target_os = "macos")]
pub fn get_main_screen_dimensions() -> (f64, f64) {
  let display_id = CGMainDisplayID();
  (
    CGDisplayPixelsWide(display_id) as f64,
    CGDisplayPixelsHigh(display_id) as f64,
  )
}

#[cfg(not(target_os = "macos"))]
pub fn get_main_screen_dimensions() -> (f64, f64) {
  (1920.0, 1080.0)
}
