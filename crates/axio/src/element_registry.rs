use crate::platform::{self, AXNotification, ElementHandle, ObserverContextHandle, ObserverHandle};
use crate::types::{AXElement, AxioError, AxioResult, ElementId, WindowId};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::LazyLock;

/// Watch state for an element (notification subscriptions).
struct WatchState {
  /// Handle pointer passed to platform callbacks.
  context_handle: *mut c_void,
  notifications: Vec<AXNotification>,
}

/// Internal storage - AXElement plus platform handle and watch state.
pub struct StoredElement {
  /// The element data (what we return)
  pub element: AXElement,
  /// Platform element handle (opaque)
  pub handle: ElementHandle,
  /// Process ID
  pub pid: u32,
  /// Platform role string (for watch notifications)
  pub platform_role: String,
  /// Watch state if subscribed
  watch_state: Option<WatchState>,
}

/// Per-window state: elements and shared observer.
struct WindowState {
  elements: HashMap<ElementId, StoredElement>,
  /// Hash -> ElementId for O(1) duplicate detection (CFHash)
  by_hash: HashMap<u64, ElementId>,
  observer: Option<ObserverHandle>,
}

static ELEMENT_REGISTRY: LazyLock<Mutex<ElementRegistry>> =
  LazyLock::new(|| Mutex::new(ElementRegistry::new()));

pub struct ElementRegistry {
  windows: HashMap<WindowId, WindowState>,
  element_to_window: HashMap<ElementId, WindowId>,
}

// SAFETY: ElementRegistry is protected by a Mutex, and the raw pointers it contains
// (context handles in WatchState) are only accessed while holding the lock.
// The pointed-to data is managed by the context registry which has its own synchronization.
unsafe impl Send for ElementRegistry {}
unsafe impl Sync for ElementRegistry {}

impl ElementRegistry {
  fn new() -> Self {
    Self {
      windows: HashMap::new(),
      element_to_window: HashMap::new(),
    }
  }

  fn with<F, R>(f: F) -> R
  where
    F: FnOnce(&mut ElementRegistry) -> R,
  {
    let mut guard = ELEMENT_REGISTRY.lock();
    f(&mut guard)
  }

  /// Register element, returning existing if equivalent (stable IDs).
  /// Uses CFHash for O(1) duplicate detection.
  pub fn register(
    element: AXElement,
    handle: ElementHandle,
    pid: u32,
    platform_role: &str,
  ) -> AXElement {
    Self::with(|registry| {
      let window_id = element.window_id;

      let window_state = registry
        .windows
        .entry(window_id)
        .or_insert_with(|| WindowState {
          elements: HashMap::new(),
          by_hash: HashMap::new(),
          observer: None,
        });

      // O(1) duplicate check via CFHash
      let hash = platform::element_hash(&handle);
      if let Some(existing_id) = window_state.by_hash.get(&hash) {
        if let Some(stored) = window_state.elements.get(existing_id) {
          return stored.element.clone();
        }
      }

      // Store element
      let stored = StoredElement {
        element: element.clone(),
        handle,
        pid,
        platform_role: platform_role.to_string(),
        watch_state: None,
      };

      window_state.by_hash.insert(hash, element.id);
      window_state.elements.insert(element.id, stored);
      registry.element_to_window.insert(element.id, window_id);

      element
    })
  }

  /// Get element by ID (cached).
  pub fn get(element_id: &ElementId) -> AxioResult<AXElement> {
    Self::with(|registry| {
      let window_id = registry
        .element_to_window
        .get(element_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      registry
        .windows
        .get(window_id)
        .and_then(|w| w.elements.get(element_id))
        .map(|s| s.element.clone())
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))
    })
  }

  /// Get multiple elements by ID.
  pub fn get_many(element_ids: &[ElementId]) -> Vec<AXElement> {
    Self::with(|registry| {
      element_ids
        .iter()
        .filter_map(|id| {
          let window_id = registry.element_to_window.get(id)?;
          registry
            .windows
            .get(window_id)
            .and_then(|w| w.elements.get(id))
            .map(|s| s.element.clone())
        })
        .collect()
    })
  }

  /// Get all elements in registry (for initial sync).
  pub fn get_all() -> Vec<AXElement> {
    Self::with(|registry| {
      registry
        .windows
        .values()
        .flat_map(|w| w.elements.values().map(|s| s.element.clone()))
        .collect()
    })
  }

  /// Access stored element (for internal ops like click, write).
  pub fn with_stored<F, R>(element_id: &ElementId, f: F) -> AxioResult<R>
  where
    F: FnOnce(&StoredElement) -> R,
  {
    Self::with(|registry| {
      let window_id = registry
        .element_to_window
        .get(element_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      registry
        .windows
        .get(window_id)
        .and_then(|w| w.elements.get(element_id))
        .map(f)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))
    })
  }

  /// Update element's cached data (e.g., after refresh).
  pub fn update(element_id: &ElementId, updated: AXElement) -> AxioResult<()> {
    Self::with(|registry| {
      let window_id = *registry
        .element_to_window
        .get(element_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      let stored = registry
        .windows
        .get_mut(&window_id)
        .and_then(|w| w.elements.get_mut(element_id))
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      stored.element = updated;
      Ok(())
    })
  }

  /// Update children for an element.
  pub fn set_children(element_id: &ElementId, children: Vec<ElementId>) -> AxioResult<()> {
    Self::with(|registry| {
      let window_id = *registry
        .element_to_window
        .get(element_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      let stored = registry
        .windows
        .get_mut(&window_id)
        .and_then(|w| w.elements.get_mut(element_id))
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      stored.element.children = Some(children);
      Ok(())
    })
  }

  pub fn remove_element(element_id: &ElementId) {
    Self::with(|registry| {
      let Some(window_id) = registry.element_to_window.remove(element_id) else {
        return;
      };

      let Some(window_state) = registry.windows.get_mut(&window_id) else {
        return;
      };

      if let Some(mut stored) = window_state.elements.remove(element_id) {
        // Remove from hash index
        let hash = platform::element_hash(&stored.handle);
        window_state.by_hash.remove(&hash);

        if let Some(ref observer) = window_state.observer {
          unwatch_element(&mut stored, observer.clone());
        }
      }
    });
  }

  pub fn remove_window_elements(window_id: &WindowId) {
    Self::with(|registry| {
      let Some(mut window_state) = registry.windows.remove(window_id) else {
        return;
      };

      if let Some(ref observer) = window_state.observer {
        for (_, mut stored) in window_state.elements.drain() {
          unwatch_element(&mut stored, observer.clone());
        }
      }

      registry.element_to_window.retain(|_, wid| wid != window_id);
    });
  }

  pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
    Self::with_stored(element_id, |stored| write_value(stored, text))?
  }

  pub fn click(element_id: &ElementId) -> AxioResult<()> {
    Self::with_stored(element_id, click_element)?
  }

  pub fn watch(element_id: &ElementId) -> AxioResult<()> {
    Self::with(|registry| {
      let window_id = *registry
        .element_to_window
        .get(element_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      let window_state = registry
        .windows
        .get_mut(&window_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      let stored = window_state
        .elements
        .get(element_id)
        .ok_or_else(|| AxioError::ElementNotFound(*element_id))?;

      let pid = stored.pid;

      // Get or create observer using get_or_insert_with pattern
      let observer = match &window_state.observer {
        Some(obs) => obs.clone(),
        None => {
          let obs = platform::create_observer_for_pid(pid)?;
          window_state.observer.insert(obs).clone()
        }
      };

      // Re-borrow as mutable after observer setup
      let stored = window_state
        .elements
        .get_mut(element_id)
        .expect("element must exist - verified above");
      watch_element(stored, observer)
    })
  }

  pub fn unwatch(element_id: &ElementId) {
    Self::with(|registry| {
      let Some(window_id) = registry.element_to_window.get(element_id) else {
        return;
      };

      let Some(window_state) = registry.windows.get_mut(window_id) else {
        return;
      };

      let Some(ref observer) = window_state.observer else {
        return;
      };

      if let Some(stored) = window_state.elements.get_mut(element_id) {
        unwatch_element(stored, observer.clone());
      }
    });
  }
}

// --- Element operations (delegate to platform) ---

fn write_value(stored: &StoredElement, text: &str) -> AxioResult<()> {
  platform::write_element_value(&stored.handle, text, &stored.platform_role)
}

fn click_element(stored: &StoredElement) -> AxioResult<()> {
  platform::click_element(&stored.handle)
}

fn watch_element(stored: &mut StoredElement, observer: ObserverHandle) -> AxioResult<()> {
  if stored.watch_state.is_some() {
    return Ok(());
  }

  let (context_handle, notifications) = platform::subscribe_element_notifications(
    &stored.element.id,
    &stored.handle,
    &stored.platform_role,
    observer,
  )?;

  if notifications.is_empty() {
    return Ok(());
  }

  stored.watch_state = Some(WatchState {
    context_handle: context_handle as *mut c_void,
    notifications,
  });

  Ok(())
}

fn unwatch_element(stored: &mut StoredElement, observer: ObserverHandle) {
  let Some(watch_state) = stored.watch_state.take() else {
    return;
  };

  platform::unsubscribe_element_notifications(
    &stored.handle,
    observer,
    watch_state.context_handle as *mut ObserverContextHandle,
    &watch_state.notifications,
  );
}
