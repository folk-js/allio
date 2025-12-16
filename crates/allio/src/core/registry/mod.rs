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

use crate::a11y::{Action, Role, Value};
use crate::platform::{AppNotificationHandle, Handle, Observer, WatchHandle};
use crate::types::{
  Bounds, Element, ElementId, Event, Point, ProcessId, TextRange, TextSelection, Window, WindowId,
};
use tree::ElementTree;

/// Result of attempting to set focused element.
pub(crate) enum FocusChange {
  /// Focus didn't change (process not found or same element already focused).
  Unchanged,
  /// Focus changed. Contains the previously focused element ID, if any.
  Changed(Option<ElementId>),
}

/// Per-process state.
pub(crate) struct CachedProcess {
  pub(crate) observer: Observer,
  pub(crate) app_handle: Handle,
  pub(crate) focused_element: Option<ElementId>,
  pub(crate) last_selection: Option<TextSelection>,
  /// Handle to app-level notifications. Cleaned up via Drop when process is removed.
  pub(crate) _app_notifications: Option<AppNotificationHandle>,
}

/// Per-window state.
pub(crate) struct CachedWindow {
  pub(crate) process_id: ProcessId,
  pub(crate) info: Window,
  pub(crate) handle: Option<Handle>,
  /// Cached root element ID. Set on first `window_root` call.
  pub(crate) root_element: Option<ElementId>,
}

/// Per-element state in the registry.
pub(crate) struct CachedElement {
  // === Identity & Hierarchy ===
  pub(crate) id: ElementId,
  pub(crate) window_id: WindowId,
  pub(crate) pid: ProcessId,
  pub(crate) is_root: bool,

  // === Platform handle & tree ===
  pub(crate) handle: Handle,
  /// Parent handle for tree linkage. None for root elements.
  pub(crate) parent_handle: Option<Handle>,

  // === Semantic properties ===
  pub(crate) role: Role,
  /// Raw platform role string for debugging (e.g., "`AXRadioGroup`", "`AXButton/AXCloseButton`")
  pub(crate) platform_role: String,

  // === Text properties ===
  pub(crate) label: Option<String>,
  pub(crate) description: Option<String>,
  pub(crate) placeholder: Option<String>,
  /// URL for links, file paths (Finder), documents
  pub(crate) url: Option<String>,

  // === Value ===
  pub(crate) value: Option<Value>,

  // === Geometry ===
  pub(crate) bounds: Option<Bounds>,

  // === States ===
  pub(crate) focused: Option<bool>,
  /// Whether the element is disabled (matches ARIA aria-disabled)
  pub(crate) disabled: bool,
  /// Selection state for items in lists/tables
  pub(crate) selected: Option<bool>,
  /// Expansion state for tree nodes, disclosure triangles
  pub(crate) expanded: Option<bool>,

  // === Table/Collection position ===
  /// Row index for cells/rows in tables (0-based)
  pub(crate) row_index: Option<usize>,
  /// Column index for cells in tables (0-based)
  pub(crate) column_index: Option<usize>,
  /// Total row count (for table containers)
  pub(crate) row_count: Option<usize>,
  /// Total column count (for table containers)
  pub(crate) column_count: Option<usize>,

  // === Actions ===
  pub(crate) actions: Vec<Action>,

  // === Identity ===
  /// Platform accessibility identifier (AXIdentifier on macOS).
  pub(crate) identifier: Option<String>,

  // === Hit Test Status ===
  /// True if this element is a fallback container from Chromium/Electron lazy init.
  pub(crate) is_fallback: bool,

  // === Registry metadata ===
  pub(crate) watch: Option<WatchHandle>,
  /// When this element was last refreshed from the OS.
  pub(crate) last_refreshed: std::time::Instant,
}

impl PartialEq for CachedElement {
  /// Compare semantic element data. Excludes registry metadata (handle, watch, `last_refreshed`).
  fn eq(&self, other: &Self) -> bool {
    self.id == other.id
      && self.window_id == other.window_id
      && self.pid == other.pid
      && self.is_root == other.is_root
      && self.role == other.role
      && self.platform_role == other.platform_role
      && self.label == other.label
      && self.description == other.description
      && self.placeholder == other.placeholder
      && self.url == other.url
      && self.value == other.value
      && self.bounds == other.bounds
      && self.focused == other.focused
      && self.disabled == other.disabled
      && self.selected == other.selected
      && self.expanded == other.expanded
      && self.row_index == other.row_index
      && self.column_index == other.column_index
      && self.row_count == other.row_count
      && self.column_count == other.column_count
      && self.actions == other.actions
      && self.identifier == other.identifier
      && self.is_fallback == other.is_fallback
  }
}

impl CachedElement {
  /// Create a `CachedElement` from platform attributes.
  pub(crate) fn from_attributes(
    id: ElementId,
    window_id: WindowId,
    pid: ProcessId,
    is_root: bool,
    handle: Handle,
    parent_handle: Option<Handle>,
    attrs: crate::platform::ElementAttributes,
  ) -> Self {
    Self {
      id,
      window_id,
      pid,
      is_root,
      handle,
      parent_handle,
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
      identifier: attrs.identifier,
      is_fallback: false,
      watch: None,
      last_refreshed: std::time::Instant::now(),
    }
  }

  /// Check if this element is stale according to the given max age.
  pub(crate) fn is_stale(&self, max_age: std::time::Duration) -> bool {
    self.last_refreshed.elapsed() > max_age
  }

  /// Refresh element data from platform attributes. Updates all semantic fields in place.
  /// Preserves: id, handle, `parent_handle`, watch. Updates: `last_refreshed`.
  pub(crate) fn refresh(&mut self, attrs: crate::platform::ElementAttributes) {
    self.role = attrs.role;
    self.platform_role = attrs.platform_role;
    self.label = attrs.title;
    self.description = attrs.description;
    self.placeholder = attrs.placeholder;
    self.url = attrs.url;
    self.value = attrs.value;
    self.bounds = attrs.bounds;
    self.focused = attrs.focused;
    self.disabled = attrs.disabled;
    self.selected = attrs.selected;
    self.expanded = attrs.expanded;
    self.row_index = attrs.row_index;
    self.column_index = attrs.column_index;
    self.row_count = attrs.row_count;
    self.column_count = attrs.column_count;
    self.actions = attrs.actions;
    self.identifier = attrs.identifier;
    self.is_fallback = false;
    self.last_refreshed = std::time::Instant::now();
  }
}

/// Internal state storage with automatic event emission.
pub(crate) struct Registry {
  // Event emission
  events_tx: Sender<Event>,

  // Primary collections
  pub(super) processes: HashMap<ProcessId, CachedProcess>,
  pub(super) windows: HashMap<WindowId, CachedWindow>,
  pub(super) elements: HashMap<ElementId, CachedElement>,

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
    if let Some(element) = super::adapters::build_element(self, id) {
      self.emit(Event::ElementAdded { element });
    }
  }

  /// Emit `ElementChanged` event.
  pub(crate) fn emit_element_changed(&self, id: ElementId) {
    if let Some(element) = super::adapters::build_element(self, id) {
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

  /// Set children for a tree node.
  pub(crate) fn tree_set_children(&mut self, parent: ElementId, children: Vec<ElementId>) {
    self.tree.set_children(parent, children);
  }

  /// Refresh an element's attributes and return whether it changed.
  /// Returns None if the element doesn't exist.
  pub(crate) fn refresh_element(
    &mut self,
    id: ElementId,
    attrs: crate::platform::ElementAttributes,
  ) -> Option<bool> {
    let elem = self.elements.get_mut(&id)?;

    // Check for meaningful change
    let old_value = elem.value.clone();
    let old_label = elem.label.clone();
    let old_bounds = elem.bounds;
    let old_focused = elem.focused;
    let old_selected = elem.selected;
    let old_expanded = elem.expanded;

    elem.refresh(attrs);

    let changed = elem.value != old_value
      || elem.label != old_label
      || elem.bounds != old_bounds
      || elem.focused != old_focused
      || elem.selected != old_selected
      || elem.expanded != old_expanded;

    if changed {
      self.emit_element_changed(id);
    }

    Some(changed)
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
  pub(crate) fn set_focused_element(&mut self, pid: ProcessId, element: Element) -> FocusChange {
    let Some(process) = self.processes.get_mut(&pid) else {
      return FocusChange::Unchanged;
    };

    let previous = process.focused_element;
    if previous == Some(element.id) {
      return FocusChange::Unchanged;
    }

    process.focused_element = Some(element.id);
    self.emit(Event::FocusElement {
      element,
      previous_element_id: previous,
    });
    FocusChange::Changed(previous)
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
  pub(crate) fn window_at_point(&self, x: f64, y: f64) -> Option<&CachedWindow> {
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
