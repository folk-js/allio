/*! Event types for state changes and synchronization. */

use super::{AXElement, AXWindow, ElementId, Point, WindowId};
use serde::Serialize;
use ts_rs::TS;

/// Text selection within an element.
#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct Selection {
  pub element_id: ElementId,
  pub text: String,
  pub range: Option<TextRange>,
}

/// Initial state sent on connection.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Snapshot {
  pub windows: Vec<AXWindow>,
  pub elements: Vec<AXElement>,
  pub focused_window: Option<WindowId>,
  pub focused_element: Option<AXElement>,
  pub selection: Option<Selection>,
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
    range: Option<TextRange>,
  },

  // Input tracking
  #[serde(rename = "mouse:position")]
  MousePosition(Point),
}

/// Text selection range within an element.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize, TS)]
#[ts(export)]
pub struct TextRange {
  pub start: u32,
  pub length: u32,
}

impl TextRange {
  pub const fn new(start: u32, length: u32) -> Self {
    Self { start, length }
  }

  /// End position (exclusive).
  pub const fn end(&self) -> u32 {
    self.start + self.length
  }

  pub const fn is_empty(&self) -> bool {
    self.length == 0
  }

  /// Check if a position falls within this range.
  pub const fn contains(&self, position: u32) -> bool {
    position >= self.start && position < self.end()
  }
}

