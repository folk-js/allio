/*! Spaces: desktops, and the places full-screen windows live. */

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Space identifier, unique for the login session. A full-screen Space gets a new one each time
/// a window goes full screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SpaceId(#[ts(type = "number")] pub u64);

/// What a Space is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SpaceKind {
  /// A desktop: a place, which the user arranges windows on.
  Desktop,
  /// A full-screen window (or split pair) on its own Space. It exists only while the window is
  /// full screen, so it is best named by its window.
  Fullscreen,
}

/// A Space, on one display.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Space {
  pub id: SpaceId,
  /// The display it is on (the display's UUID).
  pub display: String,
  pub kind: SpaceKind,
  /// Position on its display, from 0, in Mission Control's order.
  pub index: u32,
  /// Whether it is the Space currently shown on its display.
  pub current: bool,
}
