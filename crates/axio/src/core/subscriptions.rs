/*!
Watch/unwatch subscription management.

Elements start with a watch containing only the Destroyed notification.
watch_element adds more notifications, unwatch_element removes them.
*/

use super::Axio;
use crate::accessibility::Notification;
use crate::types::{AxioError, AxioResult, ElementId};

impl Axio {
  /// Watch an element for change notifications (value, title, children, etc).
  /// Adds notifications to the existing watch handle.
  pub(crate) fn watch_element(&self, element_id: ElementId) -> AxioResult<()> {
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
    }
    // If no watch exists (shouldn't happen - created at registration), silently skip

    Ok(())
  }

  /// Stop watching an element for change notifications.
  /// Removes watch notifications but keeps Destroyed.
  pub(crate) fn unwatch_element(&self, element_id: ElementId) -> AxioResult<()> {
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
