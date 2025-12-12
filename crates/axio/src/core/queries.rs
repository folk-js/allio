/*!
Query methods for Axio.

## Naming Conventions

- `get(id, freshness)` = unified element access with explicit freshness
- `children(id, freshness)` = get children with freshness control
- `parent(id, freshness)` = get parent with freshness control
- `get_*` = internal registry/state lookups (fast, no OS calls)
- `fetch_*` = internal OS calls (deprecated in public API)

## Recency Model

The `get` method takes a `Recency` parameter that explicitly controls staleness:
- `Recency::Any` - use cached value, might be stale
- `Recency::Current` - always fetch from OS
- `Recency::MaxAge(duration)` - fetch if older than duration
*/

use super::builders::build_entry_from_handle;
use super::Axio;
use crate::platform::{CurrentPlatform, Handle, Platform};
use crate::types::{
  AxioError, AxioResult, Element, ElementId, ProcessId, Recency, TextSelection, Window, WindowId,
};

// ============================================================================
// Handle Helpers
// ============================================================================

impl Axio {
  /// Find the window ID for a handle.
  ///
  /// Uses a hybrid approach:
  /// 1. Fast path: Check if element is already cached, get window_id from it
  /// 2. Slow path: Query AXWindow attribute (1 FFI call), lookup window by handle
  ///
  /// Returns None if element has no window (menu bar, system tray, etc.) - this
  /// should not happen in our system as we only track elements in windows.
  pub(crate) fn window_for_handle(&self, handle: &Handle) -> Option<WindowId> {
    use crate::platform::PlatformHandle;

    // Fast path: element already cached
    if let Some(window_id) = self.read(|r| {
      r.find_element(handle)
        .and_then(|id| r.element(id))
        .map(|e| e.data.window_id)
    }) {
      return Some(window_id);
    }

    // Slow path: derive from handle's AXWindow attribute (1 FFI call)
    let window_handle = handle.window().or_else(|| {
      log::error!(
        "Element has no window (AXWindow returned None). \
         This should not happen - we only track elements in windows. PID: {}",
        handle.pid()
      );
      None
    })?;

    self.read(|r| r.find_window_by_handle(&window_handle))
  }

  /// Cache an element from a platform handle.
  ///
  /// This is the common pattern: build entry → upsert → ensure watched → return ID.
  /// Makes OS calls (via build_entry) but is a clean, composable helper.
  pub(crate) fn cache_from_handle(
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

// ============================================================================
// Unified Element Access
// ============================================================================

impl Axio {
  /// Get element with specified freshness.
  ///
  /// This is the primary way to access elements with explicit freshness control.
  ///
  /// # Arguments
  ///
  /// * `element_id` - The element to retrieve
  /// * `freshness` - How fresh the data should be
  ///
  /// # Examples
  ///
  /// ```ignore
  /// // Get from cache (fast, might be stale)
  /// let elem = axio.get(id, Recency::Any)?;
  ///
  /// // Always fetch from OS (slow, guaranteed fresh)
  /// let elem = axio.get(id, Recency::Current)?;
  ///
  /// // Fetch if older than 100ms
  /// let elem = axio.get(id, Recency::max_age_ms(100))?;
  /// ```
  pub fn get(&self, element_id: ElementId, freshness: Recency) -> AxioResult<Option<Element>> {
    match freshness {
      Recency::Any => {
        // Fast path: just read from cache
        Ok(self.read(|r| super::build_element(r, element_id)))
      }
      Recency::Current => {
        // Always refresh from OS
        if self.read(|r| r.element(element_id).is_some()) {
          self.refresh_element(element_id)?;
        }
        Ok(self.read(|r| super::build_element(r, element_id)))
      }
      Recency::MaxAge(max_age) => {
        // Check if stale, refresh if needed
        let needs_refresh = self.read(|r| {
          r.element(element_id)
            .map(|e| e.is_stale(max_age))
            .unwrap_or(false)
        });
        if needs_refresh {
          self.refresh_element(element_id)?;
        }
        Ok(self.read(|r| super::build_element(r, element_id)))
      }
    }
  }

  /// Get children of an element with specified freshness.
  ///
  /// - `Recency::Any` - return known children (might be incomplete if never fetched)
  /// - `Recency::Current` - fetch from OS, register new children
  /// - `Recency::MaxAge(d)` - fetch if children list is older than d
  ///
  /// Note: For Cached, if children have never been fetched, returns an empty vec.
  pub fn children(&self, element_id: ElementId, freshness: Recency) -> AxioResult<Vec<Element>> {
    match freshness {
      Recency::Any => {
        // Return cached children
        Ok(
          self
            .read(|r| {
              super::build_element(r, element_id).and_then(|e| {
                e.children.map(|ids| {
                  ids
                    .iter()
                    .filter_map(|id| super::build_element(r, *id))
                    .collect::<Vec<_>>()
                })
              })
            })
            .unwrap_or_default(),
        )
      }
      Recency::Current => {
        // Always fetch from OS
        self.fetch_children(element_id, usize::MAX)
      }
      Recency::MaxAge(max_age) => {
        // Check if children are stale
        // For now, treat children staleness same as element staleness
        let needs_refresh = self.read(|r| {
          r.element(element_id)
            .map(|e| e.is_stale(max_age))
            .unwrap_or(true)
        });
        if needs_refresh {
          self.fetch_children(element_id, usize::MAX)
        } else {
          self.children(element_id, Recency::Any)
        }
      }
    }
  }

  /// Get parent of an element with specified freshness.
  ///
  /// - `Recency::Any` - return known parent (None if never fetched)
  /// - `Recency::Current` - fetch from OS
  /// - `Recency::MaxAge(d)` - fetch if parent link is older than d
  ///
  /// Returns `Ok(None)` if element is root (has no parent).
  pub fn parent(&self, element_id: ElementId, freshness: Recency) -> AxioResult<Option<Element>> {
    match freshness {
      Recency::Any => {
        // Return cached parent
        Ok(self.read(|r| {
          super::build_element(r, element_id)
            .and_then(|e| e.parent_id)
            .and_then(|pid| super::build_element(r, pid))
        }))
      }
      Recency::Current => {
        // Always fetch from OS
        self.fetch_parent(element_id)
      }
      Recency::MaxAge(max_age) => {
        // Check if stale
        let needs_refresh = self.read(|r| {
          r.element(element_id)
            .map(|e| e.is_stale(max_age))
            .unwrap_or(true)
        });
        if needs_refresh {
          self.fetch_parent(element_id)
        } else {
          self.parent(element_id, Recency::Any)
        }
      }
    }
  }

  /// Refresh element data from OS (internal).
  ///
  /// Updates the cached element with fresh data from the platform.
  /// Returns the refreshed element.
  ///
  /// Use `get(id, Recency::Current)` for public API.
  pub(crate) fn refresh_element(&self, element_id: ElementId) -> AxioResult<Element> {
    use crate::platform::PlatformHandle;

    // Step 1: Extract handle and metadata (quick read, lock released)
    let (handle, window_id, pid, is_root) = self.element_handle(element_id)?;

    // Step 2: Platform call (NO LOCK)
    let attrs = handle.fetch_attributes();

    let updated_data =
      super::ElementData::from_attributes(element_id, window_id, ProcessId(pid), is_root, attrs);

    // Step 3: Update registry and build element
    self.write(|r| r.update_element(element_id, updated_data));
    self
      .read(|r| super::build_element(r, element_id))
      .ok_or(AxioError::ElementNotFound(element_id))
  }
}

// ============================================================================
// Registry Lookups
// ============================================================================

impl Axio {
  /// Get all windows from registry.
  pub fn all_windows(&self) -> Vec<Window> {
    self.read(|s| s.windows().map(|w| w.info.clone()).collect())
  }

  /// Get a specific window from registry.
  pub fn window(&self, window_id: WindowId) -> Option<Window> {
    self.read(|s| s.window(window_id).map(|w| w.info.clone()))
  }

  /// Get the focused window ID from registry.
  pub fn focused_window(&self) -> Option<WindowId> {
    self.read(|s| s.focused_window())
  }

  /// Get window z-order (front to back) from registry.
  pub fn z_order(&self) -> Vec<WindowId> {
    self.read(|s| s.z_order().to_vec())
  }

  /// Get all elements from registry.
  pub fn all_elements(&self) -> Vec<Element> {
    self.read(|s| super::builders::build_all_elements(s))
  }

  /// Get a snapshot of the current state for sync.
  pub fn snapshot(&self) -> crate::types::Snapshot {
    self.read(|s| super::build_snapshot(s))
  }

  /// Find window at a point from registry.
  pub(crate) fn window_at_point(&self, x: f64, y: f64) -> Option<Window> {
    self.read(|s| s.window_at_point(x, y).map(|w| w.info.clone()))
  }

  /// Get window info with handle from registry.
  pub(crate) fn window_with_handle(&self, window_id: WindowId) -> Option<(Window, Option<Handle>)> {
    self.read(|s| {
      let window = s.window(window_id)?;
      let handle = window.handle.clone();
      Some((window.info.clone(), handle))
    })
  }

  /// Get the app element handle for a process from registry.
  pub(crate) fn app_handle(&self, pid: u32) -> Option<Handle> {
    self.read(|s| s.process(ProcessId(pid)).map(|p| p.app_handle.clone()))
  }

  /// Get element handle and metadata. Use this to extract data before platform calls.
  /// Returns (handle, window_id, pid, is_root).
  pub(crate) fn element_handle(
    &self,
    element_id: ElementId,
  ) -> AxioResult<(Handle, WindowId, u32, bool)> {
    self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.data.window_id, e.pid(), e.data.is_root))
    })
  }
}

// ============================================================================
// Platform Fetches
// ============================================================================

impl Axio {
  /// Check if accessibility permissions are granted.
  pub fn has_permissions() -> bool {
    CurrentPlatform::has_permissions()
  }

  /// Get screen dimensions (width, height).
  ///
  /// Cached on first access for the lifetime of this Axio instance.
  ///
  /// TODO: Detect display configuration changes and update the cache.
  /// For now, assumes screen size is constant.
  pub fn screen_size(&self) -> (f64, f64) {
    *self
      .screen_size
      .get_or_init(|| CurrentPlatform::fetch_screen_size())
  }

  /// Get element at screen coordinates (always fresh from OS).
  ///
  /// Returns `Ok(None)` if no tracked window exists at the position.
  /// This is not an error - it's valid to query positions outside windows.
  ///
  /// # Chromium/Electron Apps
  ///
  /// Chromium/Electron apps lazily build their accessibility spatial index on a per-region
  /// basis. The first hit test at any coordinate triggers async initialization of that region,
  /// potentially returning a window-sized fallback container. When a fallback container is
  /// detected, the returned element has `is_fallback = true`. Clients should retry on the
  /// next frame to get the real element.
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

    // Get the app element handle from ProcessEntry
    let app_handle = self
      .app_handle(pid)
      .ok_or_else(|| AxioError::Internal(format!("Process {pid} not registered")))?;

    // Step 1: Platform call - get handle at position
    let element_handle = app_handle.fetch_element_at_position(x, y).ok_or_else(|| {
      AxioError::AccessibilityError(format!("No element at ({x}, {y}) in app {pid}"))
    })?;

    // Step 2: Cache element from handle
    let element_id = self.cache_from_handle(element_handle, window_id, ProcessId(pid));

    // Step 3: Build element with relationships
    let mut element = self
      .read(|r| super::build_element(r, element_id))
      .ok_or_else(|| {
        AxioError::AccessibilityError(format!("Element at ({x}, {y}) was previously destroyed"))
      })?;

    // Detect Chromium/Electron fallback container
    let is_fallback = matches!(element.role, Role::Group | Role::GenericGroup)
      && element
        .bounds
        .as_ref()
        .is_some_and(|b| b.matches(&window_bounds, 0.0));

    element.is_fallback = is_fallback;

    Ok(Some(element))
  }

  /// Fetch and register children of element from OS (internal).
  ///
  /// Prefer using `children(id, Recency::Current)` in public API.
  pub(crate) fn fetch_children(
    &self,
    element_id: ElementId,
    max_children: usize,
  ) -> AxioResult<Vec<Element>> {
    use crate::platform::PlatformHandle;

    // Step 1: Extract handle (quick read, lock released)
    let (handle, window_id, pid, _is_root) = self.element_handle(element_id)?;

    // Step 2: Platform call (NO LOCK)
    let child_handles = handle.fetch_children();

    if child_handles.is_empty() {
      self.write(|r| r.set_children(element_id, vec![]));
      return Ok(vec![]);
    }

    // Step 3: Cache each child from handle
    let mut children = Vec::new();
    let mut child_ids = Vec::new();

    for child_handle in child_handles.into_iter().take(max_children) {
      let child_id = self.cache_from_handle(child_handle, window_id, ProcessId(pid));
      if let Some(child) = self.read(|r| super::build_element(r, child_id)) {
        child_ids.push(child.id);
        children.push(child);
      }
    }

    // Step 4: Update parent's children list
    self.write(|r| r.set_children(element_id, child_ids));
    Ok(children)
  }

  /// Fetch and register parent of element from OS (internal).
  ///
  /// Prefer using `parent(id, Recency::Current)` in public API.
  /// Returns None if element is root.
  pub(crate) fn fetch_parent(&self, element_id: ElementId) -> AxioResult<Option<Element>> {
    use crate::platform::PlatformHandle;

    // Step 1: Extract handle (quick read, lock released)
    let (handle, window_id, pid, _is_root) = self.element_handle(element_id)?;

    // Step 2: Platform call (NO LOCK)
    let Some(parent_handle) = handle.fetch_parent() else {
      return Ok(None);
    };

    // Step 3: Cache parent from handle
    let parent_id = self.cache_from_handle(parent_handle, window_id, ProcessId(pid));
    Ok(self.read(|r| super::build_element(r, parent_id)))
  }

  /// Get root element for a window.
  ///
  /// Returns `Ok(None)` if the window doesn't exist or has no accessibility element.
  /// The root element is constant for the lifetime of a window, so this only
  /// hits the OS on the first call for each window. Subsequent calls return
  /// the cached element.
  pub fn window_root(&self, window_id: WindowId) -> AxioResult<Option<Element>> {
    // Fast path: return cached root if available
    if let Some(element_id) = self.read(|r| r.window_root(window_id)) {
      if let Some(element) = self.read(|r| super::build_element(r, element_id)) {
        return Ok(Some(element));
      }
      // Element was removed but window still exists - fall through to re-fetch
    }

    // Slow path: fetch from OS
    let Some((window, handle)) = self.window_with_handle(window_id) else {
      return Ok(None); // Window doesn't exist
    };

    let Some(window_handle) = handle else {
      return Ok(None); // Window has no accessibility element
    };

    // Cache element from handle
    let element_id =
      self.cache_from_handle(window_handle, window_id, ProcessId(window.process_id.0));

    // Store in window for future calls
    self.write(|r| r.set_window_root(window_id, element_id));

    Ok(self.read(|r| super::build_element(r, element_id)))
  }

  /// Get currently focused element and text selection for a window (always fresh from OS).
  pub fn window_focus(
    &self,
    window_id: WindowId,
  ) -> AxioResult<(Option<Element>, Option<TextSelection>)> {
    use crate::platform::PlatformHandle;

    let window = self
      .window(window_id)
      .ok_or(AxioError::WindowNotFound(window_id))?;
    let pid = window.process_id.0;

    // Get app handle from ProcessEntry
    let app_handle = self
      .app_handle(pid)
      .ok_or_else(|| AxioError::Internal(format!("Process {pid} not registered")))?;

    // No focused element is a legitimate state, not an error
    let Some(focused_handle) = CurrentPlatform::fetch_focused_element(&app_handle) else {
      return Ok((None, None));
    };

    // Try to get window ID from existing element or fall back to requested window
    let focus_window_id = self
      .read(|r| {
        r.find_element(&focused_handle)
          .and_then(|id| r.element(id))
          .map(|e| e.data.window_id)
      })
      .unwrap_or(window_id);

    // Cache element from handle
    let element_id =
      self.cache_from_handle(focused_handle.clone(), focus_window_id, ProcessId(pid));

    // Build element
    let element = self
      .read(|r| super::build_element(r, element_id))
      .ok_or_else(|| {
        AxioError::Internal("Focused element was destroyed during registration".to_string())
      })?;

    let selection = focused_handle
      .fetch_selection()
      .map(|(text, range)| TextSelection {
        element_id: element.id,
        text,
        range,
      });

    Ok((Some(element), selection))
  }
}
