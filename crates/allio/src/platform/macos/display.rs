#![allow(clippy::cast_precision_loss)]

use crate::types::Bounds;
use objc2_core_graphics::{
  CGDirectDisplayID, CGDisplayBounds, CGDisplayPixelsHigh, CGDisplayPixelsWide,
  CGGetActiveDisplayList, CGMainDisplayID,
};

/// Get main screen dimensions (width, height).
pub(crate) fn get_main_screen_dimensions() -> (f64, f64) {
  let display_id = CGMainDisplayID();
  (
    CGDisplayPixelsWide(display_id) as f64,
    CGDisplayPixelsHigh(display_id) as f64,
  )
}

/// Bounds of every active display, in global points (top-left origin).
#[allow(unsafe_code)]
pub(crate) fn active_displays() -> Vec<Bounds> {
  let mut ids: [CGDirectDisplayID; 16] = [0; 16];
  let mut count = 0u32;
  // SAFETY: `ids` has room for 16 displays and `count` receives how many were written.
  let err = unsafe { CGGetActiveDisplayList(16, ids.as_mut_ptr(), &raw mut count) };
  if err.0 != 0 {
    return Vec::new();
  }
  ids
    .iter()
    .take(count as usize)
    .map(|&id| {
      let b = CGDisplayBounds(id);
      Bounds { x: b.origin.x, y: b.origin.y, w: b.size.width, h: b.size.height }
    })
    .collect()
}
