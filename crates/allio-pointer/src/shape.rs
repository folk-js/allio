//! macOS: the system cursor's current shape (arrow, I-beam, hand…), to draw the pointer with
//! while the system cursor is hidden. The app under the real cursor keeps setting the shape as
//! usual, so this is the shape the pointer has where it acts.

#![allow(unsafe_code)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use dispatch2::DispatchQueue;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSCursor};
use objc2_foundation::NSDictionary;

/// A cursor image and where in it the pointer's tip is.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
  /// Changes whenever the image does.
  pub id: u64,
  /// The image as PNG, at the best resolution the system has.
  pub png: Vec<u8>,
  /// Size in points.
  pub size: (f64, f64),
  /// The pointer's tip, in points from the top-left.
  pub hot: (f64, f64),
}

/// Reads the current shape, re-encoding only when it changed since `last`. On the main thread,
/// as `AppKit` expects. `None` if the system will not say.
pub(crate) fn current(last: Option<&Shape>) -> Option<Shape> {
  let last_id = last.map(|s| s.id);
  let mut out = None;
  DispatchQueue::main().exec_sync(|| out = read(last_id));
  out.or_else(|| last.cloned())
}

/// `None` when unavailable or unchanged from `last_id`.
fn read(last_id: Option<u64>) -> Option<Shape> {
  // Deprecated, and documented to return nil "in a future version of macOS", but the only public
  // way to see another app's cursor. Without it the page draws a plain arrow.
  #[allow(deprecated)]
  let cursor = NSCursor::currentSystemCursor()?;
  let image = cursor.image();
  let tiff = image.TIFFRepresentation()?;
  let bytes = tiff.to_vec();
  let mut hasher = DefaultHasher::new();
  bytes.hash(&mut hasher);
  let hot = cursor.hotSpot();
  hot.x.to_bits().hash(&mut hasher);
  hot.y.to_bits().hash(&mut hasher);
  let id = hasher.finish();
  if last_id == Some(id) {
    return None;
  }

  let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
  let properties = NSDictionary::new();
  // SAFETY: an empty properties dictionary is valid for every file type.
  let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &properties) }?;
  let size = image.size();
  Some(Shape {
    id,
    png: png.to_vec(),
    size: (size.width, size.height),
    hot: (hot.x, hot.y),
  })
}
