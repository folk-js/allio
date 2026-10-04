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
#[ts(export)]
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
  /// Value and title changes are subscribed for every role: whether an element
  /// posts them is the app's decision, not something our role table can predict
  /// (e.g. pop-up buttons and segmented toggles carry values but aren't "writable").
  /// Registrations the app doesn't support are simply skipped by the platform layer.
  /// Text inputs additionally get `SelectionChanged`.
  ///
  /// # Example
  /// ```
  /// use allio::a11y::{Notification, Role};
  ///
  /// let notifs = Notification::for_watching(Role::TextField);
  /// assert!(notifs.contains(&Notification::ValueChanged));
  /// ```
  pub fn for_watching(role: Role) -> Vec<Self> {
    let mut notifs = vec![Self::ValueChanged, Self::TitleChanged];

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
  fn non_writable_roles_still_get_value_and_title() {
    for role in [Role::Button, Role::Unknown, Role::StaticText] {
      let notifs = Notification::for_watching(role);
      assert!(notifs.contains(&Notification::ValueChanged));
      assert!(notifs.contains(&Notification::TitleChanged));
      assert!(!notifs.contains(&Notification::SelectionChanged));
    }
  }
}
