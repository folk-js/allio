/*!
Query methods for Axio.

## Naming Conventions

- `get(id, recency)` = unified element access with explicit recency
- `children(id, recency)` = get children with recency control
- `parent(id, recency)` = get parent with recency control
- `get_*` = internal registry/state lookups (fast, no OS calls)
- `fetch_*` = internal OS calls (deprecated in public API)

## Recency Model

The `get` method takes a `Recency` parameter that explicitly controls staleness:
- `Recency::Any` - use cached value, might be stale
- `Recency::Current` - always fetch from OS
- `Recency::MaxAge(duration)` - fetch if older than duration
*/

use super::adapters::build_entry_from_handle;
use super::Axio;
use crate::platform::{CurrentPlatform, Handle, Platform};
use crate::types::{
  AxioError, AxioResult, Element, ElementId, ProcessId, Recency, Window, WindowId,
};

impl Axio {
  /// Find the window ID for a handle.
  pub(crate) fn window_for_handle(&self, handle: &Handle) -> Option<WindowId> {
    use crate::platform::PlatformHandle;

    // Fast path: element already cached
    if let Some(window_id) = self.read(|r| {
      r.find_element(handle)
        .and_then(|id| r.element(id))
        .map(|e| e.window_id)
    }) {
      return Some(window_id);
    }

    let window_handle = handle.window().or_else(|| {
      log::error!(
        "Element has no window (AXWindow returned None). PID: {}",
        handle.pid()
      );
      None
    })?;

    self.read(|r| r.find_window_by_handle(&window_handle))
  }

  /// Cache an element from a platform handle.
  pub(crate) fn upsert_from_handle(
    &self,
    handle: Handle,
    window_id: WindowId,
    pid: ProcessId,
  ) -> ElementId {
    let entry = build_entry_from_handle(handle, window_id, pid);
    let element_id = self.write(|r| r.upsert_element(entry));
    self.ensure_watched(element_id);
    element_id
  }
}

impl Axio {
  /// Get element with specified recency.
  #[must_use = "this returns a Result that may contain an element"]
  pub fn get(&self, element_id: ElementId, recency: Recency) -> AxioResult<Element> {
    match recency {
      Recency::Any => {
        // Fast path: just read from cache
        self
          .read(|r| super::build_element(r, element_id))
          .ok_or(AxioError::ElementNotFound(element_id))
      }
      Recency::Current => {
        // Always refresh from OS
        if self.read(|r| r.element(element_id).is_some()) {
          self.refresh_element(element_id)?;
        }
        self
          .read(|r| super::build_element(r, element_id))
          .ok_or(AxioError::ElementNotFound(element_id))
      }
      Recency::MaxAge(max_age) => {
        // Check if stale, refresh if needed
        let needs_refresh =
          self.read(|r| r.element(element_id).is_some_and(|e| e.is_stale(max_age)));
        if needs_refresh {
          self.refresh_element(element_id)?;
        }
        self
          .read(|r| super::build_element(r, element_id))
          .ok_or(AxioError::ElementNotFound(element_id))
      }
    }
  }

  /// Get children of an element with specified recency.
  #[must_use = "this returns a Result that may contain elements"]
  pub fn children(&self, element_id: ElementId, recency: Recency) -> AxioResult<Vec<Element>> {
    match recency {
      Recency::Any => Ok(self.read(|r| {
        r.tree_children(element_id)
          .iter()
          .filter_map(|id| super::build_element(r, *id))
          .collect()
      })),
      Recency::Current => self.fetch_children(element_id, usize::MAX),
      Recency::MaxAge(max_age) => {
        let needs_refresh =
          self.read(|r| r.element(element_id).is_none_or(|e| e.is_stale(max_age)));
        if needs_refresh {
          self.fetch_children(element_id, usize::MAX)
        } else {
          self.children(element_id, Recency::Any)
        }
      }
    }
  }

  /// Get parent of an element with specified recency.
  /// Returns `Ok(None)` if element is root.
  #[must_use = "this returns a Result that may contain an element"]
  pub fn parent(&self, element_id: ElementId, recency: Recency) -> AxioResult<Option<Element>> {
    match recency {
      Recency::Any => Ok(self.read(|r| {
        super::build_element(r, element_id)
          .and_then(|e| e.parent_id)
          .and_then(|pid| super::build_element(r, pid))
      })),
      Recency::Current => self.fetch_parent(element_id),
      Recency::MaxAge(max_age) => {
        let needs_refresh =
          self.read(|r| r.element(element_id).is_none_or(|e| e.is_stale(max_age)));
        if needs_refresh {
          self.fetch_parent(element_id)
        } else {
          self.parent(element_id, Recency::Any)
        }
      }
    }
  }

  /// Refresh element data from OS.
  pub(crate) fn refresh_element(&self, element_id: ElementId) -> AxioResult<Element> {
    use crate::platform::PlatformHandle;

    let handle = self.read(|r| {
      r.element(element_id)
        .map(|e| e.handle.clone())
        .ok_or(AxioError::ElementNotFound(element_id))
    })?;

    let attrs = handle.fetch_attributes();

    self.write(|r| {
      if let Some(elem) = r.elements.get_mut(&element_id) {
        elem.refresh(attrs);
      }
    });

    self
      .read(|r| super::build_element(r, element_id))
      .ok_or(AxioError::ElementNotFound(element_id))
  }

  /// Get all windows.
  pub fn all_windows(&self) -> Vec<Window> {
    self.read(|s| s.windows().map(|w| w.info.clone()).collect())
  }

  /// Get a specific window.
  pub fn window(&self, window_id: WindowId) -> Option<Window> {
    self.read(|s| s.window(window_id).map(|w| w.info.clone()))
  }

  /// Get the focused window ID.
  pub fn focused_window(&self) -> Option<WindowId> {
    self.read(super::registry::Registry::focused_window)
  }

  /// Get window z-order (front to back).
  pub fn z_order(&self) -> Vec<WindowId> {
    self.read(|s| s.z_order().to_vec())
  }

  /// Get all elements.
  pub fn all_elements(&self) -> Vec<Element> {
    self.read(super::adapters::build_all_elements)
  }

  /// Get a snapshot of the current state.
  pub fn snapshot(&self) -> crate::types::Snapshot {
    self.read(super::build_snapshot)
  }

  /// Find window at a point.
  pub(crate) fn window_at_point(&self, x: f64, y: f64) -> Option<Window> {
    self.read(|s| s.window_at_point(x, y).map(|w| w.info.clone()))
  }

  /// Get window info with handle.
  pub(crate) fn window_with_handle(&self, window_id: WindowId) -> Option<(Window, Option<Handle>)> {
    self.read(|s| {
      let window = s.window(window_id)?;
      let handle = window.handle.clone();
      Some((window.info.clone(), handle))
    })
  }

  /// Get the app element handle for a process.
  pub(crate) fn app_handle(&self, pid: u32) -> Option<Handle> {
    self.read(|s| s.process(ProcessId(pid)).map(|p| p.app_handle.clone()))
  }

  /// Get element handle and metadata.
  pub(crate) fn element_handle(
    &self,
    element_id: ElementId,
  ) -> AxioResult<(Handle, WindowId, u32, bool)> {
    self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.window_id, e.pid.0, e.is_root))
    })
  }

  /// Check if accessibility permissions are granted.
  pub fn has_permissions() -> bool {
    CurrentPlatform::has_permissions()
  }

  /// Get screen dimensions (width, height). Cached on first access.
  pub fn screen_size(&self) -> (f64, f64) {
    *self
      .screen_size
      .get_or_init(CurrentPlatform::fetch_screen_size)
  }

  /// Get element at screen coordinates (always fresh from OS).
  /// Returns `Ok(None)` if no tracked window exists at the position.
  ///
  /// For Chromium/Electron apps, the first hit test may return a fallback container
  /// while the accessibility tree initializes. Check `is_fallback` and retry if true.
  #[must_use = "this returns a Result that may contain an element"]
  pub fn element_at(&self, x: f64, y: f64) -> AxioResult<Option<Element>> {
    use crate::accessibility::Role;
    use crate::platform::PlatformHandle;

    // Find which TRACKED window is at this point
    let Some(window) = self.window_at_point(x, y) else {
      return Ok(None);
    };
    let window_id = window.id;
    let window_bounds = window.bounds;
    let pid = window.process_id.0;

    // Get the app element handle from cached process
    let app_handle = self
      .app_handle(pid)
      .ok_or(AxioError::ProcessNotFound(ProcessId(pid)))?;

    let element_handle = app_handle
      .fetch_element_at_position(x, y)
      .ok_or(AxioError::NoElementAtPosition { x, y })?;

    let element_id = self.upsert_from_handle(element_handle, window_id, ProcessId(pid));
    let mut element = self
      .read(|r| super::build_element(r, element_id))
      .ok_or(AxioError::ElementNotFound(element_id))?;

    // Detect Chromium/Electron fallback container
    let is_fallback = matches!(element.role, Role::Group | Role::GenericGroup)
      && element
        .bounds
        .as_ref()
        .is_some_and(|b| b.matches(&window_bounds, 0.0));

    element.is_fallback = is_fallback;

    Ok(Some(element))
  }

  /// Fetch and register children of element from OS.
  pub(crate) fn fetch_children(
    &self,
    element_id: ElementId,
    max_children: usize,
  ) -> AxioResult<Vec<Element>> {
    use crate::platform::PlatformHandle;

    let (handle, window_id, pid, _is_root) = self.element_handle(element_id)?;
    let child_handles = handle.fetch_children();

    if child_handles.is_empty() {
      self.write(|r| r.set_children(element_id, vec![]));
      return Ok(vec![]);
    }

    let cap = child_handles.len().min(max_children);
    let mut children = Vec::with_capacity(cap);
    let mut child_ids = Vec::with_capacity(cap);

    for child_handle in child_handles.into_iter().take(max_children) {
      let child_id = self.upsert_from_handle(child_handle, window_id, ProcessId(pid));
      if let Some(child) = self.read(|r| super::build_element(r, child_id)) {
        child_ids.push(child.id);
        children.push(child);
      }
    }

    self.write(|r| r.set_children(element_id, child_ids));
    Ok(children)
  }

  /// Fetch and register parent of element from OS. Returns None if element is root.
  pub(crate) fn fetch_parent(&self, element_id: ElementId) -> AxioResult<Option<Element>> {
    use crate::platform::PlatformHandle;

    let (handle, window_id, pid, _is_root) = self.element_handle(element_id)?;
    let Some(parent_handle) = handle.fetch_parent() else {
      return Ok(None);
    };

    let parent_id = self.upsert_from_handle(parent_handle, window_id, ProcessId(pid));
    Ok(self.read(|r| super::build_element(r, parent_id)))
  }

  /// Get root element for a window. Cached after first fetch.
  #[must_use = "this returns a Result that may contain an element"]
  pub fn window_root(&self, window_id: WindowId) -> AxioResult<Option<Element>> {
    // Fast path: return cached root if available
    if let Some(element_id) = self.read(|r| r.window_root(window_id)) {
      if let Some(element) = self.read(|r| super::build_element(r, element_id)) {
        return Ok(Some(element));
      }
    }

    let Some((window, handle)) = self.window_with_handle(window_id) else {
      return Ok(None);
    };
    let Some(window_handle) = handle else {
      return Ok(None);
    };

    let element_id =
      self.upsert_from_handle(window_handle, window_id, ProcessId(window.process_id.0));
    self.write(|r| r.set_window_root(window_id, element_id));

    Ok(self.read(|r| super::build_element(r, element_id)))
  }
}
