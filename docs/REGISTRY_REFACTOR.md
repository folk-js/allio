# Registry Refactor Design

> Design document for refactoring the accessibility registry and platform code.

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

## Proposed File Structure

```
crates/axio/src/
  lib.rs                    # Re-exports

  accessibility/            # Cross-platform abstractions
    mod.rs
    role.rs                 # Role enum + metadata (writable, focusable, etc.)
    action.rs               # Action enum
    notification.rs         # Notification types
    value.rs                # Value types (String, Number, Boolean)
    element.rs              # Element struct (the public data type)

  registry/                 # Unified state management
    mod.rs                  # Registry struct + public API
    process.rs              # ProcessState
    window.rs               # WindowState
    element.rs              # ElementState (internal, includes handle)

  platform/                 # Platform implementations
    mod.rs                  # Shared types, maybe Platform trait
    macos/                  # macOS-specific
      mod.rs                # Re-exports
      observer.rs           # AXObserver management & callbacks
      element.rs            # Element handle operations
      windows.rs            # Window enumeration (CGWindowList)
      attributes.rs         # Attribute fetching
      mapping.rs            # Platform role/action → our types
```

## Key Types

### accessibility/role.rs

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

### accessibility/notification.rs

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
}

impl Notification {
  /// Notifications to ALWAYS subscribe for any registered element
  pub const fn always() -> &'static [Self] {
    &[Self::Destroyed]
  }

  /// Additional notifications based on role (when watching)
  pub fn for_role(role: Role) -> Vec<Self> {
    let mut notifs = vec![];

    if role.is_writable() {
      notifs.push(Self::ValueChanged);
    }

    if matches!(role, Role::Window) {
      notifs.push(Self::TitleChanged);
    }

    notifs
  }
}
```

### accessibility/element.rs

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

### accessibility/value.rs

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

### registry/mod.rs

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
  dead_hashes: HashSet<u64>,

  // Current focus
  focused_element: Option<ElementId>,
}

struct ProcessState {
  pid: u32,
  // Observer lives in Platform, not here
}

struct WindowState {
  process_id: ProcessId,
  title: Option<String>,
}

struct ElementState {
  element: Element,
  hash: u64,
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

### Platform Stores Handles

The Registry stores `Element` (the data) and logical subscription state.
The Platform stores the actual `AXUIElement` handles for operations.

```rust
// platform/macos/handles.rs
static HANDLES: LazyLock<Mutex<HashMap<ElementId, ElementHandle>>> = ...;

fn store_handle(id: ElementId, handle: ElementHandle) { ... }
fn get_handle(id: &ElementId) -> Option<ElementHandle> { ... }
fn remove_handle(id: &ElementId) { ... }
```

This keeps Registry decoupled from platform types while Platform has
everything it needs to perform operations.

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
      let id = registry.register(element, hash)?;

      // Store handle (Option A)
      store_handle(id, handle.clone());

      // Subscribe to destruction
      subscribe_destruction(id, &handle, pid);

      id
    };

    registry.set_focus(element_id);
  });
}
```

### Destruction Notification Flow

```rust
// platform/macos/observer.rs

unsafe extern "C-unwind" fn observer_callback(...) {
  let element_id = lookup_context(refcon)?;
  let notification_name = notification.as_ref().to_string();

  match notification_name.as_str() {
    "AXUIElementDestroyed" => {
      // Clean up platform state
      remove_handle(&element_id);
      remove_subscription(&element_id);

      // Tell registry
      with_registry(|registry| {
        registry.handle_destroyed(element_id);
      });
    }

    "AXValueChanged" => {
      if let Some(handle) = get_handle(&element_id) {
        if let Some(value) = handle.get_value() {
          with_registry(|registry| {
            registry.update_value(element_id, value);
          });
        }
      }
    }
    // ...
  }
}
```

## Subscription Tracking

**Logical state** (which notifications) lives in Registry's `ElementState.subscriptions`.

**Operational state** (how to deliver) lives in Platform:

```rust
// platform/macos/subscription.rs

struct OperationalState {
  observer: ObserverHandle,
  context_handle: *mut c_void,
}

static SUBSCRIPTIONS: LazyLock<Mutex<HashMap<ElementId, OperationalState>>> = ...;

pub fn subscribe(element_id: ElementId, handle: &ElementHandle, notif: Notification, pid: u32) {
  // Get or create operational state
  let mut subs = SUBSCRIPTIONS.lock();
  let state = subs.entry(element_id).or_insert_with(|| {
    let observer = get_or_create_observer(pid);
    let context = register_context(element_id);
    OperationalState { observer, context_handle: context }
  });

  // Actually subscribe with macOS
  let notif_str = notif.to_macos_string();
  unsafe {
    state.observer.add_notification(handle.inner(), &notif_str, state.context_handle);
  }

  // Update Registry's logical state
  with_registry(|r| r.mark_subscribed(element_id, notif));
}

pub fn unsubscribe_all(element_id: &ElementId) {
  let mut subs = SUBSCRIPTIONS.lock();
  if let Some(state) = subs.remove(element_id) {
    // Unsubscribe from macOS
    // ...
    unregister_context(state.context_handle);
  }

  // Registry cleanup happens via handle_destroyed()
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

1. **Create `accessibility/` module** with Role, Action, Notification, Value, Element
2. **Create `registry/` module** with the new unified Registry
3. **Refactor `platform/macos/`** into sub-modules (observer, element, windows, etc.)
4. **Update API layer** to use new Registry
5. **Remove old `element_registry.rs`** and clean up `APP_OBSERVERS` usage
6. **Test thoroughly** - element lifecycle, focus tracking, cleanup

## Design Decisions

### Subscription State: Registry (not Platform)

The Registry tracks _logical_ subscription state (which notifications are active per element).
The Platform tracks _operational_ state (observer handles, context pointers).

```rust
// registry/element.rs
struct ElementState {
  element: Element,
  hash: u64,
  subscriptions: HashSet<Notification>,  // Logical state in Registry
}

// platform/macos/subscription.rs
struct OperationalState {
  observer: ObserverHandle,
  context: *mut c_void,
}
static SUBSCRIPTIONS: HashMap<ElementId, OperationalState>;  // Operational state in Platform
```

**Why Registry knows:**

- Complete picture of element lifecycle in one place
- API can answer "is element X watched?" without platform call
- Enables features like "list all watched elements" or "watch all matching X"
- Registry is the source of truth; Platform is the mechanism

**Flow:**

```rust
// When subscribing
platform::subscribe(element_id, handle, Notification::ValueChanged);
registry.mark_subscribed(element_id, Notification::ValueChanged);

// When unsubscribing
platform::unsubscribe(element_id, Notification::ValueChanged);
registry.mark_unsubscribed(element_id, Notification::ValueChanged);

// Query
registry.is_watched(element_id) -> bool
registry.subscriptions(element_id) -> &HashSet<Notification>
```

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
