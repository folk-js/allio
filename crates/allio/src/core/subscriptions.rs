/*!
Watch/unwatch subscription methods for Allio.

Uses take/replace pattern to avoid holding lock during OS calls.
*/

use super::Allio;
use crate::a11y::Notification;
use crate::types::{AllioError, AllioResult, ElementId};

impl Allio {
  /// Watch an element for change notifications (value, title, children, etc).
  ///
  /// Uses OS notifications where the element supports them. Some elements (e.g.
  /// `AppKit` segmented-control segments) reject every registration; those fall back
  /// to polling, so clients see `element:changed` either way.
  pub fn watch(&self, element_id: ElementId) -> AllioResult<()> {
    self.add_change_watch(element_id)?;
    let has_value_notifs = self.write(|s| {
      s.set_explicitly_watched(element_id, true);
      s.element(element_id)
        .and_then(|e| e.watch.as_ref())
        .is_some_and(|w| w.has(Notification::ValueChanged))
    });
    if !has_value_notifs {
      self.start_node_poll(element_id);
    }
    Ok(())
  }

  /// Stop watching an element for change notifications.
  pub fn unwatch(&self, element_id: ElementId) -> AllioResult<()> {
    self.write(|s| s.set_explicitly_watched(element_id, false));
    self.stop_node_poll(element_id);
    self.remove_change_watch(element_id)
  }

  /// Subscribe to the role's change notifications. Does not mark the watch as explicit.
  pub(crate) fn add_change_watch(&self, element_id: ElementId) -> AllioResult<()> {
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
      // Expected for elements that reject all notifications; `watch()` polls those.
      log::debug!("Element {element_id} has no watch handle");
      return Ok(());
    };

    let added = watch.add(&notifs);
    if added < notifs.len() {
      log::warn!(
        "Element {element_id}: only {added}/{} notifications registered",
        notifs.len()
      );
    }

    // Step 3: Put watch back (quick write)
    self.write(|s| s.set_element_watch(element_id, watch));

    Ok(())
  }

  /// Unsubscribe the role's change notifications (destruction tracking is kept).
  pub(crate) fn remove_change_watch(&self, element_id: ElementId) -> AllioResult<()> {
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
