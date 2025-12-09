/*!
Watch/unwatch subscription management.
*/

use super::Axio;
use crate::accessibility::Notification;
use crate::platform::{self, ObserverContextHandle};
use crate::types::{AxioError, AxioResult, ElementId, ProcessId};
use std::ffi::c_void;

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

    if elem_state.watch_context.is_some() {
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

    let context = platform::subscribe_notifications(
      &elem_state.element.id,
      &elem_state.handle,
      observer,
      &elem_state.raw_role,
      &notifs,
      axio,
    )?;

    elem_state.watch_context = Some(context.cast::<c_void>());
    for n in notifs {
      elem_state.subscriptions.insert(n);
    }

    Ok(())
  }

  /// Unsubscribe from watch notifications.
  pub(super) fn unwatch_internal(
    state: &mut super::State,
    element_id: &ElementId,
  ) -> AxioResult<()> {
    let elem_state = state
      .elements
      .get_mut(element_id)
      .ok_or(AxioError::ElementNotFound(*element_id))?;

    let Some(context) = elem_state.watch_context.take() else {
      return Ok(());
    };

    let process_id = ProcessId(elem_state.pid());
    let process = state
      .processes
      .get(&process_id)
      .ok_or_else(|| AxioError::Internal("Process not found during unwatch".into()))?;

    let notifs: Vec<_> = elem_state
      .subscriptions
      .iter()
      .filter(|n| **n != Notification::Destroyed)
      .copied()
      .collect();

    platform::unsubscribe_notifications(
      &elem_state.handle,
      &process.observer,
      context.cast::<ObserverContextHandle>(),
      &notifs,
    );

    elem_state
      .subscriptions
      .retain(|n| *n == Notification::Destroyed);

    Ok(())
  }
}
