//! Accessibility actions.
//!
//! Actions are operations that can be performed on UI elements.
//! Platform-specific action strings are mapped in `platform/*/mapping.rs`.

use serde::{Deserialize, Serialize};

/// Platform-agnostic action that can be performed on an element.
///
/// Platform mappings (macOS kAX*Action, Windows UIA patterns) are handled
/// by the platform layer, not here. See `platform::map_action_from_platform`
/// and `platform::action_to_platform_string`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
  /// Primary activation (click, press).
  Press,

  /// Show context menu or dropdown.
  ShowMenu,

  /// Increase value (slider, stepper).
  Increment,

  /// Decrease value (slider, stepper).
  Decrement,

  /// Confirm/submit action.
  Confirm,

  /// Cancel operation.
  Cancel,

  /// Bring element to front / give focus.
  Raise,

  /// Pick/select from a list or menu.
  Pick,

  /// Expand a collapsed element (tree node, accordion).
  Expand,

  /// Collapse an expanded element.
  Collapse,

  /// Scroll to make element visible.
  ScrollToVisible,
}

impl Action {
  /// All known actions.
  pub const ALL: &'static [Self] = &[
    Self::Press,
    Self::ShowMenu,
    Self::Increment,
    Self::Decrement,
    Self::Confirm,
    Self::Cancel,
    Self::Raise,
    Self::Pick,
    Self::Expand,
    Self::Collapse,
    Self::ScrollToVisible,
  ];
}
