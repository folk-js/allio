/*! Geometry types for screen coordinates. */

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Rectangle bounds in screen coordinates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct Bounds {
  pub x: f64,
  pub y: f64,
  pub w: f64,
  pub h: f64,
}

impl Bounds {
  /// Check if two bounds match within a margin of error.
  pub fn matches(&self, other: &Bounds, margin: f64) -> bool {
    (self.x - other.x).abs() <= margin
      && (self.y - other.y).abs() <= margin
      && (self.w - other.w).abs() <= margin
      && (self.h - other.h).abs() <= margin
  }

  /// Check if a point is contained within these bounds.
  pub fn contains(&self, point: Point) -> bool {
    point.x >= self.x
      && point.x <= self.x + self.w
      && point.y >= self.y
      && point.y <= self.y + self.h
  }

  /// Check if bounds match a given size at origin (0,0) within a margin.
  pub fn matches_size_at_origin(&self, width: f64, height: f64) -> bool {
    let target = Bounds {
      x: 0.0,
      y: 0.0,
      w: width,
      h: height,
    };
    self.matches(&target, 1.0)
  }
}

/// A 2D point in screen coordinates.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, TS)]
#[ts(export)]
pub struct Point {
  pub x: f64,
  pub y: f64,
}

impl Point {
  pub const fn new(x: f64, y: f64) -> Self {
    Self { x, y }
  }

  /// Check if this point moved more than threshold from another.
  pub fn moved_from(&self, other: Point, threshold: f64) -> bool {
    (self.x - other.x).abs() >= threshold || (self.y - other.y).abs() >= threshold
  }
}

