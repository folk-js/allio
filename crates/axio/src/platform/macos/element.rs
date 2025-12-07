//! Element building and operations for macOS accessibility.
//!
//! This module handles:
//! - Building AXElement from platform handles
//! - Child discovery
//! - Element refresh
//! - Element hash for deduplication
//! - Element operations (write, click)

use objc2_core_foundation::CFHash;

use crate::events::emit;
use crate::platform::handles::ElementHandle;
use crate::types::{AXElement, AxioError, AxioResult, ElementId, Event, WindowId};

use super::mapping::{ax_action, role_from_macos};

// =============================================================================
// Element Building
// =============================================================================

/// Build an AXElement from an ElementHandle and register it.
/// Uses batch attribute fetching for ~10x faster element creation.
/// All unsafe code is encapsulated in ElementHandle methods.
/// Returns None if the element's hash is in the dead set (was previously destroyed).
pub fn build_element_from_handle(
  handle: ElementHandle,
  window_id: &WindowId,
  pid: u32,
  parent_id: Option<&ElementId>,
) -> Option<AXElement> {
  // Fetch all attributes in ONE IPC call - safe method!
  let attrs = handle.get_attributes(None);

  let platform_role = attrs.role.clone().unwrap_or_else(|| "Unknown".to_string());
  let role = role_from_macos(&platform_role);

  let subrole = if matches!(role, crate::accessibility::Role::Unknown) {
    Some(platform_role.clone())
  } else {
    attrs.subrole
  };

  let element = AXElement {
    id: ElementId::new(),
    window_id: *window_id,
    parent_id: parent_id.copied(),
    children: None,
    role,
    subrole,
    label: attrs.title,
    value: attrs.value,
    description: attrs.description,
    placeholder: attrs.placeholder,
    bounds: attrs.bounds,
    focused: attrs.focused,
    enabled: attrs.enabled,
    actions: attrs.actions,
  };

  crate::registry::register_element(element, handle, pid, &platform_role)
}

// =============================================================================
// Child Discovery
// =============================================================================

/// Discover and register children of an element.
pub fn discover_children(parent_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  let info = crate::registry::get_stored_element_info(parent_id)?;

  // Use safe ElementHandle method
  let child_handles = info.handle.get_children();
  if child_handles.is_empty() {
    crate::registry::set_element_children(parent_id, vec![])?;
    return Ok(vec![]);
  }

  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for child_handle in child_handles.into_iter().take(max_children) {
    // Skip children that were previously destroyed
    if let Some(child) =
      build_element_from_handle(child_handle, &info.window_id, info.pid, Some(parent_id))
    {
      child_ids.push(child.id);
      children.push(child);
    }
  }

  crate::registry::set_element_children(parent_id, child_ids)?;

  for child in &children {
    emit(Event::ElementAdded {
      element: child.clone(),
    });
  }

  if let Ok(updated_parent) = crate::registry::get_element(parent_id) {
    emit(Event::ElementChanged {
      element: updated_parent.clone(),
    });
  }

  Ok(children)
}

// =============================================================================
// Element Refresh
// =============================================================================

/// Refresh an element's attributes from the platform.
pub fn refresh_element(element_id: &ElementId) -> AxioResult<AXElement> {
  let info = crate::registry::get_stored_element_info(element_id)?;

  // Use safe ElementHandle method for batch attribute fetch
  let attrs = info.handle.get_attributes(Some(&info.platform_role));

  let role = role_from_macos(&info.platform_role);
  let subrole = if matches!(role, crate::accessibility::Role::Unknown) {
    Some(info.platform_role.to_string())
  } else {
    attrs.subrole
  };

  let updated = AXElement {
    id: *element_id,
    window_id: info.window_id,
    parent_id: info.parent_id,
    children: info.children,
    role,
    subrole,
    label: attrs.title,
    value: attrs.value,
    description: attrs.description,
    placeholder: attrs.placeholder,
    bounds: attrs.bounds,
    focused: attrs.focused,
    enabled: attrs.enabled,
    actions: attrs.actions,
  };

  crate::registry::update_element(element_id, updated.clone())?;
  Ok(updated)
}

// =============================================================================
// Element Hash
// =============================================================================

/// Get hash for element handle (for O(1) dedup lookup).
pub fn element_hash(handle: &ElementHandle) -> u64 {
  CFHash(Some(handle.inner())) as u64
}

// =============================================================================
// Element Operations
// =============================================================================

/// Write a text value to an element.
pub fn write_element_value(
  handle: &ElementHandle,
  text: &str,
  platform_role: &str,
) -> AxioResult<()> {
  // Use Role::is_writable() for writability check
  let role = role_from_macos(platform_role);
  if !role.is_writable() {
    return Err(AxioError::NotSupported(format!(
      "Element with role '{platform_role}' is not writable"
    )));
  }

  handle
    .set_value(text)
    .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {e:?}")))
}

/// Perform a click (press) action on an element.
pub fn click_element(handle: &ElementHandle) -> AxioResult<()> {
  handle
    .perform_action(ax_action::PRESS)
    .map_err(|e| AxioError::AccessibilityError(format!("AXPress failed: {e:?}")))
}
