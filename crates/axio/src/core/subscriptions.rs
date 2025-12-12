/*!
Watch/unwatch subscription methods for Axio.

Uses take/replace pattern to avoid holding lock during OS calls.
*/

use super::Allio;
use crate::a11y::Notification;
use crate::types::{AllioError, AllioResult, ElementId};

impl Allio {
  /// Watch an element for change notifications (value, title, children, etc).
  pub fn watch(&self, element_id: ElementId) -> AllioResult<()> {
    // Step 1: Get role and take watch handle (quick write, releases lock)
    let (notifs, watch_handle) = self.write(|s| {
      let role = s
        .element(element_id)
        .map(|e| e.role)
        .ok_or(AllioError::ElementNotFound(element_id))?;

      let notifs = Notification::for_watching(role);
      if notifs.is_empty() {
        return Ok((notifs, None));
      }

      let watch = s.take_element_watch(element_id);
      Ok((notifs, watch))
    })?;

    // Step 2: OS operations (NO LOCK)
    let Some(mut watch) = watch_handle else {
      if !notifs.is_empty() {
        log::warn!("Element {element_id} has no watch handle");
      }
      return Ok(());
    };

    watch.add(&notifs);

    // Step 3: Put watch back (quick write)
    self.write(|s| s.set_element_watch(element_id, watch));

    Ok(())
  }

  /// Stop watching an element for change notifications.
  pub fn unwatch(&self, element_id: ElementId) -> AllioResult<()> {
    // Step 1: Get role and take watch handle (quick write, releases lock)
    let (notifs, watch_handle) = self.write(|s| {
      let role = s
        .element(element_id)
        .map(|e| e.role)
        .ok_or(AllioError::ElementNotFound(element_id))?;

      let notifs = Notification::for_watching(role);
      let watch = s.take_element_watch(element_id);
      Ok((notifs, watch))
    })?;

    // Step 2: OS operations (NO LOCK)
    let Some(mut watch) = watch_handle else {
      return Ok(());
    };

    watch.remove(&notifs);

    // Step 3: Put watch back (quick write)
    self.write(|s| s.set_element_watch(element_id, watch));

    Ok(())
  }
}
