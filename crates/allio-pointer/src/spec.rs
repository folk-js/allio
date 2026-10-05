//! The portable description of a pointer field: what a client declares.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A rectangle in screen points (top-left origin).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Rect {
  /// Left edge.
  pub x: f64,
  /// Top edge.
  pub y: f64,
  /// Width.
  pub w: f64,
  /// Height.
  pub h: f64,
}

/// An area that slows the pointer while it is inside.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Target {
  /// The area, before `reach`.
  pub rect: Rect,
  /// Motion inside is multiplied by this. Below 1 makes the area sticky. Default 1.
  #[serde(default)]
  #[ts(optional)]
  pub gain: Option<f64>,
  /// Grows the area by this many points on every side. Default 0.
  #[serde(default)]
  #[ts(optional)]
  pub reach: Option<f64>,
}

/// Part of the screen drawn somewhere else (as in `WinCuts`). While the pointer appears over
/// `shown`, it really is over the matching point of `source`, so it acts on what it appears to.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Cut {
  /// Where the cut is drawn.
  pub shown: Rect,
  /// What it shows: the part of the screen it was cut from. May differ in size from `shown`.
  pub source: Rect,
}

/// A fixed magnifier. Within `r * LENS_FLAT` of its centre the screen is magnified `mag` times;
/// out to `r` the magnification eases back to 1. While the pointer appears in the lens, it really
/// is over what it appears to be over, so it moves more slowly over the real screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Lens {
  /// Centre, left to right.
  pub x: f64,
  /// Centre, top to bottom.
  pub y: f64,
  /// Radius.
  pub r: f64,
  /// Magnification at the centre.
  pub mag: f64,
}

/// A window drawn deformed: rotated, scaled, pushed around. While the pointer appears over the
/// deformed window it really is over the matching point of the real one.
///
/// The map from where the pointer appears to where it acts works in window-local points (from the
/// window's top-left), so it moves with the window: first `affine`, then `grid`. Where the result
/// falls outside the window, the pointer acts where it appears.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Warp {
  /// The real window, in screen points.
  pub window: Rect,
  /// `[a, b, c, d, tx, ty]`: local point (x, y) goes to (a·x + b·y + tx, c·x + d·y + ty).
  #[serde(default)]
  #[ts(optional)]
  pub affine: Option<[f64; 6]>,
  /// Then this displacement is added.
  #[serde(default)]
  #[ts(optional)]
  pub grid: Option<Grid>,
  /// Windows in front of this one, in screen points: there the pointer acts where it appears.
  #[serde(default)]
  #[ts(optional, as = "Option<Vec<Rect>>")]
  pub above: Vec<Rect>,
}

/// A displacement field over a window, grown by `margin` on every side so edges can be pushed
/// outwards: `cols` × `rows` evenly spaced points, row by row, each an (x, y) offset in points,
/// interpolated bilinearly in between. Zero outside.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Grid {
  /// How far the grid reaches past each edge of the window.
  pub margin: f64,
  /// Points across.
  pub cols: u32,
  /// Points down.
  pub rows: u32,
  /// `2 * cols * rows` numbers: x and y offsets.
  pub offsets: Vec<f64>,
}

/// Fraction of a lens's radius that is magnified evenly. Shaders drawing a lens must use the same.
pub const LENS_FLAT: f64 = 0.72;

/// How real mouse motion moves the pointer, and where on the real screen the pointer acts.
///
/// The host keeps a visual pointer that the hand moves (scaled by `gain` and `targets`), and puts
/// the real cursor where the screen shown under the visual pointer really is (through `cuts` and
/// `lenses`). When the two differ the system cursor is hidden and the page draws the visual one.
///
/// A client sends its complete set of these; the host applies all of them at once (gains
/// multiply, the rest add up; later cuts are on top of earlier ones, and cuts on top of lenses).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PointerSpec {
  /// All motion is multiplied by this. Default 1.
  #[serde(default)]
  #[ts(optional)]
  pub gain: Option<f64>,
  /// Sticky areas.
  #[serde(default)]
  #[ts(optional, as = "Option<Vec<Target>>")]
  pub targets: Vec<Target>,
  /// Parts of the screen drawn elsewhere.
  #[serde(default)]
  #[ts(optional, as = "Option<Vec<Cut>>")]
  pub cuts: Vec<Cut>,
  /// Fixed magnifiers.
  #[serde(default)]
  #[ts(optional, as = "Option<Vec<Lens>>")]
  pub lenses: Vec<Lens>,
  /// Deformed windows.
  #[serde(default)]
  #[ts(optional, as = "Option<Vec<Warp>>")]
  pub warps: Vec<Warp>,
}

/// Where the pointer appears, for a page that draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PointerState {
  /// Where the pointer appears, left to right.
  pub x: f64,
  /// Where the pointer appears, top to bottom.
  pub y: f64,
  /// Whether the system cursor is hidden (the pointer acts somewhere else), so the page must
  /// draw it.
  pub hidden: bool,
  /// While hidden: which shape to draw it with (changes when the shape does). Absent if the
  /// system won't say; draw an arrow then.
  #[serde(default)]
  #[ts(optional)]
  pub shape: Option<String>,
}

/// Gains outside this range would freeze the cursor or fling it.
pub(crate) const GAIN_RANGE: (f64, f64) = (0.05, 8.0);

impl PointerSpec {
  /// Overall gain, clamped to a usable range.
  pub fn gain(&self) -> f64 {
    clamp_gain(self.gain.unwrap_or(1.0))
  }

  /// Whether this leaves the pointer alone.
  pub fn is_identity(&self) -> bool {
    (self.gain() - 1.0).abs() < f64::EPSILON
      && self.targets.is_empty()
      && self.cuts.is_empty()
      && self.lenses.is_empty()
      && self.warps.is_empty()
  }

  /// One spec that does what all of these do together.
  pub fn merge<'a>(specs: impl IntoIterator<Item = &'a Self>) -> Self {
    let mut out = Self::default();
    let mut gain = 1.0;
    for spec in specs {
      gain *= spec.gain.unwrap_or(1.0);
      out.targets.extend(&spec.targets);
      out.cuts.extend(&spec.cuts);
      out.lenses.extend(&spec.lenses);
      out.warps.extend(spec.warps.iter().cloned());
    }
    out.gain = Some(clamp_gain(gain));
    out
  }
}

pub(crate) const fn clamp_gain(gain: f64) -> f64 {
  if gain.is_finite() {
    gain.clamp(GAIN_RANGE.0, GAIN_RANGE.1)
  } else {
    1.0
  }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
  use super::*;

  #[test]
  fn optional_fields_default() {
    let spec: PointerSpec = serde_json::from_str("{}").unwrap();
    assert!(spec.is_identity());
    let t: Target =
      serde_json::from_str(r#"{ "rect": { "x": 0, "y": 0, "w": 1, "h": 1 } }"#).unwrap();
    assert_eq!(t.gain, None);
  }

  #[test]
  fn merge_multiplies_gains_and_collects_the_rest() {
    let r = Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 };
    let a = PointerSpec {
      gain: Some(0.5),
      cuts: vec![Cut { shown: r, source: r }],
      ..PointerSpec::default()
    };
    let b = PointerSpec {
      gain: Some(0.5),
      lenses: vec![Lens { x: 0.0, y: 0.0, r: 1.0, mag: 2.0 }],
      ..PointerSpec::default()
    };
    let m = PointerSpec::merge([&a, &b]);
    assert!((m.gain() - 0.25).abs() < 1e-9);
    assert_eq!((m.cuts.len(), m.lenses.len()), (1, 1));
  }

  #[test]
  fn gain_is_clamped() {
    let spec = PointerSpec {
      gain: Some(0.0),
      ..PointerSpec::default()
    };
    assert!((spec.gain() - GAIN_RANGE.0).abs() < f64::EPSILON);
    let spec = PointerSpec {
      gain: Some(f64::NAN),
      ..PointerSpec::default()
    };
    assert!((spec.gain() - 1.0).abs() < f64::EPSILON);
  }
}
