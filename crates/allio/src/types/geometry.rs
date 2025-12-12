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

#[cfg(test)]
mod tests {
  use super::*;

  mod bounds_matches {
    use super::*;

    #[test]
    fn identical_bounds_match() {
      let a = Bounds {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
      };
      assert!(
        a.matches(&a, 0.0),
        "identical bounds should match with zero margin"
      );
    }

    #[test]
    fn bounds_within_margin_match() {
      let a = Bounds {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
      };
      let b = Bounds {
        x: 10.5,
        y: 20.5,
        w: 100.5,
        h: 50.5,
      };
      assert!(a.matches(&b, 1.0), "bounds within margin should match");
      assert!(
        !a.matches(&b, 0.4),
        "bounds outside margin should not match"
      );
    }

    #[test]
    fn matches_is_symmetric() {
      let a = Bounds {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
      };
      let b = Bounds {
        x: 0.5,
        y: 0.5,
        w: 100.5,
        h: 100.5,
      };
      assert_eq!(
        a.matches(&b, 1.0),
        b.matches(&a, 1.0),
        "matches should be symmetric"
      );
    }

    #[test]
    fn negative_coordinates() {
      let a = Bounds {
        x: -100.0,
        y: -50.0,
        w: 200.0,
        h: 100.0,
      };
      let b = Bounds {
        x: -100.5,
        y: -50.5,
        w: 200.5,
        h: 100.5,
      };
      assert!(a.matches(&b, 1.0), "negative coordinates should work");
    }
  }

  mod bounds_contains {
    use super::*;

    #[test]
    fn point_inside_bounds() {
      let bounds = Bounds {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
      };
      assert!(
        bounds.contains(Point::new(50.0, 50.0)),
        "center point should be contained"
      );
    }

    #[test]
    fn corners_are_contained() {
      let bounds = Bounds {
        x: 10.0,
        y: 20.0,
        w: 100.0,
        h: 50.0,
      };
      // Top-left corner
      assert!(bounds.contains(Point::new(10.0, 20.0)), "top-left corner");
      // Top-right corner
      assert!(bounds.contains(Point::new(110.0, 20.0)), "top-right corner");
      // Bottom-left corner
      assert!(
        bounds.contains(Point::new(10.0, 70.0)),
        "bottom-left corner"
      );
      // Bottom-right corner
      assert!(
        bounds.contains(Point::new(110.0, 70.0)),
        "bottom-right corner"
      );
    }

    #[test]
    fn point_outside_bounds() {
      let bounds = Bounds {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
      };
      assert!(!bounds.contains(Point::new(-1.0, 50.0)), "left of bounds");
      assert!(!bounds.contains(Point::new(101.0, 50.0)), "right of bounds");
      assert!(!bounds.contains(Point::new(50.0, -1.0)), "above bounds");
      assert!(!bounds.contains(Point::new(50.0, 101.0)), "below bounds");
    }

    #[test]
    fn zero_size_bounds() {
      let bounds = Bounds {
        x: 50.0,
        y: 50.0,
        w: 0.0,
        h: 0.0,
      };
      assert!(
        bounds.contains(Point::new(50.0, 50.0)),
        "point at zero-size bounds origin"
      );
      assert!(
        !bounds.contains(Point::new(50.1, 50.0)),
        "point near zero-size bounds"
      );
    }

    #[test]
    fn negative_origin_bounds() {
      let bounds = Bounds {
        x: -50.0,
        y: -50.0,
        w: 100.0,
        h: 100.0,
      };
      assert!(
        bounds.contains(Point::new(0.0, 0.0)),
        "origin in negative-origin bounds"
      );
      assert!(
        bounds.contains(Point::new(-50.0, -50.0)),
        "corner of negative-origin bounds"
      );
      assert!(
        !bounds.contains(Point::new(-51.0, 0.0)),
        "outside negative-origin bounds"
      );
    }
  }

  mod bounds_matches_size_at_origin {
    use super::*;

    #[test]
    fn exact_match_at_origin() {
      let bounds = Bounds {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
      };
      assert!(bounds.matches_size_at_origin(1920.0, 1080.0));
    }

    #[test]
    fn within_default_margin() {
      let bounds = Bounds {
        x: 0.5,
        y: 0.5,
        w: 1920.5,
        h: 1080.5,
      };
      assert!(
        bounds.matches_size_at_origin(1920.0, 1080.0),
        "within 1.0 margin"
      );
    }

    #[test]
    fn outside_default_margin() {
      let bounds = Bounds {
        x: 2.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
      };
      assert!(
        !bounds.matches_size_at_origin(1920.0, 1080.0),
        "x offset > 1.0"
      );
    }

    #[test]
    fn non_origin_position() {
      let bounds = Bounds {
        x: 100.0,
        y: 100.0,
        w: 1920.0,
        h: 1080.0,
      };
      assert!(
        !bounds.matches_size_at_origin(1920.0, 1080.0),
        "not at origin"
      );
    }
  }

  mod point_new {
    use super::*;

    #[test]
    fn creates_point() {
      let p = Point::new(10.0, 20.0);
      assert_eq!(p.x, 10.0);
      assert_eq!(p.y, 20.0);
    }

    #[test]
    fn negative_coordinates() {
      let p = Point::new(-10.0, -20.0);
      assert_eq!(p.x, -10.0);
      assert_eq!(p.y, -20.0);
    }
  }

  mod point_moved_from {
    use super::*;

    #[test]
    fn no_movement() {
      let p = Point::new(10.0, 20.0);
      assert!(
        !p.moved_from(p, 1.0),
        "same point should not register as moved"
      );
    }

    #[test]
    fn movement_below_threshold() {
      let a = Point::new(10.0, 20.0);
      let b = Point::new(10.5, 20.5);
      assert!(!a.moved_from(b, 1.0), "movement below threshold");
    }

    #[test]
    fn movement_at_threshold() {
      let a = Point::new(10.0, 20.0);
      let b = Point::new(11.0, 20.0);
      assert!(a.moved_from(b, 1.0), "movement exactly at threshold");
    }

    #[test]
    fn movement_above_threshold() {
      let a = Point::new(10.0, 20.0);
      let b = Point::new(12.0, 20.0);
      assert!(a.moved_from(b, 1.0), "movement above threshold");
    }

    #[test]
    fn movement_in_y_only() {
      let a = Point::new(10.0, 20.0);
      let b = Point::new(10.0, 22.0);
      assert!(a.moved_from(b, 1.0), "y-only movement");
    }

    #[test]
    fn diagonal_movement() {
      let a = Point::new(0.0, 0.0);
      let b = Point::new(0.5, 0.5);
      // Neither x nor y moved >= 1.0
      assert!(
        !a.moved_from(b, 1.0),
        "diagonal movement below threshold in both axes"
      );
    }

    #[test]
    fn is_symmetric() {
      let a = Point::new(0.0, 0.0);
      let b = Point::new(2.0, 3.0);
      assert_eq!(
        a.moved_from(b, 1.0),
        b.moved_from(a, 1.0),
        "moved_from should be symmetric"
      );
    }
  }
}

#[cfg(test)]
mod proptests {
  use super::*;
  use proptest::prelude::*;

  /// Strategy for generating reasonable screen coordinates
  fn coord() -> impl Strategy<Value = f64> {
    -10000.0..10000.0f64
  }

  /// Strategy for generating non-negative dimensions
  fn dimension() -> impl Strategy<Value = f64> {
    0.0..5000.0f64
  }

  /// Strategy for generating positive margins
  fn margin() -> impl Strategy<Value = f64> {
    0.0..100.0f64
  }

  proptest! {
    /// Bounds::matches is reflexive (a.matches(a, m) for any m >= 0)
    #[test]
    fn matches_reflexive(x in coord(), y in coord(), w in dimension(), h in dimension(), m in margin()) {
      let bounds = Bounds { x, y, w, h };
      prop_assert!(bounds.matches(&bounds, m), "bounds should match itself");
    }

    /// Bounds::matches is symmetric
    #[test]
    fn matches_symmetric(
      x1 in coord(), y1 in coord(), w1 in dimension(), h1 in dimension(),
      x2 in coord(), y2 in coord(), w2 in dimension(), h2 in dimension(),
      m in margin()
    ) {
      let a = Bounds { x: x1, y: y1, w: w1, h: h1 };
      let b = Bounds { x: x2, y: y2, w: w2, h: h2 };
      prop_assert_eq!(a.matches(&b, m), b.matches(&a, m), "matches should be symmetric");
    }

    /// Larger margins are more permissive
    #[test]
    fn matches_margin_monotonic(
      x1 in coord(), y1 in coord(), w1 in dimension(), h1 in dimension(),
      x2 in coord(), y2 in coord(), w2 in dimension(), h2 in dimension(),
      m1 in 0.0..50.0f64, m2 in 50.0..100.0f64
    ) {
      let a = Bounds { x: x1, y: y1, w: w1, h: h1 };
      let b = Bounds { x: x2, y: y2, w: w2, h: h2 };
      // If matches with smaller margin, must match with larger margin
      if a.matches(&b, m1) {
        prop_assert!(a.matches(&b, m2), "larger margin should be more permissive");
      }
    }

    /// Bounds corners are always contained
    #[test]
    fn corners_contained(x in coord(), y in coord(), w in dimension(), h in dimension()) {
      let bounds = Bounds { x, y, w, h };
      // Top-left
      prop_assert!(bounds.contains(Point::new(x, y)), "top-left corner");
      // Top-right
      prop_assert!(bounds.contains(Point::new(x + w, y)), "top-right corner");
      // Bottom-left
      prop_assert!(bounds.contains(Point::new(x, y + h)), "bottom-left corner");
      // Bottom-right
      prop_assert!(bounds.contains(Point::new(x + w, y + h)), "bottom-right corner");
    }

    /// Points outside bounds are not contained
    #[test]
    fn outside_not_contained(x in coord(), y in coord(), w in 1.0..5000.0f64, h in 1.0..5000.0f64) {
      let bounds = Bounds { x, y, w, h };
      // Points just outside each edge
      prop_assert!(!bounds.contains(Point::new(x - 0.001, y + h / 2.0)), "left of bounds");
      prop_assert!(!bounds.contains(Point::new(x + w + 0.001, y + h / 2.0)), "right of bounds");
      prop_assert!(!bounds.contains(Point::new(x + w / 2.0, y - 0.001)), "above bounds");
      prop_assert!(!bounds.contains(Point::new(x + w / 2.0, y + h + 0.001)), "below bounds");
    }

    /// Point::moved_from is symmetric
    #[test]
    fn moved_from_symmetric(x1 in coord(), y1 in coord(), x2 in coord(), y2 in coord(), threshold in 0.1..100.0f64) {
      let a = Point::new(x1, y1);
      let b = Point::new(x2, y2);
      prop_assert_eq!(a.moved_from(b, threshold), b.moved_from(a, threshold), "moved_from should be symmetric");
    }

    /// Point::moved_from with zero threshold always returns true (unless same point)
    #[test]
    fn moved_from_zero_threshold(x1 in coord(), y1 in coord(), x2 in coord(), y2 in coord()) {
      let a = Point::new(x1, y1);
      let b = Point::new(x2, y2);
      if x1 != x2 || y1 != y2 {
        prop_assert!(a.moved_from(b, 0.0), "different points should be 'moved' with zero threshold");
      }
    }

    /// matches_size_at_origin correctly checks origin position
    #[test]
    fn matches_size_origin_check(w in dimension(), h in dimension(), offset in 2.0..100.0f64) {
      let at_origin = Bounds { x: 0.0, y: 0.0, w, h };
      let offset_bounds = Bounds { x: offset, y: offset, w, h };

      prop_assert!(at_origin.matches_size_at_origin(w, h), "should match at origin");
      prop_assert!(!offset_bounds.matches_size_at_origin(w, h), "should not match when offset");
    }
  }
}
