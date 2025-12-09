/*!
Accessibility notifications.

Notifications are events that the system fires when UI elements change.
Platform-specific notification strings are mapped in `platform/macos/mapping.rs`.
*/

use super::Role;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Notifications we can subscribe to for an element.
///
/// Platform mappings (macOS kAX*Notification, Windows UIA events) are handled
/// by the platform layer. See `platform::notification_to_platform_string`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum Notification {
  /// Element was destroyed and is no longer valid.
  /// This is ALWAYS subscribed for all registered elements.
  Destroyed,

  /// Element's value changed (text input, slider position, checkbox state, etc.)
  ValueChanged,

  /// Element's title/label changed
  TitleChanged,

  /// Focus moved to this element
  FocusChanged,

  /// Selection within this element changed (text selection, list selection)
  SelectionChanged,

  /// Element's position or size changed
  BoundsChanged,

  /// Element's children changed (added/removed)
  ChildrenChanged,
}

impl Notification {
  /// Notifications that are ALWAYS subscribed for any registered element.
  ///
  /// Currently just Destroyed - we always want to know when elements die
  /// so we can clean up our registry.
  pub const ALWAYS: &'static [Self] = &[Self::Destroyed];

  /// Additional notifications to subscribe when "watching" an element.
  ///
  /// These are role-dependent: text fields get `ValueChanged` + `SelectionChanged`,
  /// windows get `TitleChanged`, etc. This does NOT include Destroyed (that's implicit).
  ///
  /// # Example
  /// ```
  /// use axio::accessibility::{Notification, Role};
  ///
  /// let notifs = Notification::for_watching(Role::TextField);
  /// assert!(notifs.contains(&Notification::ValueChanged));
  /// ```
  pub fn for_watching(role: Role) -> Vec<Self> {
    let mut notifs = vec![];

    // Track value changes for writable elements
    if role.is_writable() {
      notifs.push(Self::ValueChanged);
    }

    // Track title changes for windows
    if matches!(role, Role::Window) {
      notifs.push(Self::TitleChanged);
    }

    // Track selection for text inputs
    if role.is_text_input() {
      notifs.push(Self::SelectionChanged);
    }

    notifs
  }

  /// Whether this notification is subscribed at app/process level.
  ///
  /// App-level notifications are subscribed on the application element itself,
  /// not on individual UI elements. The callback receives the newly-focused
  /// or selection-changed element directly.
  ///
  /// Element-level notifications (the default) are subscribed per-element
  /// and the callback context identifies which element changed.
  pub const fn is_app_level(&self) -> bool {
    matches!(self, Self::FocusChanged | Self::SelectionChanged)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn destroyed_is_always_subscribed() {
    assert!(Notification::ALWAYS.contains(&Notification::Destroyed));
    assert_eq!(Notification::ALWAYS.len(), 1);
  }

  #[test]
  fn text_fields_get_value_and_selection() {
    let notifs = Notification::for_watching(Role::TextField);
    assert!(notifs.contains(&Notification::ValueChanged));
    assert!(notifs.contains(&Notification::SelectionChanged));
  }

  #[test]
  fn windows_get_title_changes() {
    let notifs = Notification::for_watching(Role::Window);
    assert!(notifs.contains(&Notification::TitleChanged));
  }

  #[test]
  fn buttons_get_nothing_special() {
    let notifs = Notification::for_watching(Role::Button);
    assert!(notifs.is_empty());
  }
}
