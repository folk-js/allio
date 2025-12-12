/*!
Bidirectional mappings between Allio accessibility types and macOS AX* strings.
*/

use crate::a11y::{Action, Notification, Role};

/// macOS notification string constants (kAX*Notification).
mod ax_notification {
  pub(super) const DESTROYED: &str = "AXUIElementDestroyed";
  pub(super) const VALUE_CHANGED: &str = "AXValueChanged";
  pub(super) const TITLE_CHANGED: &str = "AXTitleChanged";
  pub(super) const FOCUS_CHANGED: &str = "AXFocusedUIElementChanged";
  pub(super) const SELECTION_CHANGED: &str = "AXSelectedTextChanged";
  pub(super) const BOUNDS_CHANGED: &str = "AXMoved"; // Also AXResized
  pub(super) const CHILDREN_CHANGED: &str = "AXLayoutChanged";
}

/// Convert our Notification to macOS notification string.
pub(super) const fn notification_to_macos(n: Notification) -> &'static str {
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
pub(super) fn notification_from_macos(s: &str) -> Option<Notification> {
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
pub(in crate::platform::macos) mod ax_action {
  pub(in crate::platform::macos) const PRESS: &str = "AXPress";
  pub(super) const SHOW_MENU: &str = "AXShowMenu";
  pub(super) const INCREMENT: &str = "AXIncrement";
  pub(super) const DECREMENT: &str = "AXDecrement";
  pub(super) const CONFIRM: &str = "AXConfirm";
  pub(super) const CANCEL: &str = "AXCancel";
  pub(super) const RAISE: &str = "AXRaise";
  pub(super) const PICK: &str = "AXPick";
  pub(super) const EXPAND: &str = "AXExpand";
  pub(super) const COLLAPSE: &str = "AXCollapse";
  pub(super) const SCROLL_TO_VISIBLE: &str = "AXScrollToVisible";
}

/// Convert our Action to macOS action string.
pub(super) const fn action_to_macos(a: Action) -> &'static str {
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
pub(in crate::platform) fn action_from_macos(s: &str) -> Option<Action> {
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
pub(in crate::platform::macos) mod ax_role {
  // Structural
  pub(super) const APPLICATION: &str = "AXApplication";
  pub(in crate::platform::macos) const WINDOW: &str = "AXWindow";
  pub(super) const STANDARD_WINDOW: &str = "AXStandardWindow";
  pub(super) const DOCUMENT: &str = "AXDocument";
  pub(super) const WEB_AREA: &str = "AXWebArea";
  pub(super) const GROUP: &str = "AXGroup";
  pub(super) const SPLIT_GROUP: &str = "AXSplitGroup";
  pub(super) const RADIO_GROUP: &str = "AXRadioGroup";
  pub(super) const SCROLL_AREA: &str = "AXScrollArea";
  pub(super) const TOOLBAR: &str = "AXToolbar";

  // Navigation
  pub(super) const MENU: &str = "AXMenu";
  pub(super) const MENU_BAR: &str = "AXMenuBar";
  pub(super) const MENU_ITEM: &str = "AXMenuItem";
  pub(super) const TAB: &str = "AXTab";
  pub(super) const TAB_GROUP: &str = "AXTabGroup";

  // Collections
  pub(super) const LIST: &str = "AXList";
  pub(super) const ROW: &str = "AXRow";
  pub(super) const TABLE: &str = "AXTable";
  pub(super) const CELL: &str = "AXCell";
  pub(super) const OUTLINE: &str = "AXOutline"; // Tree
  pub(super) const OUTLINE_ROW: &str = "AXOutlineRow"; // TreeItem

  // Interactive
  pub(super) const BUTTON: &str = "AXButton";
  pub(super) const DEFAULT_BUTTON: &str = "AXDefaultButton";
  pub(super) const MENU_BUTTON: &str = "AXMenuButton";
  pub(super) const LINK: &str = "AXLink";
  pub(super) const TEXT_FIELD: &str = "AXTextField";
  pub(super) const TEXT_AREA: &str = "AXTextArea";
  pub(super) const SECURE_TEXT_FIELD: &str = "AXSecureTextField";
  pub(super) const SEARCH_FIELD: &str = "AXSearchField";
  pub(super) const COMBO_BOX: &str = "AXComboBox";
  pub(super) const CHECKBOX: &str = "AXCheckBox";
  pub(super) const RADIO_BUTTON: &str = "AXRadioButton";
  pub(super) const SLIDER: &str = "AXSlider";
  pub(super) const STEPPER: &str = "AXStepper";
  pub(super) const INCREMENTOR: &str = "AXIncrementor"; // Also stepper
  pub(super) const PROGRESS_INDICATOR: &str = "AXProgressIndicator";
  pub(super) const COLOR_WELL: &str = "AXColorWell";

  // Static content
  pub(super) const STATIC_TEXT: &str = "AXStaticText";
  pub(super) const HEADING: &str = "AXHeading";
  pub(super) const IMAGE: &str = "AXImage";
  pub(super) const SPLITTER: &str = "AXSplitter";

  // Generic elements (known, non-semantic chrome)
  pub(super) const SCROLL_BAR: &str = "AXScrollBar";
  pub(super) const VALUE_INDICATOR: &str = "AXValueIndicator";
  pub(super) const HANDLE: &str = "AXHandle";
  pub(super) const MATTE: &str = "AXMatte";
  pub(super) const RULER: &str = "AXRuler";
  pub(super) const RULER_MARKER: &str = "AXRulerMarker";
  pub(super) const GROW_AREA: &str = "AXGrowArea";
  pub(super) const DRAWER: &str = "AXDrawer";
  pub(super) const POPOVER: &str = "AXPopover";
  pub(super) const LAYOUT_AREA: &str = "AXLayoutArea";
  pub(super) const LAYOUT_ITEM: &str = "AXLayoutItem";
  pub(super) const RELEVANCE_INDICATOR: &str = "AXRelevanceIndicator";
  pub(super) const LEVEL_INDICATOR: &str = "AXLevelIndicator";
  pub(super) const BUSY_INDICATOR: &str = "AXBusyIndicator";

  pub(super) const UNKNOWN: &str = "AXUnknown";
}

/// Convert macOS role string to our Role.
pub(in crate::platform) fn role_from_macos(platform_role: &str) -> Role {
  match platform_role {
    // Structural
    ax_role::APPLICATION => Role::Application,
    ax_role::WINDOW | ax_role::STANDARD_WINDOW => Role::Window,
    ax_role::DOCUMENT | ax_role::WEB_AREA => Role::Document,
    ax_role::GROUP | ax_role::SPLIT_GROUP | ax_role::RADIO_GROUP => Role::Group,
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
    ax_role::BUTTON | ax_role::DEFAULT_BUTTON | ax_role::MENU_BUTTON => Role::Button,
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
    ax_role::COLOR_WELL => Role::ColorWell,

    // Static content
    ax_role::STATIC_TEXT => Role::StaticText,
    ax_role::HEADING => Role::Heading,
    ax_role::IMAGE => Role::Image,
    ax_role::SPLITTER => Role::Separator,

    // Known non-semantic leaf elements → GenericElement
    ax_role::SCROLL_BAR
    | ax_role::VALUE_INDICATOR
    | ax_role::HANDLE
    | ax_role::MATTE
    | ax_role::RULER
    | ax_role::RULER_MARKER
    | ax_role::GROW_AREA
    | ax_role::DRAWER
    | ax_role::POPOVER
    | ax_role::LAYOUT_AREA
    | ax_role::LAYOUT_ITEM
    | ax_role::RELEVANCE_INDICATOR
    | ax_role::LEVEL_INDICATOR
    | ax_role::BUSY_INDICATOR => Role::GenericElement,

    ax_role::UNKNOWN => Role::Unknown,

    _ => {
      log::warn!("Unknown macOS role: {platform_role}");
      Role::Unknown
    }
  }
}

#[allow(dead_code)]
const fn role_to_macos(r: Role) -> &'static str {
  match r {
    // Structural
    Role::Application => ax_role::APPLICATION,
    Role::Window => ax_role::WINDOW,
    Role::Document => ax_role::DOCUMENT,
    Role::Group | Role::GenericGroup => ax_role::GROUP,
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
    Role::ListItem | Role::Row => ax_role::ROW,
    Role::Table => ax_role::TABLE,
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
    Role::Checkbox | Role::Switch => ax_role::CHECKBOX, // macOS doesn't have distinct switch role
    Role::RadioButton => ax_role::RADIO_BUTTON,
    Role::Slider => ax_role::SLIDER,
    Role::Stepper => ax_role::STEPPER,
    Role::ProgressBar => ax_role::PROGRESS_INDICATOR,
    Role::ColorWell => ax_role::COLOR_WELL,

    // Static content
    Role::StaticText => ax_role::STATIC_TEXT,
    Role::Heading => ax_role::HEADING,
    Role::Image => ax_role::IMAGE,
    Role::Separator => ax_role::SPLITTER,

    // Fallback
    Role::Unknown | Role::GenericElement => ax_role::UNKNOWN,
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
