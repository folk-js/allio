/*!
Registry - the single source of truth for cached accessibility data.

All fields are private. Mutations go through methods that maintain invariants
and emit events. This guarantees:
- Indexes are always updated
- Events are always emitted
- Cascades always happen

## Module Structure

- `mod.rs` - Registry struct, entry types, Global UI State
- `elements.rs` - Element CRUD, queries, element-specific ops, indexes
- `windows.rs` - Window CRUD, queries, window-specific ops
- `processes.rs` - Process CRUD, queries
- `tree.rs` - `ElementTree` for parent/child relationships
*/

mod elements;
mod processes;
mod tree;
mod windows;

use async_broadcast::Sender;
use std::collections::HashMap;

use crate::accessibility::{Action, Role, Value};
use crate::platform::{AppNotificationHandle, Handle, Observer, WatchHandle};
use crate::types::{
  Bounds, Element, ElementId, Event, Point, ProcessId, TextRange, TextSelection, Window, WindowId,
};
use tree::ElementTree;

/// Per-process state.
pub(crate) struct ProcessEntry {
  pub(crate) observer: Observer,
  pub(crate) app_handle: Handle,
  pub(crate) focused_element: Option<ElementId>,
  pub(crate) last_selection: Option<TextSelection>,
  /// Handle to app-level notifications. Cleaned up via Drop when process is removed.
  pub(crate) _app_notifications: Option<AppNotificationHandle>,
}

/// Per-window state.
pub(crate) struct WindowEntry {
  pub(crate) process_id: ProcessId,
  pub(crate) info: Window,
  pub(crate) handle: Option<Handle>,
  /// Cached root element ID. Set on first `window_root` call.
  pub(crate) root_element: Option<ElementId>,
}

/// Pure element data without tree relationships.
#[derive(Debug, Clone)]
pub(crate) struct ElementData {
  pub id: ElementId,
  pub window_id: WindowId,
  pub pid: ProcessId,
  pub is_root: bool,
  pub role: Role,
  pub platform_role: String,
  pub label: Option<String>,
  pub description: Option<String>,
  pub placeholder: Option<String>,
  pub url: Option<String>,
  pub value: Option<Value>,
  pub bounds: Option<Bounds>,
  pub focused: Option<bool>,
  pub disabled: bool,
  pub selected: Option<bool>,
  pub expanded: Option<bool>,
  pub row_index: Option<usize>,
  pub column_index: Option<usize>,
  pub row_count: Option<usize>,
  pub column_count: Option<usize>,
  pub actions: Vec<Action>,
  pub is_fallback: bool,
}

impl ElementData {
  /// Create `ElementData` from platform attributes.
  pub(crate) fn from_attributes(
    id: ElementId,
    window_id: WindowId,
    pid: ProcessId,
    is_root: bool,
    attrs: crate::platform::ElementAttributes,
  ) -> Self {
    Self {
      id,
      window_id,
      pid,
      is_root,
      role: attrs.role,
      platform_role: attrs.platform_role,
      label: attrs.title,
      description: attrs.description,
      placeholder: attrs.placeholder,
      url: attrs.url,
      value: attrs.value,
      bounds: attrs.bounds,
      focused: attrs.focused,
      disabled: attrs.disabled,
      selected: attrs.selected,
      expanded: attrs.expanded,
      row_index: attrs.row_index,
      column_index: attrs.column_index,
      row_count: attrs.row_count,
      column_count: attrs.column_count,
      actions: attrs.actions,
      is_fallback: false,
    }
  }
}

/// Per-element state in the registry.
pub(crate) struct ElementEntry {
  pub(crate) data: ElementData,
  pub(crate) handle: Handle,
  /// Parent handle for tree linkage. None for root elements.
  pub(crate) parent_handle: Option<Handle>,
  pub(crate) watch: Option<WatchHandle>,
  /// When this element was last refreshed from the OS.
  pub(crate) last_refreshed: std::time::Instant,
}

impl ElementEntry {
  pub(crate) fn new(data: ElementData, handle: Handle, parent_handle: Option<Handle>) -> Self {
    Self {
      data,
      handle,
      parent_handle,
      watch: None,
      last_refreshed: std::time::Instant::now(),
    }
  }

  pub(crate) const fn pid(&self) -> u32 {
    self.data.pid.0
  }

  /// Check if this element is stale according to the given max age.
  pub(crate) fn is_stale(&self, max_age: std::time::Duration) -> bool {
    self.last_refreshed.elapsed() > max_age
  }

  /// Mark this element as freshly refreshed.
  pub(crate) fn mark_refreshed(&mut self) {
    self.last_refreshed = std::time::Instant::now();
  }
}

/// Internal state storage with automatic event emission.
pub(crate) struct Registry {
  // Event emission
  events_tx: Sender<Event>,

  // Primary collections
  pub(super) processes: HashMap<ProcessId, ProcessEntry>,
  pub(super) windows: HashMap<WindowId, WindowEntry>,
  pub(super) elements: HashMap<ElementId, ElementEntry>,

  // Tree structure - single source of truth for relationships
  pub(super) tree: ElementTree,

  // Indexes - Handle implements Hash (cached CFHash) + Eq (CFEqual on collision)
  pub(super) handle_to_id: HashMap<Handle, ElementId>,
  pub(super) waiting_for_parent: HashMap<Handle, Vec<ElementId>>,
  /// Window handle → `WindowId` index for O(1) lookup from element's `AXWindow` handle.
  pub(super) window_handle_to_id: HashMap<Handle, WindowId>,

  // Focus/UI state
  focused_window: Option<WindowId>,
  pub(super) z_order: Vec<WindowId>,
  mouse_position: Option<Point>,
}

impl Registry {
  pub(crate) fn new(events_tx: Sender<Event>) -> Self {
    Self {
      events_tx,
      processes: HashMap::new(),
      windows: HashMap::new(),
      elements: HashMap::new(),
      tree: ElementTree::new(),
      handle_to_id: HashMap::new(),
      waiting_for_parent: HashMap::new(),
      window_handle_to_id: HashMap::new(),
      focused_window: None,
      z_order: Vec::new(),
      mouse_position: None,
    }
  }

  /// Emit an event.
  pub(super) fn emit(&self, event: Event) {
    if let Err(e) = self.events_tx.try_broadcast(event) {
      if e.is_full() {
        log::error!(
          "Event channel overflow - events are being dropped. \
           Consider increasing EVENT_CHANNEL_CAPACITY or processing events faster."
        );
      }
    }
  }

  /// Emit `ElementAdded` event (used by elements.rs).
  pub(super) fn emit_element_added(&self, id: ElementId) {
    if let Some(element) = super::builders::build_element(self, id) {
      self.emit(Event::ElementAdded { element });
    }
  }

  /// Emit `ElementChanged` event (used by elements.rs).
  pub(super) fn emit_element_changed(&self, id: ElementId) {
    if let Some(element) = super::builders::build_element(self, id) {
      self.emit(Event::ElementChanged { element });
    }
  }

  /// Get parent from tree.
  pub(crate) fn tree_parent(&self, id: ElementId) -> Option<ElementId> {
    self.tree.parent(id)
  }

  /// Get children from tree.
  pub(crate) fn tree_children(&self, id: ElementId) -> &[ElementId] {
    self.tree.children(id)
  }

  /// Check if element has children tracked in tree.
  pub(crate) fn tree_has_children(&self, id: ElementId) -> bool {
    self.tree.has_children(id)
  }
}

impl Registry {
  /// Set focused window. Emits `FocusWindow` if changed.
  pub(crate) fn set_focused_window(&mut self, id: Option<WindowId>) {
    if self.focused_window == id {
      return;
    }
    self.focused_window = id;
    self.emit(Event::FocusWindow { window_id: id });
  }

  /// Get focused window.
  pub(crate) const fn focused_window(&self) -> Option<WindowId> {
    self.focused_window
  }

  /// Set focused element for a process. Emits `FocusElement` if changed.
  /// Returns previous element ID if changed (for auto-unwatch).
  pub(crate) fn set_focused_element(
    &mut self,
    pid: ProcessId,
    element: Element,
  ) -> Option<Option<ElementId>> {
    let Some(process) = self.processes.get_mut(&pid) else {
      return None;
    };

    let previous = process.focused_element;
    if previous == Some(element.id) {
      return None; // No change
    }

    process.focused_element = Some(element.id);
    self.emit(Event::FocusElement {
      element,
      previous_element_id: previous,
    });
    Some(previous)
  }

  /// Set selection. Emits `SelectionChanged` if changed.
  pub(crate) fn set_selection(
    &mut self,
    pid: ProcessId,
    window_id: WindowId,
    element_id: ElementId,
    text: String,
    range: Option<(u32, u32)>,
  ) {
    let new_selection = TextSelection {
      element_id,
      text: text.clone(),
      range: range.map(TextRange::from),
    };

    let Some(process) = self.processes.get_mut(&pid) else {
      return;
    };

    if process.last_selection.as_ref() == Some(&new_selection) {
      return;
    }

    process.last_selection = Some(new_selection);
    self.emit(Event::SelectionChanged {
      window_id,
      element_id,
      text,
      range: range.map(TextRange::from),
    });
  }

  /// Update mouse position. Emits `MousePosition` if changed significantly.
  pub(crate) fn set_mouse_position(&mut self, pos: Point) {
    let changed = self
      .mouse_position
      .is_none_or(|last| pos.moved_from(last, 1.0));
    if !changed {
      return;
    }
    self.mouse_position = Some(pos);
    self.emit(Event::MousePosition(pos));
  }

  /// Get mouse position.
  pub(crate) const fn mouse_position(&self) -> Option<Point> {
    self.mouse_position
  }

  /// Get z-order (front to back).
  pub(crate) fn z_order(&self) -> &[WindowId] {
    &self.z_order
  }

  /// Find window at point. Returns topmost window containing the point.
  pub(crate) fn window_at_point(&self, x: f64, y: f64) -> Option<&WindowEntry> {
    let point = Point::new(x, y);
    for window_id in &self.z_order {
      if let Some(window) = self.windows.get(window_id) {
        if window.info.bounds.contains(point) {
          return Some(window);
        }
      }
    }
    None
  }
}
