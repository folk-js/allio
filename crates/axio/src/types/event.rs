/*! Event types for state changes and synchronization. */

use super::{AXElement, AXWindow, ElementId, Point, WindowId};
use serde::Serialize;
use ts_rs::TS;

/// Text selection within an element.
///
/// The `range` tuple is `(start, end)` - character positions within the element's text.
/// End is exclusive, matching Rust's `Range` semantics.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TextSelection {
  pub element_id: ElementId,
  pub text: String,
  /// Character range as (start, end). End is exclusive. None if range is unknown.
  pub range: Option<(u32, u32)>,
}

impl TextSelection {
  /// Length of the selection in characters.
  pub const fn len(&self) -> Option<u32> {
    match self.range {
      Some((start, end)) => Some(end - start),
      None => None,
    }
  }

  /// Check if the selection is empty (cursor with no selection).
  pub fn is_empty(&self) -> bool {
    self.text.is_empty()
  }

  /// Check if a position falls within this selection's range.
  pub const fn contains(&self, position: u32) -> bool {
    match self.range {
      Some((start, end)) => position >= start && position < end,
      None => false,
    }
  }
}

/// Initial state sent on connection.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Snapshot {
  pub windows: Vec<AXWindow>,
  pub elements: Vec<AXElement>,
  pub focused_window: Option<WindowId>,
  pub focused_element: Option<AXElement>,
  pub selection: Option<TextSelection>,
  /// Window IDs in z-order (front to back)
  pub depth_order: Vec<WindowId>,
  /// Current mouse position
  pub mouse_position: Option<Point>,
  /// Whether accessibility permissions are granted
  pub accessibility_enabled: bool,
}

/// Events emitted when state changes.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "event", content = "data")]
#[ts(export)]
pub enum Event {
  // Initial sync (on connection)
  #[serde(rename = "sync:init")]
  SyncInit(Snapshot),

  // Window lifecycle (from polling)
  #[serde(rename = "window:added")]
  WindowAdded { window: AXWindow },
  #[serde(rename = "window:changed")]
  WindowChanged { window: AXWindow },
  #[serde(rename = "window:removed")]
  WindowRemoved { window_id: WindowId },

  // Element lifecycle (from RPC, watches)
  #[serde(rename = "element:added")]
  ElementAdded { element: AXElement },
  #[serde(rename = "element:changed")]
  ElementChanged { element: AXElement },
  #[serde(rename = "element:removed")]
  ElementRemoved { element_id: ElementId },

  // Window focus (from polling)
  #[serde(rename = "focus:window")]
  FocusWindow { window_id: Option<WindowId> },

  // Element focus (from Tier 1 app-level observer)
  #[serde(rename = "focus:element")]
  FocusElement {
    element: AXElement,
    previous_element_id: Option<ElementId>,
  },

  // Text selection (from Tier 1 app-level observer)
  #[serde(rename = "selection:changed")]
  SelectionChanged {
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    /// Character range as (start, end). End is exclusive. None if range is unknown.
    range: Option<(u32, u32)>,
  },

  // Input tracking
  #[serde(rename = "mouse:position")]
  MousePosition(Point),
}
