/*!
Element building and operations for macOS accessibility.

Handles:
- Building `AXElement` from platform handles
- Child/parent discovery
- Element refresh
- Hash-based deduplication
- Element operations (write, click)
*/

use objc2_core_foundation::CFHash;

use crate::platform::handles::ElementHandle;
use crate::types::{AXElement, AxioError, AxioResult, ElementId, ProcessId, WindowId};

use super::mapping::{ax_action, role_from_macos};
use crate::accessibility::Role;

/// Refine role based on element attributes.
///
/// Plain `AXGroup` with no label/value is likely just a layout container
/// with no semantic meaning—classify as `GenericGroup`.
/// Semantic group types (`AXSplitGroup`, `AXRadioGroup`) stay as Group.
fn refine_role(
  role: Role,
  raw_role: &str,
  label: Option<&String>,
  value: Option<&crate::accessibility::Value>,
) -> Role {
  // Only demote plain AXGroup, not semantic group types
  if role == Role::Group && raw_role == "AXGroup" && label.is_none() && value.is_none() {
    Role::GenericGroup
  } else {
    role
  }
}

/// Build an `AXElement` from an `ElementHandle` and register it.
/// Uses batch attribute fetching for ~10x faster element creation.
/// All unsafe code is encapsulated in `ElementHandle` methods.
/// Returns None if the element's hash is in the dead set (was previously destroyed).
pub(super) fn build_element_from_handle(
  handle: ElementHandle,
  window_id: WindowId,
  pid: u32,
  parent_id: Option<ElementId>,
) -> Option<AXElement> {
  // Fetch all attributes in ONE IPC call - safe method!
  let attrs = handle.get_attributes(None);

  let raw_role = attrs.role.clone().unwrap_or_else(|| "Unknown".to_string());
  let base_role = role_from_macos(&raw_role);
  let role = refine_role(
    base_role,
    &raw_role,
    attrs.title.as_ref(),
    attrs.value.as_ref(),
  );

  // Combine role + subrole for debugging display (e.g., "AXButton/AXCloseButton")
  let platform_role = match &attrs.subrole {
    Some(sr) => format!("{raw_role}/{sr}"),
    None => raw_role.clone(),
  };

  // Determine if this is a root element.
  // In macOS, even windows have AXParent (the application element).
  // We consider an element a "root" if its parent is the application.
  let is_root = handle
    .get_element("AXParent")
    .and_then(|p| p.get_string("AXRole"))
    .as_deref()
    == Some("AXApplication");

  // Parent ID is set if caller passed it (child discovery), otherwise None (orphan or root)
  let parent_id_value = if is_root { None } else { parent_id };

  let element = AXElement {
    id: ElementId::new(),
    window_id,
    pid: ProcessId(pid),
    is_root,
    parent_id: parent_id_value,
    children: None,
    role,
    platform_role: platform_role.clone(),
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

  crate::registry::register_element(element, handle, pid, &raw_role)
}

/// Fetch and register children of an element.
pub(crate) fn children(parent_id: ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
  let info = crate::registry::get_stored_element_info(parent_id)?;

  let child_handles = info.handle.get_children();
  if child_handles.is_empty() {
    crate::registry::set_element_children(parent_id, vec![])?;
    return Ok(vec![]);
  }

  let mut children = Vec::new();
  let mut child_ids = Vec::new();

  for child_handle in child_handles.into_iter().take(max_children) {
    if let Some(child) =
      build_element_from_handle(child_handle, info.window_id, info.pid, Some(parent_id))
    {
      child_ids.push(child.id);
      children.push(child);
    }
  }

  crate::registry::set_element_children(parent_id, child_ids)?;

  Ok(children)
}

/// Fetch and register parent of an element.
/// The lazy linking in `register_element` will connect this element to the parent.
pub(crate) fn parent(element_id: ElementId) -> AxioResult<Option<AXElement>> {
  let info = crate::registry::get_stored_element_info(element_id)?;

  let Some(parent_handle) = info.handle.get_element("AXParent") else {
    return Ok(None);
  };

  let parent = build_element_from_handle(parent_handle, info.window_id, info.pid, None);
  Ok(parent)
}

/// Refresh an element's attributes from the platform.
pub(crate) fn refresh_element(element_id: ElementId) -> AxioResult<AXElement> {
  let info = crate::registry::get_stored_element_info(element_id)?;

  let attrs = info.handle.get_attributes(Some(&info.platform_role));

  let base_role = role_from_macos(&info.platform_role);
  let role = refine_role(
    base_role,
    &info.platform_role,
    attrs.title.as_ref(),
    attrs.value.as_ref(),
  );

  // Combine role + subrole for debugging display
  let platform_role = match &attrs.subrole {
    Some(sr) => format!("{}/{sr}", info.platform_role),
    None => info.platform_role.to_string(),
  };

  let updated = AXElement {
    id: element_id,
    window_id: info.window_id,
    pid: ProcessId(info.pid),
    is_root: info.is_root,
    parent_id: info.parent_id,
    children: info.children,
    role,
    platform_role,
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

  crate::registry::update_element(element_id, updated.clone())?;
  Ok(updated)
}

/// Get hash for element handle (for O(1) dedup lookup).
pub(crate) fn element_hash(handle: &ElementHandle) -> u64 {
  CFHash(Some(handle.inner())) as u64
}

/// Write a typed value to an element.
pub(crate) fn write_element_value(
  handle: &ElementHandle,
  value: &crate::accessibility::Value,
  platform_role: &str,
) -> AxioResult<()> {
  let role = role_from_macos(platform_role);
  if !role.is_writable() {
    return Err(AxioError::NotSupported(format!(
      "Element with role '{platform_role}' is not writable"
    )));
  }

  handle
    .set_typed_value(value)
    .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {e:?}")))
}

/// Perform a click (press) action on an element.
pub(crate) fn click_element(handle: &ElementHandle) -> AxioResult<()> {
  handle
    .perform_action(ax_action::PRESS)
    .map_err(|e| AxioError::AccessibilityError(format!("AXPress failed: {e:?}")))
}
