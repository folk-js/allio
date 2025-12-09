/*!
Element operations that coordinate between platform and core state.

These functions:
- Take an Axio reference (need state access)
- Use `PlatformHandle` trait methods internally
- Build and register elements
*/

use crate::accessibility::Role;
use crate::core::{Axio, ElementState};
use crate::types::{AXElement, AxioError, AxioResult, ElementId, ProcessId, WindowId};

use super::{Handle, PlatformHandle};

// === Element Building ===

/// Build element state from a handle (pure - no side effects).
pub(crate) fn build_element_state(
  handle: Handle,
  window_id: WindowId,
  pid: u32,
  parent_id: Option<ElementId>,
) -> ElementState {
  let attrs = handle.fetch_attributes();

  // Determine if this is a root element (parent is Application)
  let is_root = handle
    .fetch_parent()
    .map(|p| p.fetch_attributes().role == Role::Application)
    .unwrap_or(false);

  let parent_id_value = if is_root { None } else { parent_id };

  let hash = handle.element_hash();
  let parent_hash = if is_root {
    None
  } else {
    handle.fetch_parent().map(|p| p.element_hash())
  };

  let element = AXElement {
    id: ElementId::new(),
    window_id,
    pid: ProcessId(pid),
    is_root,
    parent_id: parent_id_value,
    children: None,
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
  };

  ElementState::new(element, handle, hash, parent_hash)
}

/// Build and register an element (convenience wrapper).
pub(crate) fn build_and_register_element(
  axio: &Axio,
  handle: Handle,
  window_id: WindowId,
  pid: u32,
  parent_id: Option<ElementId>,
) -> Option<AXElement> {
  let elem_state = build_element_state(handle, window_id, pid, parent_id);
  axio.register_element(elem_state)
}

// === Element Discovery ===

/// Fetch and register children of an element from platform.
pub(crate) fn fetch_children(
  axio: &Axio,
  parent_id: ElementId,
  max_children: usize,
) -> AxioResult<Vec<AXElement>> {
  // Step 1: Extract handle (quick read, lock released)
  let (handle, window_id, pid) = axio.get_element_handle(parent_id)?;

  // Step 2: Platform call (NO LOCK)
  let child_handles = handle.fetch_children();

  if child_handles.is_empty() {
    axio.set_element_children(parent_id, vec![])?;
    return Ok(vec![]);
  }

  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for child_handle in child_handles.into_iter().take(max_children) {
    if let Some(child) =
      build_and_register_element(axio, child_handle, window_id, pid, Some(parent_id))
    {
      child_ids.push(child.id);
      children.push(child);
    }
  }

  axio.set_element_children(parent_id, child_ids)?;
  Ok(children)
}

/// Fetch and register parent of an element from platform.
pub(crate) fn fetch_parent(axio: &Axio, element_id: ElementId) -> AxioResult<Option<AXElement>> {
  // Step 1: Extract handle (quick read, lock released)
  let (handle, window_id, pid) = axio.get_element_handle(element_id)?;

  // Step 2: Platform call (NO LOCK)
  let parent_handle = handle.fetch_parent();

  let Some(parent_handle) = parent_handle else {
    return Ok(None);
  };

  Ok(build_and_register_element(
    axio,
    parent_handle,
    window_id,
    pid,
    None,
  ))
}

/// Fetch fresh attributes for an element from platform.
pub(crate) fn fetch_element(axio: &Axio, element_id: ElementId) -> AxioResult<AXElement> {
  // Step 1: Extract handle and metadata (quick read, lock released)
  let (handle, window_id, pid, is_root, parent_id, existing_children) =
    axio.get_element_for_refresh(element_id)?;

  // Step 2: Platform call (NO LOCK)
  let attrs = handle.fetch_attributes();

  let updated = AXElement {
    id: element_id,
    window_id,
    pid: ProcessId(pid),
    is_root,
    parent_id,
    children: existing_children,
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
  };

  axio.update_element(element_id, updated.clone())?;
  Ok(updated)
}

// === Window/Hit Testing ===

/// Get the root element for a window.
pub(crate) fn fetch_window_root(axio: &Axio, window_id: WindowId) -> AxioResult<AXElement> {
  let (window, handle) = axio
    .get_window_with_handle(window_id)
    .ok_or(AxioError::WindowNotFound(window_id))?;

  let window_handle =
    handle.ok_or_else(|| AxioError::Internal(format!("Window {window_id} has no AX element")))?;

  build_and_register_element(axio, window_handle, window_id, window.process_id.0, None)
    .ok_or_else(|| AxioError::Internal("Window root element was previously destroyed".to_string()))
}

/// Retry delays (in ms) for Chromium/Electron lazy accessibility initialization.
///
/// Chromium/Electron apps lazily build their accessibility spatial index on a per-region
/// basis. The first hit test at any coordinate triggers async initialization of that region,
/// returning a window-sized fallback container. Subsequent queries return the actual element.
const HIT_TEST_RETRY_DELAYS_MS: [u64; 3] = [0, 10, 25];
const HIT_TEST_MAX_DEPTH: u8 = 10;

/// Get the accessibility element at a specific screen position.
/// Uses retry logic to handle Chromium/Electron lazy initialization.
pub(crate) fn fetch_element_at_position(axio: &Axio, x: f64, y: f64) -> AxioResult<AXElement> {
  // First, find which TRACKED window is at this point.
  // This ensures we only hit-test within apps we're monitoring (excludes axio overlay).
  let window = axio.get_window_at_point(x, y).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No tracked window found at position ({x}, {y})"))
  })?;
  let window_id = window.id;
  let pid = window.process_id.0;

  // Get the app element handle from ProcessState (stored at process creation time).
  // This ensures we only query within the correct app.
  let app_handle = axio
    .get_app_handle(pid)
    .ok_or_else(|| AxioError::Internal(format!("Process {pid} not registered")))?;

  let mut element_handle = None;
  let mut fallback_container = None;

  // Try hit test with retries (for Chromium/Electron lazy initialization)
  for &delay_ms in &HIT_TEST_RETRY_DELAYS_MS {
    if delay_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(delay_ms));
    }

    let Some(hit) = app_handle.fetch_element_at_position(x, y) else {
      continue;
    };

    let attrs = hit.fetch_attributes();
    // Chromium/Electron returns a window-sized Group as a placeholder during lazy init
    let is_fallback_container = matches!(attrs.role, Role::Group | Role::GenericGroup)
      && attrs
        .bounds
        .as_ref()
        .is_some_and(|b| b.matches(&window.bounds, 0.0));

    if is_fallback_container {
      fallback_container = Some(hit);
      continue;
    }

    element_handle = Some(hit);
    break;
  }

  // Use real element if found, otherwise fall back to container (better than nothing)
  let mut element_handle = element_handle.or(fallback_container).ok_or_else(|| {
    AxioError::AccessibilityError(format!("No element found at ({x}, {y}) in app {pid}"))
  })?;

  // Try recursive hit testing - drill down through nested containers
  let raw_attrs = element_handle.fetch_attributes();
  for _ in 1..=HIT_TEST_MAX_DEPTH {
    let Some(deeper) = element_handle.fetch_element_at_position(x, y) else {
      break;
    };

    let deeper_attrs = deeper.fetch_attributes();
    let same_element = deeper_attrs.bounds == raw_attrs.bounds
      && deeper_attrs.role == raw_attrs.role
      && deeper_attrs.title == raw_attrs.title;

    if same_element {
      break;
    }

    element_handle = deeper;
  }

  build_and_register_element(axio, element_handle, window_id, pid, None).ok_or_else(|| {
    AxioError::AccessibilityError(format!("Element at ({x}, {y}) was previously destroyed"))
  })
}

/// Fetch the currently focused element and selection for an app from platform.
pub(crate) fn fetch_focus(
  axio: &Axio,
  pid: u32,
) -> (Option<AXElement>, Option<crate::types::TextSelection>) {
  use crate::platform::{CurrentPlatform, Platform};

  // Get app handle from ProcessState
  let Some(app_handle) = axio.get_app_handle(pid) else {
    return (None, None);
  };

  let Some(focused_handle) = CurrentPlatform::fetch_focused_element(&app_handle) else {
    return (None, None);
  };

  // Try to get window ID from existing element or fall back to focused window
  let window_id = {
    let hash = focused_handle.element_hash();
    axio
      .get_element_by_hash(hash)
      .map(|e| e.window_id)
      .or_else(|| axio.get_focused_window_for_pid(pid))
  };

  let Some(window_id) = window_id else {
    return (None, None);
  };

  let Some(element) =
    build_and_register_element(axio, focused_handle.clone(), window_id, pid, None)
  else {
    return (None, None);
  };

  let selection =
    focused_handle
      .fetch_selection()
      .map(|(text, range)| crate::types::TextSelection {
        element_id: element.id,
        text,
        range,
      });

  (Some(element), selection)
}
