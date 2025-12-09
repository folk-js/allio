/*!
Watch/unwatch subscription management.
*/

use super::Axio;
use crate::accessibility::Notification;
use crate::platform::PlatformObserver;
use crate::types::{AxioError, AxioResult, ElementId, ProcessId};

impl Axio {
  /// Watch an element for notifications.
  pub(crate) fn watch_element(&self, element_id: ElementId) -> AxioResult<()> {
    let mut state = self.state.write();
    Self::watch_internal(&mut state, &element_id, self.clone())
  }

  /// Stop watching an element.
  pub(crate) fn unwatch_element(&self, element_id: ElementId) -> AxioResult<()> {
    let mut state = self.state.write();
    Self::unwatch_internal(&mut state, &element_id)
  }

  /// Subscribe to watch notifications for an element.
  pub(super) fn watch_internal(
    state: &mut super::State,
    element_id: &ElementId,
    axio: Axio,
  ) -> AxioResult<()> {
    let elem_state = state
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;

    if elem_state.element_watch.is_some() {
      return Ok(()); // Already watching
    }

    let process_id = ProcessId(elem_state.pid());
    let observer = state
      .processes
      .get(&process_id)
      .map(|p| &p.observer)
      .ok_or(AxioError::NotSupported("Process not found".into()))?;

    let notifs = Notification::for_watching(elem_state.element.role);
    if notifs.is_empty() {
      return Ok(()); // Nothing to watch
    }

    let watch_handle =
      observer.watch_element(&elem_state.handle, *element_id, &notifs, axio)?;

    elem_state.element_watch = Some(watch_handle);
    for n in notifs {
      elem_state.subscriptions.insert(n);
    }

    Ok(())
  }

  /// Unsubscribe from watch notifications.
  /// With WatchHandle, this just drops the handle (RAII cleanup).
  pub(super) fn unwatch_internal(
    state: &mut super::State,
    element_id: &ElementId,
  ) -> AxioResult<()> {
    let elem_state = state
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;

    // Drop the watch handle - this automatically unsubscribes
    elem_state.element_watch.take();

    elem_state
      .subscriptions
      .retain(|n| *n == Notification::Destroyed);

    Ok(())
  }
}
