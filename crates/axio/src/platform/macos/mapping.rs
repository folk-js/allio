/*!
Bidirectional mappings between axio accessibility types and macOS AX* strings.

Provides zero-cost conversions between our cross-platform types
and the macOS Accessibility API string constants.
*/

#![allow(dead_code)]

use crate::accessibility::{Action, Notification, Role};

/// macOS notification string constants (kAX*Notification).
pub mod ax_notification {
  pub const DESTROYED: &str = "AXUIElementDestroyed";
  pub const VALUE_CHANGED: &str = "AXValueChanged";
  pub const TITLE_CHANGED: &str = "AXTitleChanged";
  pub const FOCUS_CHANGED: &str = "AXFocusedUIElementChanged";
  pub const SELECTION_CHANGED: &str = "AXSelectedTextChanged";
  pub const BOUNDS_CHANGED: &str = "AXMoved"; // Also AXResized
  pub const CHILDREN_CHANGED: &str = "AXLayoutChanged";
}

/// Convert our Notification to macOS notification string.
pub fn notification_to_macos(n: Notification) -> &'static str {
  match n {
    Notification::Destroyed => ax_notification::DESTROYED,
    Notification::ValueChanged => ax_notification::VALUE_CHANGED,
    Notification::TitleChanged => ax_notification::TITLE_CHANGED,
    Notification::FocusChanged => ax_notification::FOCUS_CHANGED,
    Notification::SelectionChanged => ax_notification::SELECTION_CHANGED,
    Notification::BoundsChanged => ax_notification::BOUNDS_CHANGED,
    Notification::ChildrenChanged => ax_notification::CHILDREN_CHANGED,
  }
}

/// Convert macOS notification string to our Notification.
pub fn notification_from_macos(s: &str) -> Option<Notification> {
  match s {
    ax_notification::DESTROYED => Some(Notification::Destroyed),
    ax_notification::VALUE_CHANGED => Some(Notification::ValueChanged),
    ax_notification::TITLE_CHANGED => Some(Notification::TitleChanged),
    ax_notification::FOCUS_CHANGED => Some(Notification::FocusChanged),
    ax_notification::SELECTION_CHANGED => Some(Notification::SelectionChanged),
    ax_notification::BOUNDS_CHANGED | "AXResized" => Some(Notification::BoundsChanged),
    ax_notification::CHILDREN_CHANGED => Some(Notification::ChildrenChanged),
    _ => None,
  }
}

/// macOS action string constants (kAX*Action).
pub mod ax_action {
  pub const PRESS: &str = "AXPress";
  pub const SHOW_MENU: &str = "AXShowMenu";
  pub const INCREMENT: &str = "AXIncrement";
  pub const DECREMENT: &str = "AXDecrement";
  pub const CONFIRM: &str = "AXConfirm";
  pub const CANCEL: &str = "AXCancel";
  pub const RAISE: &str = "AXRaise";
  pub const PICK: &str = "AXPick";
  pub const EXPAND: &str = "AXExpand";
  pub const COLLAPSE: &str = "AXCollapse";
  pub const SCROLL_TO_VISIBLE: &str = "AXScrollToVisible";
}

/// Convert our Action to macOS action string.
pub fn action_to_macos(a: Action) -> &'static str {
  match a {
    Action::Press => ax_action::PRESS,
    Action::ShowMenu => ax_action::SHOW_MENU,
    Action::Increment => ax_action::INCREMENT,
    Action::Decrement => ax_action::DECREMENT,
    Action::Confirm => ax_action::CONFIRM,
    Action::Cancel => ax_action::CANCEL,
    Action::Raise => ax_action::RAISE,
    Action::Pick => ax_action::PICK,
    Action::Expand => ax_action::EXPAND,
    Action::Collapse => ax_action::COLLAPSE,
    Action::ScrollToVisible => ax_action::SCROLL_TO_VISIBLE,
  }
}

/// Convert macOS action string to our Action.
pub fn action_from_macos(s: &str) -> Option<Action> {
  match s {
    ax_action::PRESS => Some(Action::Press),
    ax_action::SHOW_MENU => Some(Action::ShowMenu),
    ax_action::INCREMENT => Some(Action::Increment),
    ax_action::DECREMENT => Some(Action::Decrement),
    ax_action::CONFIRM => Some(Action::Confirm),
    ax_action::CANCEL => Some(Action::Cancel),
    ax_action::RAISE => Some(Action::Raise),
    ax_action::PICK => Some(Action::Pick),
    ax_action::EXPAND => Some(Action::Expand),
    ax_action::COLLAPSE => Some(Action::Collapse),
    ax_action::SCROLL_TO_VISIBLE => Some(Action::ScrollToVisible),
    _ => None,
  }
}

/// macOS role string constants (kAX*Role).
pub mod ax_role {
  pub const APPLICATION: &str = "AXApplication";
  pub const WINDOW: &str = "AXWindow";
  pub const STANDARD_WINDOW: &str = "AXStandardWindow";
  pub const DOCUMENT: &str = "AXDocument";
  pub const GROUP: &str = "AXGroup";
  pub const SCROLL_AREA: &str = "AXScrollArea";
  pub const TOOLBAR: &str = "AXToolbar";

  pub const MENU: &str = "AXMenu";
  pub const MENU_BAR: &str = "AXMenuBar";
  pub const MENU_ITEM: &str = "AXMenuItem";
  pub const TAB: &str = "AXTab";
  pub const TAB_GROUP: &str = "AXTabGroup";

  pub const LIST: &str = "AXList";
  pub const ROW: &str = "AXRow";
  pub const TABLE: &str = "AXTable";
  pub const CELL: &str = "AXCell";
  pub const OUTLINE: &str = "AXOutline"; // Tree
  pub const OUTLINE_ROW: &str = "AXOutlineRow"; // TreeItem

  pub const BUTTON: &str = "AXButton";
  pub const DEFAULT_BUTTON: &str = "AXDefaultButton";
  pub const LINK: &str = "AXLink";
  pub const TEXT_FIELD: &str = "AXTextField";
  pub const TEXT_AREA: &str = "AXTextArea";
  pub const SECURE_TEXT_FIELD: &str = "AXSecureTextField";
  pub const SEARCH_FIELD: &str = "AXSearchField";
  pub const COMBO_BOX: &str = "AXComboBox";
  pub const CHECKBOX: &str = "AXCheckBox";
  pub const RADIO_BUTTON: &str = "AXRadioButton";
  pub const SLIDER: &str = "AXSlider";
  pub const STEPPER: &str = "AXStepper";
  pub const INCREMENTOR: &str = "AXIncrementor"; // Also stepper
  pub const PROGRESS_INDICATOR: &str = "AXProgressIndicator";

  pub const STATIC_TEXT: &str = "AXStaticText";
  pub const HEADING: &str = "AXHeading";
  pub const IMAGE: &str = "AXImage";
  pub const SPLITTER: &str = "AXSplitter";

  pub const UNKNOWN: &str = "AXUnknown";
}

/// Convert macOS role string to our Role.
/// Expects the exact macOS role string (e.g., "AXButton").
/// Returns `Role::Unknown` for unrecognized roles.
pub fn role_from_macos(platform_role: &str) -> Role {
  match platform_role {
    // Structural
    ax_role::APPLICATION => Role::Application,
    ax_role::WINDOW | ax_role::STANDARD_WINDOW => Role::Window,
    ax_role::DOCUMENT => Role::Document,
    ax_role::GROUP => Role::Group,
    ax_role::SCROLL_AREA => Role::ScrollArea,
    ax_role::TOOLBAR => Role::Toolbar,

    // Navigation
    ax_role::MENU => Role::Menu,
    ax_role::MENU_BAR => Role::MenuBar,
    ax_role::MENU_ITEM => Role::MenuItem,
    ax_role::TAB => Role::Tab,
    ax_role::TAB_GROUP => Role::TabList,

    // Collections
    ax_role::LIST => Role::List,
    ax_role::ROW => Role::ListItem, // Rows in lists are list items
    ax_role::TABLE => Role::Table,
    ax_role::CELL => Role::Cell,
    ax_role::OUTLINE => Role::Tree,
    ax_role::OUTLINE_ROW => Role::TreeItem,

    // Interactive
    ax_role::BUTTON | ax_role::DEFAULT_BUTTON => Role::Button,
    ax_role::LINK => Role::Link,
    ax_role::TEXT_FIELD | ax_role::SECURE_TEXT_FIELD => Role::TextField,
    ax_role::TEXT_AREA => Role::TextArea,
    ax_role::SEARCH_FIELD => Role::SearchField,
    ax_role::COMBO_BOX => Role::ComboBox,
    ax_role::CHECKBOX => Role::Checkbox,
    ax_role::RADIO_BUTTON => Role::RadioButton,
    ax_role::SLIDER => Role::Slider,
    ax_role::STEPPER | ax_role::INCREMENTOR => Role::Stepper,
    ax_role::PROGRESS_INDICATOR => Role::ProgressBar,

    // Static content
    ax_role::STATIC_TEXT => Role::StaticText,
    ax_role::HEADING => Role::Heading,
    ax_role::IMAGE => Role::Image,
    ax_role::SPLITTER => Role::Separator,

    // Fallback
    _ => Role::Unknown,
  }
}

/// Convert our Role to macOS role string.
///
/// Returns the canonical macOS role string (with "AX" prefix).
pub fn role_to_macos(r: Role) -> &'static str {
  match r {
    // Structural
    Role::Application => ax_role::APPLICATION,
    Role::Window => ax_role::WINDOW,
    Role::Document => ax_role::DOCUMENT,
    Role::Group => ax_role::GROUP,
    Role::ScrollArea => ax_role::SCROLL_AREA,
    Role::Toolbar => ax_role::TOOLBAR,

    // Navigation
    Role::Menu => ax_role::MENU,
    Role::MenuBar => ax_role::MENU_BAR,
    Role::MenuItem => ax_role::MENU_ITEM,
    Role::Tab => ax_role::TAB,
    Role::TabList => ax_role::TAB_GROUP,

    // Collections
    Role::List => ax_role::LIST,
    Role::ListItem => ax_role::ROW,
    Role::Table => ax_role::TABLE,
    Role::Row => ax_role::ROW,
    Role::Cell => ax_role::CELL,
    Role::Tree => ax_role::OUTLINE,
    Role::TreeItem => ax_role::OUTLINE_ROW,

    // Interactive
    Role::Button => ax_role::BUTTON,
    Role::Link => ax_role::LINK,
    Role::TextField => ax_role::TEXT_FIELD,
    Role::TextArea => ax_role::TEXT_AREA,
    Role::SearchField => ax_role::SEARCH_FIELD,
    Role::ComboBox => ax_role::COMBO_BOX,
    Role::Checkbox => ax_role::CHECKBOX,
    Role::Switch => ax_role::CHECKBOX, // macOS doesn't have a distinct switch role
    Role::RadioButton => ax_role::RADIO_BUTTON,
    Role::Slider => ax_role::SLIDER,
    Role::Stepper => ax_role::STEPPER,
    Role::ProgressBar => ax_role::PROGRESS_INDICATOR,

    // Static content
    Role::StaticText => ax_role::STATIC_TEXT,
    Role::Heading => ax_role::HEADING,
    Role::Image => ax_role::IMAGE,
    Role::Separator => ax_role::SPLITTER,

    // Fallback
    Role::GenericContainer => ax_role::GROUP,
    Role::Unknown => ax_role::UNKNOWN,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn notification_roundtrip() {
    let notifs = [
      Notification::Destroyed,
      Notification::ValueChanged,
      Notification::TitleChanged,
      Notification::FocusChanged,
      Notification::SelectionChanged,
      Notification::ChildrenChanged,
    ];

    for n in notifs {
      let macos_str = notification_to_macos(n);
      let back = notification_from_macos(macos_str);
      assert_eq!(back, Some(n), "Roundtrip failed for {n:?}");
    }
  }

  #[test]
  fn action_roundtrip() {
    for action in Action::ALL {
      let macos_str = action_to_macos(*action);
      let back = action_from_macos(macos_str);
      assert_eq!(back, Some(*action), "Roundtrip failed for {action:?}");
    }
  }

  #[test]
  fn role_from_macos_exact_strings() {
    assert_eq!(role_from_macos(ax_role::BUTTON), Role::Button);
    assert_eq!(role_from_macos(ax_role::TEXT_FIELD), Role::TextField);
    assert_eq!(role_from_macos(ax_role::WINDOW), Role::Window);
  }

  #[test]
  fn role_from_macos_variants() {
    assert_eq!(role_from_macos(ax_role::STANDARD_WINDOW), Role::Window);
    assert_eq!(role_from_macos(ax_role::DEFAULT_BUTTON), Role::Button);
    assert_eq!(role_from_macos(ax_role::SECURE_TEXT_FIELD), Role::TextField);
  }

  #[test]
  fn unknown_role() {
    assert_eq!(role_from_macos("AXSomeWeirdThing"), Role::Unknown);
    // Without prefix = unknown (we expect exact macOS strings)
    assert_eq!(role_from_macos("Button"), Role::Unknown);
  }

  #[test]
  fn unknown_action() {
    assert_eq!(action_from_macos("AXSomeWeirdAction"), None);
  }

  #[test]
  fn unknown_notification() {
    assert_eq!(notification_from_macos("AXSomeWeirdNotification"), None);
  }
}
