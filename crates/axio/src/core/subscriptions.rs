/*!
Watch/unwatch subscription methods for Axio.
*/

use super::Axio;
use crate::accessibility::Notification;
use crate::types::{AxioError, AxioResult, ElementId};

impl Axio {
  /// Watch an element for change notifications (value, title, children, etc).
  pub fn watch(&self, element_id: ElementId) -> AxioResult<()> {
    let mut state = self.state.write();

    let elem_state = state
      .elements
      .get_mut(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;

    let notifs = Notification::for_watching(elem_state.element.role);
    if notifs.is_empty() {
      return Ok(()); // Nothing to watch for this role
    }

    // Add notifications to existing watch
    if let Some(watch) = &mut elem_state.watch {
      watch.add(&notifs);
    } else {
      log::warn!("Element {element_id} has no watch handle");
    }

    Ok(())
  }

  /// Stop watching an element for change notifications.
  pub fn unwatch(&self, element_id: ElementId) -> AxioResult<()> {
    let mut state = self.state.write();

    let elem_state = state
      .elements
      .get_mut(&element_id)
      .ok_or(AxioError::ElementNotFound(element_id))?;

    let notifs = Notification::for_watching(elem_state.element.role);

    // Remove notifications from watch (keeps Destroyed)
    if let Some(watch) = &mut elem_state.watch {
      watch.remove(&notifs);
    }

    Ok(())
  }
}

