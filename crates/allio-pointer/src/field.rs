//! What a pointer field does: how the hand moves the visual pointer, and where the real cursor
//! has to be for the visual pointer to act on what it appears to. Pure, so it can be tested
//! without moving anyone's cursor.

use crate::spec::{clamp_gain, Cut, Lens, PointerSpec, Rect, Target, LENS_FLAT};

/// A point or a vector in screen points.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct Vec2 {
  pub(crate) x: f64,
  pub(crate) y: f64,
}

impl Vec2 {
  pub(crate) const fn new(x: f64, y: f64) -> Self {
    Self { x, y }
  }
  pub(crate) fn add(self, o: Self) -> Self {
    Self::new(self.x + o.x, self.y + o.y)
  }
  pub(crate) fn sub(self, o: Self) -> Self {
    Self::new(self.x - o.x, self.y - o.y)
  }
  pub(crate) fn scale(self, k: f64) -> Self {
    Self::new(self.x * k, self.y * k)
  }
  pub(crate) fn len(self) -> f64 {
    self.x.hypot(self.y)
  }
}

/// A pointer field.
#[derive(Debug, Default)]
pub(crate) struct Field {
  spec: PointerSpec,
}

impl Field {
  pub(crate) fn set(&mut self, spec: PointerSpec) {
    self.spec = spec;
  }

  /// Where the visual pointer goes when the hand moves it by `hand` from `from`.
  pub(crate) fn moved(&self, from: Vec2, hand: Vec2) -> Vec2 {
    let target = self.target_at(from);
    let gain = self.spec.gain() * target.map_or(1.0, |t| clamp_gain(t.gain.unwrap_or(1.0)));
    from.add(hand.scale(gain))
  }

  /// Where the real cursor must be for the visual pointer at `visual` to act on what is drawn
  /// under it.
  pub(crate) fn real(&self, visual: Vec2) -> Vec2 {
    if let Some(cut) = self.spec.cuts.iter().rev().find(|c| contains(c.shown, visual)) {
      return through_cut(cut, visual);
    }
    self
      .spec
      .lenses
      .iter()
      .rev()
      .find(|l| visual.sub(Vec2::new(l.x, l.y)).len() < l.r)
      .map_or(visual, |l| through_lens(l, visual))
  }

  /// The smallest target (grown by its reach) containing `p`.
  fn target_at(&self, p: Vec2) -> Option<&Target> {
    self
      .spec
      .targets
      .iter()
      .filter(|t| contains(grow(t.rect, t.reach.unwrap_or(0.0)), p))
      .min_by(|a, b| area(a.rect).total_cmp(&area(b.rect)))
  }
}

fn through_cut(cut: &Cut, p: Vec2) -> Vec2 {
  let (s, src) = (cut.shown, cut.source);
  let sx = if s.w > 0.0 { src.w / s.w } else { 1.0 };
  let sy = if s.h > 0.0 { src.h / s.h } else { 1.0 };
  Vec2::new(src.x + (p.x - s.x) * sx, src.y + (p.y - s.y) * sy)
}

/// The point of the screen a lens draws at `p`. Must match the lens shader.
fn through_lens(lens: &Lens, p: Vec2) -> Vec2 {
  let centre = Vec2::new(lens.x, lens.y);
  let d = p.sub(centre);
  let r = d.len();
  if lens.mag <= 1.0 || r >= lens.r {
    return p;
  }
  let band = smoothstep(lens.r * LENS_FLAT, lens.r, r);
  let scale = (1.0 / lens.mag) + (1.0 - 1.0 / lens.mag) * band;
  centre.add(d.scale(scale))
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
  let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
  t * t * (3.0 - 2.0 * t)
}

fn grow(r: Rect, by: f64) -> Rect {
  Rect {
    x: r.x - by,
    y: r.y - by,
    w: r.w + 2.0 * by,
    h: r.h + 2.0 * by,
  }
}

fn area(r: Rect) -> f64 {
  r.w * r.h
}

fn contains(r: Rect, p: Vec2) -> bool {
  p.x >= r.x && p.x < r.x + r.w && p.y >= r.y && p.y < r.y + r.h
}

#[cfg(test)]
mod tests {
  use super::*;

  fn near(a: Vec2, b: Vec2) -> bool {
    a.sub(b).len() < 1e-6
  }

  fn field(spec: PointerSpec) -> Field {
    let mut f = Field::default();
    f.set(spec);
    f
  }

  fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
  }

  #[test]
  fn identity_passes_motion_through_and_acts_in_place() {
    let f = field(PointerSpec::default());
    let p = f.moved(Vec2::new(10.0, 10.0), Vec2::new(3.0, -2.0));
    assert!(near(p, Vec2::new(13.0, 8.0)));
    assert!(near(f.real(p), p));
  }

  #[test]
  fn gain_and_targets_scale_motion() {
    let target = Target {
      rect: rect(100.0, 100.0, 20.0, 20.0),
      gain: Some(0.25),
      reach: Some(5.0),
    };
    let f = field(PointerSpec {
      gain: Some(2.0),
      targets: vec![target],
      ..PointerSpec::default()
    });
    assert!(near(f.moved(Vec2::new(90.0, 110.0), Vec2::new(4.0, 0.0)), Vec2::new(98.0, 110.0)));
    // In the reach margin: 2 * 0.25.
    assert!(near(f.moved(Vec2::new(96.0, 110.0), Vec2::new(4.0, 0.0)), Vec2::new(98.0, 110.0)));
  }

  #[test]
  fn a_cut_acts_on_its_source_scaled() {
    let f = field(PointerSpec {
      cuts: vec![Cut {
        shown: rect(500.0, 100.0, 200.0, 100.0),
        source: rect(10.0, 20.0, 100.0, 50.0),
      }],
      ..PointerSpec::default()
    });
    assert!(near(f.real(Vec2::new(600.0, 150.0)), Vec2::new(60.0, 45.0)));
    assert!(near(f.real(Vec2::new(499.0, 150.0)), Vec2::new(499.0, 150.0)));
  }

  #[test]
  fn the_topmost_cut_wins() {
    let shown = rect(0.0, 0.0, 10.0, 10.0);
    let f = field(PointerSpec {
      cuts: vec![
        Cut { shown, source: rect(100.0, 0.0, 10.0, 10.0) },
        Cut { shown, source: rect(200.0, 0.0, 10.0, 10.0) },
      ],
      ..PointerSpec::default()
    });
    assert!(near(f.real(Vec2::new(5.0, 5.0)), Vec2::new(205.0, 5.0)));
  }

  #[test]
  fn a_lens_acts_on_what_it_magnifies() {
    let f = field(PointerSpec {
      lenses: vec![Lens { x: 100.0, y: 50.0, r: 40.0, mag: 2.0 }],
      ..PointerSpec::default()
    });
    // The same numbers as the lens shader's test: 10pt right of centre shows 5pt right of it.
    assert!(near(f.real(Vec2::new(110.0, 50.0)), Vec2::new(105.0, 50.0)));
    assert!(near(f.real(Vec2::new(100.0, 50.0)), Vec2::new(100.0, 50.0)));
    assert!(near(f.real(Vec2::new(141.0, 50.0)), Vec2::new(141.0, 50.0)));
    // No seam at the rim, and the map never folds back on itself.
    let mut last = 100.0;
    for i in 1..=400 {
      let x = 100.0 + f64::from(i) * 0.1;
      let real = f.real(Vec2::new(x, 50.0)).x;
      assert!(real >= last, "monotonic at {x}");
      last = real;
    }
    assert!((last - 140.0).abs() < 0.01);
  }
}
