/*! Event types for state changes and synchronization. */

use super::{Element, ElementId, Point, Window, WindowId};
use serde::Serialize;
use ts_rs::TS;

/// Character range within text. End is exclusive, matching Rust's `Range` semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct TextRange {
  /// Start position (inclusive).
  pub start: u32,
  /// End position (exclusive).
  pub end: u32,
}

impl TextRange {
  /// Create a new text range.
  pub const fn new(start: u32, end: u32) -> Self {
    Self { start, end }
  }

  /// Length of the range in characters.
  pub const fn len(&self) -> u32 {
    self.end - self.start
  }

  /// Check if the range is empty (cursor position, no selection).
  pub const fn is_empty(&self) -> bool {
    self.start == self.end
  }

  /// Check if a position falls within this range.
  pub const fn contains(&self, position: u32) -> bool {
    position >= self.start && position < self.end
  }
}

impl From<(u32, u32)> for TextRange {
  fn from((start, end): (u32, u32)) -> Self {
    Self { start, end }
  }
}

/// Text selection within an element.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct TextSelection {
  pub element_id: ElementId,
  pub text: String,
  /// Character range. None if range is unknown.
  pub range: Option<TextRange>,
}

impl TextSelection {
  /// Length of the selection in characters.
  pub const fn len(&self) -> Option<u32> {
    match &self.range {
      Some(range) => Some(range.len()),
      None => None,
    }
  }

  /// Check if the selection text is empty.
  pub const fn is_empty(&self) -> bool {
    self.text.is_empty()
  }

  /// Check if a position falls within this selection's range.
  pub const fn contains(&self, position: u32) -> bool {
    match &self.range {
      Some(range) => range.contains(position),
      None => false,
    }
  }
}

/// Initial state sent on connection.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Snapshot {
  pub windows: Vec<Window>,
  pub elements: Vec<Element>,
  pub focused_window: Option<WindowId>,
  pub focused_element: Option<Element>,
  pub selection: Option<TextSelection>,
  /// Window IDs in z-order (front to back)
  pub z_order: Vec<WindowId>,
  /// Current mouse position
  pub mouse_position: Option<Point>,
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
  WindowAdded { window: Window },
  #[serde(rename = "window:changed")]
  WindowChanged { window: Window },
  #[serde(rename = "window:removed")]
  WindowRemoved { window_id: WindowId },

  // Element lifecycle (from RPC, watches)
  #[serde(rename = "element:added")]
  ElementAdded { element: Element },
  #[serde(rename = "element:changed")]
  ElementChanged { element: Element },
  #[serde(rename = "element:removed")]
  ElementRemoved { element_id: ElementId },

  // Window focus (from polling)
  #[serde(rename = "focus:window")]
  FocusWindow { window_id: Option<WindowId> },

  // Element focus (from Tier 1 app-level observer)
  #[serde(rename = "focus:element")]
  FocusElement {
    element: Element,
    previous_element_id: Option<ElementId>,
  },

  // Text selection (from Tier 1 app-level observer)
  #[serde(rename = "selection:changed")]
  SelectionChanged {
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    /// Character range. None if range is unknown.
    range: Option<TextRange>,
  },

  // Input tracking
  #[serde(rename = "mouse:position")]
  MousePosition(Point),

  // Subtree observation (from observation polling)
  #[serde(rename = "subtree:changed")]
  SubtreeChanged {
    root_id: ElementId,
    added: Vec<ElementId>,
    removed: Vec<ElementId>,
    modified: Vec<ElementId>,
  },
}
