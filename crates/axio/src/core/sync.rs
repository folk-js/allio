/*!
Bulk synchronization operations called by the polling loop.

These methods update the registry with fresh data from the OS without going
through individual element queries.
*/

use super::Axio;
use crate::platform::{CurrentPlatform, Platform};
use crate::types::{Window, WindowId};
use std::collections::HashSet;

impl Axio {
  /// Sync windows from polling. Handles add/update/remove.
  /// `skip_removal=true` during space transitions where window visibility is unreliable.
  /// TODO: remove `skip_removal` and just pause sync in this instance ^
  pub(crate) fn sync_windows(&self, new_windows: Vec<Window>, skip_removal: bool) {
    let new_ids: HashSet<WindowId> = new_windows.iter().map(|w| w.id).collect();

    let windows_needing_handle: HashSet<WindowId> = self.read(|s| {
      new_ids
        .iter()
        .filter(|id| {
          // Fetch handle if: window is new OR existing window has no handle
          s.window(**id).and_then(|w| w.handle.as_ref()).is_none()
        })
        .copied()
        .collect()
    });

    let windows_with_handles: Vec<_> = new_windows
      .into_iter()
      .map(|w| {
        let handle = if windows_needing_handle.contains(&w.id) {
          CurrentPlatform::fetch_window_handle(&w)
        } else {
          None // Already have a cached handle
        };
        (w, handle)
      })
      .collect();

    let new_process_pids = self.write(|s| {
      // Remove windows no longer present
      if !skip_removal {
        let to_remove: Vec<WindowId> = s.window_ids().filter(|id| !new_ids.contains(id)).collect();
        for window_id in to_remove {
          s.remove_window(window_id);
        }
      }

      // Add/update windows
      let mut fresh_pids = Vec::new();
      for (window_info, handle) in windows_with_handles {
        let window_id = window_info.id;
        let process_id = window_info.process_id;
        let is_new = s.window(window_id).is_none();

        if is_new {
          s.upsert_window(window_id, process_id, window_info.clone(), handle.clone());
          fresh_pids.push(process_id);
        } else {
          // Already existed - update
          s.update_window(window_id, window_info);
          // Update handle if we fetched one (retrying for windows that had None)
          if let Some(h) = handle {
            s.set_window_handle(window_id, h);
          }
        }
      }
      fresh_pids
    });

    for process_id in new_process_pids {
      if let Err(e) = self.ensure_process(process_id.0) {
        log::warn!("Failed to create process for window: {e:?}");
      }
    }
  }

  /// Sync focused window from polling.
  pub(crate) fn sync_focused_window(&self, window_id: Option<WindowId>) {
    self.write(|s| s.set_focused_window(window_id));
  }

  /// Sync mouse position from polling.
  pub(crate) fn sync_mouse(&self, pos: crate::types::Point) {
    self.write(|s| s.set_mouse_position(pos));
  }
}
