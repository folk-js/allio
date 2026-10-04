//! Stand-in so dependents compile on platforms without an implementation.

use crate::spec::{PointerSpec, PointerState};

/// Pointer fields are not implemented on this platform, so no value of this type exists.
#[derive(Debug)]
#[allow(missing_copy_implementations)] // there are no values to copy
pub enum Pointer {}

impl Pointer {
  /// Always fails on this platform.
  pub fn new(_spec: PointerSpec) -> Result<Self, String> {
    Err("pointer fields are only implemented on macOS".into())
  }

  /// Unreachable: no instance can exist.
  pub fn set(&self, _spec: PointerSpec) {
    match *self {}
  }

  /// Unreachable: no instance can exist.
  pub fn state(&self) -> PointerState {
    match *self {}
  }

  /// Unreachable: no instance can exist.
  pub fn shape(&self) -> Option<Shape> {
    match *self {}
  }
}

/// A cursor image; never produced on this platform.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
  /// Changes whenever the image does.
  pub id: u64,
  /// The image as PNG.
  pub png: Vec<u8>,
  /// Size in points.
  pub size: (f64, f64),
  /// The pointer's tip, in points from the top-left.
  pub hot: (f64, f64),
}
