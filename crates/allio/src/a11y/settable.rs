/*!
Attributes an element's app reports as writable.

Writability comes from the app (macOS `AXUIElementIsAttributeSettable`), not from
the element's role: the same role can be writable in one app and read-only in
another, and setting a non-settable attribute often "succeeds" without effect.
*/

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// An attribute allio can write, when the app allows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum SettableAttribute {
  /// The element's value (text, number, boolean…).
  Value,
  /// Selection state (rows, cells, list items).
  Selected,
  /// Expansion state (tree nodes, disclosure).
  Expanded,
}

impl SettableAttribute {
  /// All settable attributes allio knows about.
  pub const ALL: &'static [Self] = &[Self::Value, Self::Selected, Self::Expanded];
}
