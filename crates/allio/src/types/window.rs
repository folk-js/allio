/*! Window type representing an on-screen window. */

use super::{Bounds, ProcessId, SpaceId, WindowId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a window is, relative to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Presence {
  /// On screen: on a Space that is current.
  Here,
  /// On a Space that isn't current (another desktop, or a full-screen Space).
  Elsewhere,
  /// Minimised to the Dock.
  Minimised,
  /// Its app is hidden.
  Hidden,
}

/// A window. Windows persist while they exist, wherever they are: leaving the current Space
/// changes `presence`, it doesn't remove the window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Window {
  pub id: WindowId,
  pub title: String,
  pub app_name: String,
  pub bounds: Bounds,
  pub focused: bool,
  pub process_id: ProcessId,
  /// Z-order index: 0 = frontmost, higher = further back. Windows that aren't here come after
  /// every window that is.
  pub z_index: u32,
  /// Where the window is relative to the user.
  pub presence: Presence,
  /// The Spaces it is on: one, usually; none when minimised or hidden; several if it is on all
  /// desktops.
  pub spaces: Vec<SpaceId>,
}
