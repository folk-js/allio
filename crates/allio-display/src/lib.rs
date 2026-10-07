//! A backstage display: a virtual display nobody looks at.
//!
//! Windows moved onto it keep rendering at full rate and stay uncovered (each can have the
//! whole display to itself), so their pixels can be shown elsewhere and the pointer can act on
//! them there. To the window server it is a real display; to the user it doesn't exist, as long
//! as nothing shows it and the pointer can't wander onto it.
//!
//! On macOS this uses `CGVirtualDisplay`, a private `CoreGraphics` class that every
//! virtual-display app relies on. Its mode is set once, when it is created, and never changed: a
//! display whose mode has been changed can't be removed. Dropping a [`Backstage`] removes it, and
//! the window server moves its windows back onto real displays; so does the process exiting.
//! On other platforms [`Backstage::new`] returns an error.

#[cfg(target_os = "macos")]
mod macos;

/// A rectangle in global screen points (top-left origin).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
  /// Left edge.
  pub x: f64,
  /// Top edge.
  pub y: f64,
  /// Width.
  pub w: f64,
  /// Height.
  pub h: f64,
}

#[cfg(target_os = "macos")]
pub use macos::Backstage;

/// Backstage displays are only implemented on macOS, so no value of this type exists elsewhere.
#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
#[allow(missing_copy_implementations)]
pub enum Backstage {}

#[cfg(not(target_os = "macos"))]
impl Backstage {
  /// Always fails on this platform.
  pub fn new(_w: u32, _h: u32) -> Result<Self, String> {
    Err("backstage displays are only implemented on macOS".into())
  }

  /// Unreachable: no instance can exist.
  pub fn frame(&self) -> Frame {
    match *self {}
  }
}
