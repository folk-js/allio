/*!
Adapter functions that convert between internal and public types.

These transform raw Registry data into public API types,
and platform handles into cached registry entries.
*/

use super::registry::{CachedElement, Registry};
use crate::accessibility::Role;
use crate::platform::{Handle, PlatformHandle};
use crate::types::{Element, ElementId, ProcessId, Snapshot, WindowId};

/// Build an Element from a `CachedElement` + tree relationships.
pub(crate) fn build_element(registry: &Registry, id: ElementId) -> Option<Element> {
  let elem = registry.element(id)?;
  let parent_id = if elem.is_root {
    None
  } else {
    registry.tree_parent(id)
  };
  let children_slice = registry.tree_children(id);
  let children = if children_slice.is_empty() && !registry.tree_has_children(id) {
    None // Children not yet fetched
  } else {
    Some(children_slice.to_vec())
  };

  Some(Element {
    id,
    window_id: elem.window_id,
    pid: elem.pid,
    is_root: elem.is_root,
    parent_id,
    children,
    role: elem.role,
    platform_role: elem.platform_role.clone(),
    label: elem.label.clone(),
    description: elem.description.clone(),
    placeholder: elem.placeholder.clone(),
    url: elem.url.clone(),
    value: elem.value.clone(),
    bounds: elem.bounds,
    focused: elem.focused,
    disabled: elem.disabled,
    selected: elem.selected,
    expanded: elem.expanded,
    row_index: elem.row_index,
    column_index: elem.column_index,
    row_count: elem.row_count,
    column_count: elem.column_count,
    actions: elem.actions.clone(),
    is_fallback: elem.is_fallback,
  })
}

/// Build all elements as public API types.
pub(crate) fn build_all_elements(registry: &Registry) -> Vec<Element> {
  registry
    .elements()
    .filter_map(|(id, _)| build_element(registry, id))
    .collect()
}

/// Build a `CachedElement` from a platform handle.
pub(crate) fn build_entry_from_handle(
  handle: Handle,
  window_id: WindowId,
  pid: ProcessId,
) -> CachedElement {
  let attrs = handle.fetch_attributes();

  // Fetch parent once and reuse (OS call is expensive)
  let parent_handle = handle.fetch_parent();

  // Determine if this is a root element (parent is Application)
  let is_root = parent_handle
    .as_ref()
    .is_some_and(|p| p.fetch_attributes().role == Role::Application);

  // For root elements, don't store parent handle
  let parent_for_entry = if is_root { None } else { parent_handle };

  CachedElement::from_attributes(
    ElementId::new(),
    window_id,
    pid,
    is_root,
    handle,
    parent_for_entry,
    attrs,
  )
}

/// Build a snapshot of current state.
pub(crate) fn build_snapshot(registry: &Registry) -> Snapshot {
  let (focused_element, selection) = registry
    .focused_window()
    .and_then(|wid| {
      let window = registry.window(wid)?;
      let process = registry.process(window.process_id)?;
      let focused = process
        .focused_element
        .and_then(|eid| build_element(registry, eid));
      Some((focused, process.last_selection.clone()))
    })
    .unwrap_or((None, None));

  Snapshot {
    windows: registry.windows().map(|w| w.info.clone()).collect(),
    elements: build_all_elements(registry),
    focused_window: registry.focused_window(),
    focused_element,
    selection,
    z_order: registry.z_order().to_vec(),
    mouse_position: registry.mouse_position(),
  }
}
