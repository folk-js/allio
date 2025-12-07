# Registry Refactor Design

> Design document for refactoring the accessibility registry and platform code.

## Progress

### ✅ Phase 1: Accessibility Types (Complete)

- [x] Created `accessibility/` module with cross-platform types
  - `Role` - 36 roles with metadata methods (`is_writable()`, `is_focusable()`, etc.)
  - `Action` - 11 actions (Press, ShowMenu, Increment, Decrement, Confirm, Cancel, Raise, Pick, Expand, Collapse, ScrollToVisible)
  - `Value` - typed values with `From` impls
  - `Notification` - 7 notification types with `ALWAYS` and `for_watching()`
- [x] Created `platform/macos_platform/mapping.rs` with:
  - `ax_role::*`, `ax_action::*`, `ax_notification::*` constants
  - Bidirectional mapping functions (`role_from_macos`, `action_from_macos`, etc.)
- [x] Migrated `AXElement` to use new `Role`, `Action`, `Value` types
- [x] Removed old `AXRole`, `AXAction`, `AXValue` from `types.rs`
- [x] Regenerated TypeScript types
- [x] Updated `handles.rs` to use new types + `action_from_macos()`
- [x] Updated `macos.rs` to use `role_from_macos()` and `Role` methods

### ✅ Phase 2: Observer & Registry Unification (Complete)

**Goal:** Consolidate to ONE observer per process with unified callback routing.

- [x] Add `Notification::is_app_level()` method for clean dispatch
- [x] Unify callbacks: one `unified_observer_callback` that converts macOS string → `Notification`, then dispatches
- [x] Remove `AXNotification` enum (replaced by `Notification` + mapping)
- [x] Remove `APP_OBSERVERS` (observer moves to `ProcessState` in Registry)
- [x] Remove `AppState`, `APP_CONTEXTS`, `app_observer_callback`, `create_app_observer`
- [x] Remove `WRITABLE_ROLES` (replaced by `Role::is_writable()`)
- [x] Remove `ensure_app_observer`, `cleanup_dead_observers`
- [x] Remove old `subscribe_element_notifications`, `unsubscribe_element_notifications`
- [x] Subscribe app-level notifications (FocusChanged, SelectionChanged) when ProcessState created
- [x] Unified context type: `ObserverContext::Element(id)` or `ObserverContext::Process(pid)`
- [x] Focus tracking via `Registry::set_process_focus()` / `get_process_focus()`
- [x] Cascading cleanup already implemented

**Removed from `macos.rs`:**

- `AXNotification` enum
- `APP_CONTEXTS` + `register_app_context`, `unregister_app_context`, `lookup_app_context`
- `AppState` struct
- `APP_OBSERVERS` static
- `cleanup_dead_observers` fn
- `ensure_app_observer` fn
- `create_app_observer` fn
- `app_observer_callback` fn
- `WRITABLE_ROLES` const
- `subscribe_element_notifications` fn
- `unsubscribe_element_notifications` fn

**New architecture:**

- Single `OBSERVER_CONTEXTS` registry with `ObserverContext` enum (Element or Process)
- `unified_observer_callback` dispatches based on context type
- `subscribe_app_notifications()` called when ProcessState created
- `handle_app_focus_changed()` / `handle_app_selection_changed()` called from unified callback

### 🔲 Phase 3: Platform Organization (Future)

- [ ] Split `macos.rs` into sub-modules (observer, element, windows, attributes)
- [ ] Final cleanup pass

### 🔲 Future Exploration

**AXSelectedChildrenChanged** - Item selection in lists/tables

Currently we track:

- `AXFocusedUIElementChanged` → keyboard focus (app-level notification)
- `AXSelectedTextChanged` → text selection within text fields (app-level notification)

We do NOT track:

- `AXSelectedChildrenChanged` → which items are selected in a list/table/outline

This is an **element-level** notification (not app-level). You subscribe on a specific container element (list, table) and get notified when its selected children change.

Use case: Tracking which todo item is selected in Apple Notes, which row is selected in a table, etc.

To implement:

1. Add `SelectedChildrenChanged` to `Notification` enum
2. Map to `"AXSelectedChildrenChanged"` in platform mapping
3. Subscribe when watching list/table container elements
4. Emit event with the selected element IDs

## Goals

1. **Clear lifecycle management** - Process, window, and element lifecycles are explicit
2. **Proper cleanup** - Cascading removal when processes/windows go away
3. **Robust destruction tracking** - Always know when elements die
4. **Cleaner code organization** - Separate concerns into modules
5. **Flexible for future platforms** - Even if we couple to macOS now, design should accommodate others

## Current Problems

- **Two parallel registries**: `APP_OBSERVERS` and `ElementRegistry` with overlapping concerns
- **Observer confusion**: AXObserver is per-PID, but stored per-window
- **Unclear ownership**: Who owns what? When does cleanup happen?
- **No explicit lifecycle**: Process/window/element creation/destruction not modeled
- **Platform code is a grab bag**: `macos.rs` is ~1000 lines of unorganized functions

## Core Entities & Lifecycles

```
Process (ProcessId / PID)
├─ created: first window seen for this app
├─ destroyed: no windows remain (or process exits)
└─ owns: ONE AXObserver for all notifications

Window (WindowId)
├─ created: window enumeration sees it
├─ destroyed: window enumeration stops seeing it
├─ belongs to: one Process
└─ owns: set of ElementIds (the elements in this window)

Element (ElementId)
├─ created: discovered via API call (children, elementAt, focus, etc.)
├─ destroyed: AXUIElementDestroyed notification (or window removed)
├─ belongs to: one Window
└─ owns: platform handle, notification subscriptions
```

## File Structure

### Current State

```
crates/axio/src/
  lib.rs                    # Re-exports
  types.rs                  # AXElement, AXWindow, Event, IDs, Bounds, etc.

  accessibility/            # ✅ NEW - Cross-platform abstractions
    mod.rs
    role.rs                 # Role enum + metadata (writable, focusable, etc.)
    action.rs               # Action enum
    notification.rs         # Notification types
    value.rs                # Value types (String, Number, Boolean)

  element_registry.rs       # Current registry (to be replaced)
  window_registry.rs        # Window tracking
  events.rs                 # Event emission

  platform/
    mod.rs                  # Re-exports
    handles.rs              # ElementHandle, ObserverHandle
    macos.rs                # ~960 lines (to be split)
    macos_cf.rs             # CF helpers
    macos_windows.rs        # Window enumeration
    macos_platform/         # ✅ NEW - organized macOS code
      mod.rs
      mapping.rs            # ax_role/ax_action/ax_notification constants + bidirectional mapping
```

### Target State

```
crates/axio/src/
  lib.rs
  types.rs                  # IDs, Bounds, Event (slim)

  accessibility/            # ✅ Done
    mod.rs
    role.rs
    action.rs
    notification.rs
    value.rs

  registry/                 # 🔲 TODO - Unified state management
    mod.rs                  # Registry struct + public API
    process.rs              # ProcessState
    window.rs               # WindowState
    element.rs              # ElementState (internal)

  platform/
    mod.rs
    handles.rs
    macos_platform/         # 🔲 TODO - Complete migration
      mod.rs
      mapping.rs            # ✅ Done
      observer.rs           # AXObserver management & callbacks
      element.rs            # Element handle operations
      windows.rs            # Window enumeration (CGWindowList)
      attributes.rs         # Attribute fetching
```

## Key Types

> Note: The `accessibility/` module is now implemented. Code samples below are for reference.
> See actual implementation in `crates/axio/src/accessibility/`.

### accessibility/role.rs ✅

```rust
/// Semantic UI role (cross-platform)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
  // Containers
  Window, Group, List, Table, Tree, ScrollArea,

  // Interactive
  Button, Link, MenuItem,
  TextField, TextArea, SearchField, ComboBox,
  Checkbox, Switch, RadioButton,
  Slider, Stepper,

  // Static
  StaticText, Image, Heading,

  // Generic
  GenericContainer,
  Unknown,
}

/// What kind of value can be written to this role
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritableAs {
  NotWritable,
  String,
  Integer,
  Float,
  Boolean,
}

impl Role {
  pub fn writable_as(&self) -> WritableAs {
    match self {
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox => WritableAs::String,
      Self::Checkbox | Self::Switch | Self::RadioButton => WritableAs::Boolean,
      Self::Slider => WritableAs::Float,
      Self::Stepper => WritableAs::Integer,
      _ => WritableAs::NotWritable,
    }
  }

  pub fn is_writable(&self) -> bool {
    !matches!(self.writable_as(), WritableAs::NotWritable)
  }

  pub fn auto_watch_on_focus(&self) -> bool {
    // Watch value changes when focused text fields
    matches!(self.writable_as(), WritableAs::String)
  }

  pub fn is_focusable(&self) -> bool {
    matches!(self,
      Self::Button | Self::Link | Self::MenuItem |
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox |
      Self::Checkbox | Self::Switch | Self::RadioButton |
      Self::Slider | Self::Stepper |
      Self::List | Self::Table | Self::Tree
    )
  }

  pub fn is_container(&self) -> bool {
    matches!(self,
      Self::Window | Self::Group | Self::List | Self::Table |
      Self::Tree | Self::ScrollArea | Self::GenericContainer
    )
  }
}
```

### accessibility/notification.rs ✅

```rust
/// Notifications we can subscribe to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Notification {
  Destroyed,
  ValueChanged,
  TitleChanged,
  FocusChanged,
  SelectionChanged,
  BoundsChanged,
  ChildrenChanged,
}

impl Notification {
  /// Notifications to ALWAYS subscribe for any registered element
  pub const ALWAYS: &'static [Self] = &[Self::Destroyed];

  /// Additional notifications based on role (when watching)
  pub fn for_watching(role: Role) -> Vec<Self> {
    let mut notifs = vec![];

    if role.is_writable() {
      notifs.push(Self::ValueChanged);
    }

    if matches!(role, Role::Window) {
      notifs.push(Self::TitleChanged);
    }

    notifs
  }

  /// Whether this notification is subscribed at app/process level.
  ///
  /// App-level notifications are subscribed on the application element itself.
  /// Element-level notifications are subscribed per UI element.
  pub fn is_app_level(&self) -> bool {
    matches!(self, Self::FocusChanged | Self::SelectionChanged)
  }
}
```

### accessibility/element.rs (Future - currently in types.rs as AXElement)

```rust
/// The public element type (what we expose via API)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
  pub id: ElementId,
  pub window_id: WindowId,
  pub parent_id: Option<ElementId>,
  pub children: Option<Vec<ElementId>>,  // None = not yet discovered

  // Identity (fetched once, stable)
  pub role: Role,
  pub subrole: Option<String>,

  // State (may change, can be refreshed)
  pub label: Option<String>,
  pub value: Option<Value>,
  pub description: Option<String>,
  pub bounds: Option<Bounds>,
  pub focused: bool,
  pub enabled: bool,

  // Actions
  pub actions: Vec<Action>,
}
```

### accessibility/value.rs ✅

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Value {
  String(String),
  Integer(i64),
  Float(f64),
  Boolean(bool),
}

impl Value {
  pub fn as_str(&self) -> Option<&str> {
    match self { Self::String(s) => Some(s), _ => None }
  }
  // ... other accessors
}
```

## Registry Design

> Note: The unified Registry is not yet implemented. This section describes the target design.
> Current state uses `element_registry.rs` + `APP_OBSERVERS` in `macos.rs`.

### Design Decisions (Updated)

1. **Handles stay in Registry** - `ElementState` includes the platform handle (`ElementHandle`).
   The handle type is generic (defined in `platform/handles.rs`), not macOS-specific.
   This avoids extra indirection while keeping the type portable.

2. **Registry emits events** - Coupling to `events::emit()` is acceptable since events are core to axio.

3. **Dead hashes** - Cleaned up when window is removed (remove hashes for elements in that window).
   Accept unbounded growth otherwise. Future exploration: may not need dead_hashes at all if
   we can detect stale elements another way.

4. **Cascade behavior**:
   - Elements can be removed individually (e.g., `AXUIElementDestroyed` notification)
   - Window removal cascades to all elements in that window
   - Process removal cascades to all windows, which cascade to elements

### registry/mod.rs (Planned)

```rust
pub struct Registry {
  processes: HashMap<ProcessId, ProcessState>,
  windows: HashMap<WindowId, WindowState>,
  elements: HashMap<ElementId, ElementState>,

  // Reverse indexes
  window_to_process: HashMap<WindowId, ProcessId>,
  element_to_window: HashMap<ElementId, WindowId>,
  hash_to_element: HashMap<u64, ElementId>,

  // Dead tracking (prevent re-registration of destroyed elements)
  // Note: Pruned on window removal. May explore removing entirely in future.
  dead_hashes: HashSet<u64>,

  // Current focus
  focused_element: Option<ElementId>,
}

struct ProcessState {
  pid: u32,
  observer: ObserverHandle,  // One per process
}

struct WindowState {
  process_id: ProcessId,
  title: Option<String>,
}

struct ElementState {
  element: Element,
  handle: ElementHandle,  // Platform handle (generic type from platform/handles.rs)
  hash: u64,
  pid: u32,  // For observer operations
  subscriptions: HashSet<Notification>,  // Logical subscription state
}
```

### Registry Public API

```rust
impl Registry {
  // === Element Management (called by platform) ===

  /// Register a new element. Returns existing if hash matches.
  pub fn register(&mut self, element: Element, hash: u64) -> Option<ElementId>;

  /// Find element by platform hash
  pub fn find_by_hash(&self, hash: u64) -> Option<ElementId>;

  /// Check if hash is known dead
  pub fn is_dead(&self, hash: u64) -> bool;

  /// Update element's mutable state
  pub fn update_value(&mut self, id: ElementId, value: Value);
  pub fn update_label(&mut self, id: ElementId, label: String);
  pub fn update_bounds(&mut self, id: ElementId, bounds: Bounds);
  pub fn set_children(&mut self, id: ElementId, children: Vec<ElementId>);

  // === Subscriptions (logical state) ===

  /// Mark a notification as subscribed for this element
  pub fn mark_subscribed(&mut self, id: ElementId, notif: Notification);

  /// Mark a notification as unsubscribed
  pub fn mark_unsubscribed(&mut self, id: ElementId, notif: Notification);

  /// Check if element has any active subscriptions (beyond destruction)
  pub fn is_watched(&self, id: ElementId) -> bool;

  /// Get all active subscriptions for an element
  pub fn subscriptions(&self, id: ElementId) -> Option<&HashSet<Notification>>;

  // === Lifecycle Events (called by platform) ===

  /// Element was destroyed (notification received)
  pub fn handle_destroyed(&mut self, id: ElementId);

  /// Window was removed (from enumeration)
  pub fn remove_window(&mut self, id: WindowId);

  /// Process went away
  pub fn remove_process(&mut self, id: ProcessId);

  // === Focus ===

  pub fn set_focus(&mut self, id: ElementId);
  pub fn clear_focus(&mut self);

  // === Queries (called by API layer) ===

  pub fn get(&self, id: ElementId) -> Option<&Element>;
  pub fn get_all_in_window(&self, window_id: WindowId) -> Vec<&Element>;
  pub fn focused(&self) -> Option<ElementId>;
  pub fn watched_elements(&self) -> Vec<ElementId>;
}
```

### Cleanup Cascade

```rust
impl Registry {
  pub fn remove_window(&mut self, window_id: WindowId) {
    // Get all elements in this window
    let element_ids: Vec<_> = self.elements.iter()
      .filter(|(_, s)| s.element.window_id == window_id)
      .map(|(id, _)| *id)
      .collect();

    // Remove each element (adds to dead_hashes)
    for id in &element_ids {
      self.remove_element_internal(id);
    }

    // Remove window state
    self.windows.remove(&window_id);

    // Emit events
    for id in element_ids {
      emit(Event::ElementRemoved { element_id: id });
    }
    emit(Event::WindowRemoved { window_id });
  }

  pub fn remove_process(&mut self, process_id: ProcessId) {
    // Get all windows for this process
    let window_ids: Vec<_> = self.windows.iter()
      .filter(|(_, w)| w.process_id == process_id)
      .map(|(id, _)| *id)
      .collect();

    // Cascade to windows (which cascade to elements)
    for id in window_ids {
      self.remove_window(id);
    }

    // Remove process state
    self.processes.remove(&process_id);
  }
}
```

## Platform / Registry Interaction

### Dependency Direction

```
┌─────────────────────────────────────────────────────────┐
│                      API Layer                          │
│            (HTTP handlers, WebSocket, etc.)             │
└─────────────────────────────────────────────────────────┘
                           │ uses
                           ▼
┌─────────────────────────────────────────────────────────┐
│                       Registry                          │
│   • Element/Window/Process state                        │
│   • Emits Events                                        │
│   • NO platform imports                                 │
└─────────────────────────────────────────────────────────┘
                           ▲
                           │ calls public API
                           │
┌─────────────────────────────────────────────────────────┐
│                   Platform (macOS)                      │
│   • AXObserver management                               │
│   • Element handle operations                           │
│   • Attribute fetching                                  │
│   • Calls Registry.register(), Registry.set_focus()    │
└─────────────────────────────────────────────────────────┘
```

### Registry Stores Handles

Registry stores the `ElementHandle` directly in `ElementState`. The handle type is defined
in `platform/handles.rs` as a generic wrapper (on macOS it wraps `CFRetained<AXUIElement>`).
This keeps the code simpler and avoids synchronization between separate stores.

```rust
// In Registry
let handle = registry.get_handle(element_id)?;
handle.get_string("AXValue")  // Platform operation using handle from Registry
```

### Focus Change Flow (Example)

```rust
// platform/macos/observer.rs

fn handle_focus_notification(ax_element: CFRetained<AXUIElement>, pid: u32) {
  let handle = ElementHandle::new(ax_element);
  let hash = element_hash(&handle);

  with_registry(|registry| {
    // Check if dead
    if registry.is_dead(hash) {
      return;
    }

    // Find or register
    let element_id = if let Some(id) = registry.find_by_hash(hash) {
      id
    } else {
      // New element discovered via focus
      let window_id = determine_window(pid, &handle);
      let element = build_element_from_handle(&handle, window_id, pid, None);

      // Register stores element + handle together
      let id = registry.register(element, handle, hash, pid)?;

      // Subscribe to destruction (uses Notification type)
      registry.subscribe_destruction(id);

      id
    };

    registry.set_focus(element_id);
  });
}
```

### Unified Observer Callback

One observer per process, one callback that dispatches based on notification type.
Uses `Notification::is_app_level()` for clean platform-agnostic routing.

```rust
// platform/macos/observer.rs

unsafe extern "C-unwind" fn unified_callback(
  _observer: NonNull<AXObserver>,
  element: NonNull<AXUIElement>,
  notification: NonNull<CFString>,
  refcon: *mut c_void,
) {
  let notification_str = notification.as_ref().to_string();

  // Convert macOS string → our Notification type (platform boundary)
  let Some(notif) = notification_from_macos(&notification_str) else {
    log::warn!("Unknown notification: {}", notification_str);
    return;
  };

  let element_ref = CFRetained::retain(element);

  // Dispatch based on notification level (uses our abstraction, not platform strings)
  if notif.is_app_level() {
    // App-level: context is PID, element comes from callback param
    let Some(pid) = lookup_pid_context(refcon) else { return };
    handle_app_notification(pid, notif, element_ref);
  } else {
    // Element-level: context is ElementId
    let Some(element_id) = lookup_element_context(refcon) else { return };
    handle_element_notification(element_id, notif, element_ref);
  }
}

fn handle_app_notification(pid: u32, notif: Notification, ax_element: CFRetained<AXUIElement>) {
  match notif {
    Notification::FocusChanged => {
      // Register element if new, then emit FocusElement event
      handle_focus_changed(pid, ax_element);
    }
    Notification::SelectionChanged => {
      // Get selected text, emit SelectionChanged event
      handle_selection_changed(pid, ax_element);
    }
    _ => {}
  }
}

fn handle_element_notification(element_id: ElementId, notif: Notification, ax_element: CFRetained<AXUIElement>) {
  match notif {
    Notification::Destroyed => {
      // Registry removes element, adds to dead_hashes, emits event
      registry.handle_destroyed(element_id);
    }
    Notification::ValueChanged => {
      // Refresh value, emit ElementChanged event
      if let Some(value) = ElementHandle::new(ax_element).get_value() {
        registry.update_value(element_id, value);
      }
    }
    Notification::TitleChanged => {
      // Refresh title, emit ElementChanged event
    }
    _ => {}
  }
}
```

## Subscription Tracking

**Logical state** (which notifications) lives in Registry's `ElementState.subscriptions`.

**Operational state** (how to subscribe with macOS) uses the mapping functions:

```rust
// Registry method for subscribing
impl Registry {
  pub fn subscribe(&mut self, element_id: ElementId, notif: Notification) -> AxioResult<()> {
    let state = self.elements.get_mut(&element_id)?;
    let process = self.processes.get(&state.pid)?;

    // Convert our Notification to macOS string using mapping
    let notif_str = notification_to_macos(notif);

    // Subscribe with macOS API
    unsafe {
      process.observer.add_notification(
        state.handle.inner(),
        &notif_str,
        state.context_handle
      );
    }

    // Track logical state
    state.subscriptions.insert(notif);
    Ok(())
  }
}
```

## What About the Platform Trait?

We discussed a trait like:

```rust
pub trait Platform {
  type Handle: Clone + Send;
  fn element_hash(&self, handle: &Self::Handle) -> u64;
  fn get_attributes(&self, handle: &Self::Handle) -> Attributes;
  // ...
}
```

**Verdict**: Nice for documentation and potential testing, but not strictly necessary right now. We can add it later if we want to mock the platform for tests, or if we add Windows/Linux support.

For now, it's fine to have macOS-specific code that directly calls Registry. The important boundary is that **Registry doesn't import macOS types**.

## Migration Path

1. ✅ **Create `accessibility/` module** with Role, Action, Notification, Value
2. ✅ **Create `platform/macos_platform/mapping.rs`** with constants and bidirectional mapping
3. ✅ **Migrate types** - AXElement now uses Role, Action, Value; old types removed
4. 🔲 **Create `registry/` module** with the new unified Registry
5. 🔲 **Refactor `platform/macos/`** into sub-modules (observer, element, windows, etc.)
6. 🔲 **Update API layer** to use new Registry
7. 🔲 **Remove old `element_registry.rs`** and clean up `APP_OBSERVERS` usage
8. 🔲 **Test thoroughly** - element lifecycle, focus tracking, cleanup

## Design Decisions

### Registry Owns Everything

Registry stores:

- Element data (`Element`)
- Platform handle (`ElementHandle` - generic, not macOS-specific)
- Subscriptions (`HashSet<Notification>`)
- Context pointer for callbacks

```rust
struct ElementState {
  element: Element,
  handle: ElementHandle,
  hash: u64,
  pid: u32,
  subscriptions: HashSet<Notification>,
  context_handle: *mut c_void,  // For observer callbacks
}
```

This keeps everything in one place:

- No synchronization between separate stores
- API can answer "is element X watched?" directly
- Cleanup is straightforward (remove element = remove everything)

**Note:** Destruction notification is always implicitly subscribed for all elements.
Registry can represent this with a method rather than storing it:

```rust
impl Registry {
  pub fn is_destruction_tracked(&self, id: ElementId) -> bool {
    self.elements.contains_key(&id)  // All registered elements are tracked
  }
}
```

### Other Decisions

- **Window-less elements**: Not tracked (filtered in window polling already)
- **ElementId stability**: Not stable across restarts (generated fresh)
- **Process info to frontend**: No, frontend only sees windows and elements
