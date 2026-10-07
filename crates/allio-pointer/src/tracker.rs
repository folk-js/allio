//! Turns the stream of real cursor positions into hand motion, moves the visual pointer by it,
//! and says where the real cursor has to go. Pure, so the race between our moves and the events
//! already on their way can be tested.
//!
//! We move the real cursor by posting a mouse event, which travels through the same stream as
//! the hardware's. So the stream itself says where the cursor was when each event happened: the
//! previous event's position, or, if our own move came through in between, where we put it. Each
//! event's hand motion is measured from there. (Event delta fields can't stand in for this: after
//! the cursor is moved they include the move.)

use crate::field::{Field, Vec2};

/// Farther than this in one event isn't the hand: something else moved the cursor.
const JUMP: f64 = 400.0;

/// What one move did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Step {
  /// Where the pointer appears.
  pub(crate) visual: Vec2,
  /// Where the real cursor must be.
  pub(crate) real: Vec2,
}

#[derive(Debug, Default)]
pub(crate) struct Tracker {
  visual: Option<Vec2>,
  /// Where the cursor was after the previous event in the stream, ours or the hardware's.
  cursor: Option<Vec2>,
}

impl Tracker {
  /// A hardware event in the stream saw the cursor at `seen`. `keep` keeps the pointer on the
  /// displays it may appear on; `clamp` keeps the real cursor on screen.
  pub(crate) fn moved(
    &mut self,
    field: &Field,
    seen: Vec2,
    keep: impl Fn(Vec2) -> Vec2,
    clamp: impl Fn(Vec2) -> Vec2,
  ) -> Step {
    let before = self.cursor.replace(seen);
    let (Some(visual), Some(before)) = (self.visual, before) else {
      return self.resync(field, keep(seen), clamp);
    };
    let hand = seen.sub(before);
    if hand.len() > JUMP {
      return self.resync(field, keep(seen), clamp);
    }
    let visual = keep(field.moved(visual, hand));
    self.visual = Some(visual);
    Step {
      visual,
      real: clamp(field.real(visual)),
    }
  }

  /// Our own move, putting the cursor at `at`, came through the stream.
  pub(crate) const fn landed(&mut self, at: Vec2) {
    self.cursor = Some(at);
  }

  /// Where the pointer appears, once anything has moved it.
  pub(crate) const fn visual(&self) -> Option<Vec2> {
    self.visual
  }

  /// Starts over from where the cursor really is: the pointer appears where it is.
  fn resync(&mut self, field: &Field, seen: Vec2, clamp: impl Fn(Vec2) -> Vec2) -> Step {
    self.visual = Some(seen);
    Step {
      visual: seen,
      real: clamp(field.real(seen)),
    }
  }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
  use super::*;
  use crate::spec::{Cut, Lens, PointerSpec, Rect};
  use std::collections::VecDeque;

  enum Item {
    Hardware(Vec2),
    Ours(Vec2),
  }

  /// The window server and the road to the tap. Every millisecond the hand moves the cursor and
  /// the event joins the stream. When the tap posts a move, the window server applies it `apply`
  /// ms later and it joins the stream then. The stream reaches the tap `lag` ms behind, in order.
  fn drive(spec: PointerSpec, start: Vec2, hand: Vec2, ms: u64, lag: u64, apply: u64) -> Vec<Step> {
    let mut field = Field::default();
    field.set(spec);
    let mut tracker = Tracker::default();
    let mut cursor = start;
    let mut posted: VecDeque<(u64, Vec2)> = VecDeque::new();
    let mut stream: VecDeque<(u64, Item)> = VecDeque::new();
    let mut steps = Vec::new();
    for now in 0..=(ms + lag + apply) {
      while posted.front().is_some_and(|(t, _)| t + apply <= now) {
        let Some((_, to)) = posted.pop_front() else { break };
        cursor = to;
        stream.push_back((now, Item::Ours(to)));
      }
      if now < ms {
        cursor = cursor.add(hand);
        stream.push_back((now, Item::Hardware(cursor)));
      }
      while stream.front().is_some_and(|(t, _)| t + lag <= now) {
        let Some((_, item)) = stream.pop_front() else { break };
        match item {
          Item::Ours(at) => tracker.landed(at),
          Item::Hardware(seen) => {
            let step = tracker.moved(&field, seen, |p| p, |p| p);
            if step.real.sub(seen).len() > 0.01 {
              posted.push_back((now, step.real));
            }
            steps.push(step);
          }
        }
      }
    }
    steps
  }

  fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect { x, y, w, h }
  }

  const TIMINGS: [(u64, u64); 5] = [(0, 0), (3, 0), (10, 0), (0, 2), (5, 3)];

  #[test]
  fn half_gain_moves_the_pointer_half_as_far_even_with_events_in_flight() {
    for (lag, apply) in TIMINGS {
      let spec = PointerSpec {
        gain: Some(0.5),
        ..PointerSpec::default()
      };
      let steps = drive(spec, Vec2::new(100.0, 100.0), Vec2::new(2.0, 0.0), 50, lag, apply);
      for w in steps.windows(2) {
        assert!((w[1].visual.x - w[0].visual.x - 1.0).abs() < 1e-9, "{lag}/{apply}: {w:?}");
        assert!((w[1].real.x - w[1].visual.x).abs() < 1e-9, "acts where it appears");
      }
    }
  }

  #[test]
  fn crossing_into_a_cut_moves_the_real_cursor_once_and_keeps_moving() {
    for (lag, apply) in TIMINGS {
      let spec = PointerSpec {
        cuts: vec![Cut {
          shown: rect(120.0, 0.0, 100.0, 200.0),
          source: rect(600.0, 0.0, 100.0, 200.0),
        }],
        ..PointerSpec::default()
      };
      let steps = drive(spec, Vec2::new(100.0, 100.0), Vec2::new(2.0, 0.0), 40, lag, apply);
      for w in steps.windows(2) {
        assert!((w[1].visual.x - w[0].visual.x - 2.0).abs() < 1e-9, "{lag}/{apply}: {w:?}");
      }
      let inside = steps.iter().filter(|s| s.visual.x >= 120.0);
      assert!(inside.clone().count() > 5);
      assert!(inside.into_iter().all(|s| (s.real.x - s.visual.x - 480.0).abs() < 1e-9));
    }
  }

  #[test]
  fn a_lens_slows_the_real_cursor_but_not_the_visual_one() {
    for (lag, apply) in TIMINGS {
      let spec = PointerSpec {
        lenses: vec![Lens { x: 200.0, y: 100.0, r: 80.0, mag: 3.0 }],
        ..PointerSpec::default()
      };
      let steps = drive(spec, Vec2::new(180.0, 100.0), Vec2::new(1.0, 0.0), 30, lag, apply);
      for w in steps.windows(2) {
        assert!((w[1].visual.x - w[0].visual.x - 1.0).abs() < 1e-9, "{lag}/{apply}: {w:?}");
        assert!((w[1].real.x - w[0].real.x - 1.0 / 3.0).abs() < 1e-9, "{lag}/{apply}: {w:?}");
      }
    }
  }

  #[test]
  fn something_else_moving_the_cursor_resyncs() {
    let field = Field::default();
    let mut tracker = Tracker::default();
    tracker.moved(&field, Vec2::new(0.0, 0.0), |p| p, |p| p);
    let step = tracker.moved(&field, Vec2::new(900.0, 0.0), |p| p, |p| p);
    assert_eq!(step.visual, Vec2::new(900.0, 0.0));
    let step = tracker.moved(&field, Vec2::new(904.0, 0.0), |p| p, |p| p);
    assert_eq!(step.visual, Vec2::new(904.0, 0.0));
  }
}
