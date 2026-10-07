/*!
Write operations that modify OS state through the platform layer.

These are user-initiated actions that send commands to the accessibility API.

Writability is decided by the app, not by role tables: every write first asks the
app whether the attribute is settable. Many apps report success for writes to
non-settable attributes while ignoring them, so the return code alone can't be trusted.
*/

use super::Allio;
use crate::a11y::{Action, SettableAttribute};
use crate::platform::PlatformHandle;
use crate::types::{AllioError, AllioResult, ElementId, TextRange, WindowId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Options for [`Allio::set_value_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetOptions {
  /// Perform the element's confirm action after writing, if it has one.
  ///
  /// Some fields only commit to the app's model when editing ends (e.g. Reminders
  /// titles revert on relaunch without it). Opt-in because on other elements
  /// "confirm" means "press Enter" (e.g. submitting a search field).
  #[serde(default)]
  pub commit: bool,
}

impl Allio {
  /// Set a typed value on an element.
  pub fn set_value(&self, element_id: ElementId, value: &crate::a11y::Value) -> AllioResult<()> {
    self.set_value_with(element_id, value, SetOptions::default())
  }

  /// Set a typed value on an element, with options.
  pub fn set_value_with(
    &self,
    element_id: ElementId,
    value: &crate::a11y::Value,
    options: SetOptions,
  ) -> AllioResult<()> {
    let (handle, current, can_confirm) = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AllioError::ElementNotFound(element_id))?;
      Ok((
        e.handle.clone(),
        e.value.clone(),
        e.actions.contains(&Action::Confirm),
      ))
    })?;

    // The app decides writability (platform call, NO LOCK).
    if !handle.is_settable(SettableAttribute::Value) {
      return Err(AllioError::NotSettable {
        element: element_id,
        attribute: SettableAttribute::Value,
      });
    }

    // Keep the element's value type: writing a string into a checkbox is a caller bug.
    if let Some(current) = &current {
      let (expected, got) = (current.value_type(), value.value_type());
      if expected != got {
        return Err(AllioError::TypeMismatch { expected, got });
      }
    }

    handle.set_value(value)?;
    if options.commit && can_confirm {
      handle.perform_action(Action::Confirm)?;
    }

    // Reflect the write immediately (emits ElementChanged if it took effect).
    self.refresh_element(element_id).map(drop)
  }

  /// Replace `range` of an element's text with `text`, without focus or selection.
  ///
  /// Preserves the rest of the text (and its styling), unlike `set_value`, which
  /// replaces the whole value. `Err(NotSupported)` where the app doesn't implement it.
  pub fn replace_text(&self, element_id: ElementId, range: TextRange, text: &str) -> AllioResult<()> {
    if range.end < range.start {
      return Err(AllioError::NotSupported(format!("Invalid range {}..{}", range.start, range.end)));
    }
    let handle = self.element_handle(element_id)?.0;
    handle.replace_text(range.start, range.len(), text)?;
    self.refresh_element(element_id).map(drop)
  }

  /// Move a window so its top-left is at (x, y), in global screen points (an AX write of its
  /// position: no focus, no raise).
  pub fn move_window(&self, window_id: WindowId, x: f64, y: f64) -> AllioResult<()> {
    let (_, handle) = self
      .window_with_handle(window_id)
      .ok_or(AllioError::WindowNotFound(window_id))?;
    let handle = handle.ok_or_else(|| AllioError::NotSupported(format!("window {window_id} has no accessibility handle")))?;
    handle.set_position(x, y)
  }

  /// Perform an action on an element.
  pub fn perform_action(&self, element_id: ElementId, action: Action) -> AllioResult<()> {
    let handle = self.element_handle(element_id)?.0;
    handle.perform_action(action)
  }

  /// Perform an app-declared custom action by label (see `Element::custom_actions`).
  pub fn perform_custom_action(&self, element_id: ElementId, label: &str) -> AllioResult<()> {
    let handle = self.element_handle(element_id)?.0;
    handle.perform_custom_action(label)
  }
}
