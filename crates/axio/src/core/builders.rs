/*!
Builder functions for derived types.

These transform raw Registry entries into public API types,
and platform handles into Registry entries.
*/

use super::registry::{ElementData, ElementEntry, Registry};
use crate::accessibility::Role;
use crate::platform::{Handle, PlatformHandle};
use crate::types::{Element, ElementId, ProcessId, Snapshot, WindowId};

/// Build an Element from an ElementEntry + tree relationships.
pub(crate) fn build_element(registry: &Registry, id: ElementId) -> Option<Element> {
  let elem = registry.element(id)?;
  let data = &elem.data;
  let parent_id = if data.is_root {
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
    window_id: data.window_id,
    pid: data.pid,
    is_root: data.is_root,
    parent_id,
    children,
    role: data.role,
    platform_role: data.platform_role.clone(),
    label: data.label.clone(),
    description: data.description.clone(),
    placeholder: data.placeholder.clone(),
    url: data.url.clone(),
    value: data.value.clone(),
    bounds: data.bounds,
    focused: data.focused,
    disabled: data.disabled,
    selected: data.selected,
    expanded: data.expanded,
    row_index: data.row_index,
    column_index: data.column_index,
    row_count: data.row_count,
    column_count: data.column_count,
    actions: data.actions.clone(),
    is_fallback: data.is_fallback,
  })
}

/// Build all elements as public API types.
pub(crate) fn build_all_elements(registry: &Registry) -> Vec<Element> {
  registry
    .elements()
    .filter_map(|(id, _)| build_element(registry, id))
    .collect()
}

/// Build an ElementEntry from a platform handle.
pub(crate) fn build_entry_from_handle(
  handle: Handle,
  window_id: WindowId,
  pid: ProcessId,
) -> ElementEntry {
  let attrs = handle.fetch_attributes();

  // Fetch parent once and reuse (OS call is expensive)
  let parent_handle = handle.fetch_parent();

  // Determine if this is a root element (parent is Application)
  let is_root = parent_handle
    .as_ref()
    .map(|p| p.fetch_attributes().role == Role::Application)
    .unwrap_or(false);

  // For root elements, don't store parent handle
  let parent_for_entry = if is_root { None } else { parent_handle };

  let data = ElementData::from_attributes(ElementId::new(), window_id, pid, is_root, attrs);

  ElementEntry::new(data, handle, parent_for_entry)
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
