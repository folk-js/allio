/*! Window type representing an on-screen window. */

use super::{Bounds, ProcessId, WindowId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An on-screen window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Window {
  pub id: WindowId,
  pub title: String,
  pub app_name: String,
  pub bounds: Bounds,
  pub focused: bool,
  pub process_id: ProcessId,
  /// Z-order index: 0 = frontmost, higher = further back
  pub z_index: u32,
}
