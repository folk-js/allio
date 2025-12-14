/*!
Semantic UI roles.

Roles describe what an element *is* in the UI hierarchy.
Platform-specific role strings are mapped in `platform/macos/mapping.rs`.
*/

#![allow(missing_docs)]

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Semantic UI role (cross-platform).
///
/// These roles are inspired by WAI-ARIA but simplified for our use case.
/// Platform mappings (macOS `AXRole` strings, Windows UIA `ControlTypes`) are
/// handled by the platform layer. See `platform::map_role_from_platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Role {
  // === Structural / Containers ===
  Application,
  Window,
  Document,
  Group,
  ScrollArea,
  Toolbar,

  // === Navigation ===
  Menu,
  MenuBar,
  MenuItem,
  Tab,
  TabList,

  // === Collections ===
  List,
  ListItem,
  Table,
  Row,
  Cell,
  Tree,
  TreeItem,

  // === Interactive ===
  Button,
  Link,
  TextField,
  TextArea,
  SearchField,
  ComboBox,
  Checkbox,
  Switch,
  RadioButton,
  Slider,
  Stepper,
  ProgressBar,
  ColorWell,

  // === Static content ===
  StaticText,
  Heading,
  Image,
  Separator,

  // === Generic / Fallback ===
  /// Generic container - layout-only groups with no semantic meaning.
  /// Candidates for tree-collapsing (e.g., a group containing just one group).
  /// Mapped from `AXGroup` when there's no label/value.
  GenericGroup,

  /// Generic element - known platform elements without specific semantics.
  /// Explicitly mapped (not unknown), but non-interactive chrome like scrollbars.
  /// Will be pruned from simplified views; `platform_role` preserved for debugging.
  GenericElement,

  /// Unknown role - platform role didn't map to anything known.
  /// This indicates a gap in our mappings that should be addressed.
  #[default]
  Unknown,
}

/// Expected value type for elements with a given role.
/// Used for type-safe value handling in TypeScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ValueType {
  /// Element does not have a meaningful value
  None,
  /// Text value (`TextField`, `TextArea`, `SearchField`, `ComboBox`)
  String,
  /// Numeric value (Slider, `ProgressBar`, Stepper)
  /// Use `Role::expects_integer()` to check if integer vs float.
  Number,
  /// Boolean value (Checkbox, Switch, `RadioButton`)
  Boolean,
  /// Color value (`ColorWell`)
  Color,
}

impl Role {
  /// Expected value type for elements with this role.
  ///
  /// # Example
  ///
  /// ```
  /// use allio::a11y::{Role, ValueType};
  ///
  /// assert_eq!(Role::TextField.value_type(), ValueType::String);
  /// assert_eq!(Role::Checkbox.value_type(), ValueType::Boolean);
  /// assert_eq!(Role::Slider.value_type(), ValueType::Number);
  /// assert_eq!(Role::Button.value_type(), ValueType::None);
  /// ```
  pub const fn value_type(&self) -> ValueType {
    match self {
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox => ValueType::String,
      Self::Checkbox | Self::Switch | Self::RadioButton => ValueType::Boolean,
      Self::Slider | Self::ProgressBar | Self::Stepper => ValueType::Number,
      Self::ColorWell => ValueType::Color,
      // Structural/navigation/static roles have no editable value
      Self::Application
      | Self::Window
      | Self::Document
      | Self::Group
      | Self::ScrollArea
      | Self::Toolbar
      | Self::Menu
      | Self::MenuBar
      | Self::MenuItem
      | Self::Tab
      | Self::TabList
      | Self::List
      | Self::ListItem
      | Self::Table
      | Self::Row
      | Self::Cell
      | Self::Tree
      | Self::TreeItem
      | Self::Button
      | Self::Link
      | Self::StaticText
      | Self::Heading
      | Self::Image
      | Self::Separator
      | Self::GenericGroup
      | Self::GenericElement
      | Self::Unknown => ValueType::None,
    }
  }

  /// Should numeric values for this role be treated as integers?
  ///
  /// Returns true for roles like Stepper where values are discrete.
  /// Returns false for roles like Slider where values are continuous.
  pub const fn expects_integer(&self) -> bool {
    matches!(self, Self::Stepper)
  }

  /// Can values be written to elements with this role?
  pub const fn is_writable(&self) -> bool {
    !matches!(self.value_type(), ValueType::None)
  }

  /// Should we automatically watch for value changes when this element is focused?
  ///
  /// True for text inputs where we want to track typing in real-time.
  pub const fn auto_watch_on_focus(&self) -> bool {
    matches!(self.value_type(), ValueType::String)
  }

  /// Can elements with this role typically receive keyboard focus?
  pub const fn is_focusable(&self) -> bool {
    matches!(
      self,
      // Windows and documents
      Self::Application | Self::Window | Self::Document |
      // Interactive controls
      Self::Button | Self::Link | Self::MenuItem |
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox |
      Self::Checkbox | Self::Switch | Self::RadioButton |
      Self::Slider | Self::Stepper | Self::ColorWell |
      Self::Tab |
      // Collections (for keyboard navigation)
      Self::List | Self::Table | Self::Tree
    )
  }

  /// Does this role typically contain other elements?
  pub const fn is_container(&self) -> bool {
    matches!(
      self,
      Self::Application
        | Self::Window
        | Self::Document
        | Self::Group
        | Self::ScrollArea
        | Self::Toolbar
        | Self::Menu
        | Self::MenuBar
        | Self::TabList
        | Self::List
        | Self::Table
        | Self::Tree
        | Self::Row
        | Self::GenericGroup
        | Self::GenericElement // May contain children (e.g., scrollbar with buttons)
    )
  }

  /// Is this a generic/placeholder role that may be pruned from simplified views?
  pub const fn is_generic(&self) -> bool {
    matches!(
      self,
      Self::GenericGroup | Self::GenericElement | Self::Unknown
    )
  }

  /// Is this an interactive element that users can click/activate?
  pub const fn is_interactive(&self) -> bool {
    matches!(
      self,
      Self::Button
        | Self::Link
        | Self::MenuItem
        | Self::Tab
        | Self::TextField
        | Self::TextArea
        | Self::SearchField
        | Self::ComboBox
        | Self::Checkbox
        | Self::Switch
        | Self::RadioButton
        | Self::Slider
        | Self::Stepper
        | Self::ColorWell
        | Self::ListItem
        | Self::TreeItem
        | Self::Cell
    )
  }

  /// Is this a text input element?
  pub const fn is_text_input(&self) -> bool {
    matches!(
      self,
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox
    )
  }

  /// Can elements with this role have a meaningful value attribute?
  pub const fn can_have_value(&self) -> bool {
    matches!(
      self,
      Self::TextField
        | Self::TextArea
        | Self::SearchField
        | Self::ComboBox
        | Self::Checkbox
        | Self::Switch
        | Self::RadioButton
        | Self::Slider
        | Self::Stepper
        | Self::ProgressBar
        | Self::ColorWell
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn text_fields_have_string_value_type() {
    assert_eq!(Role::TextField.value_type(), ValueType::String);
    assert_eq!(Role::TextArea.value_type(), ValueType::String);
    assert_eq!(Role::SearchField.value_type(), ValueType::String);
    assert!(Role::TextField.is_writable());
  }

  #[test]
  fn checkboxes_have_boolean_value_type() {
    assert_eq!(Role::Checkbox.value_type(), ValueType::Boolean);
    assert_eq!(Role::Switch.value_type(), ValueType::Boolean);
    assert!(Role::Checkbox.is_writable());
  }

  #[test]
  fn numeric_roles_have_number_value_type() {
    assert_eq!(Role::Slider.value_type(), ValueType::Number);
    assert_eq!(Role::Stepper.value_type(), ValueType::Number);
    assert_eq!(Role::ProgressBar.value_type(), ValueType::Number);
  }

  #[test]
  fn stepper_expects_integer() {
    assert!(Role::Stepper.expects_integer());
    assert!(!Role::Slider.expects_integer());
    assert!(!Role::ProgressBar.expects_integer());
  }

  #[test]
  fn buttons_have_no_value_type() {
    assert_eq!(Role::Button.value_type(), ValueType::None);
    assert!(!Role::Button.is_writable());
  }

  #[test]
  fn text_inputs_auto_watch() {
    assert!(Role::TextField.auto_watch_on_focus());
    assert!(Role::TextArea.auto_watch_on_focus());
    assert!(!Role::Button.auto_watch_on_focus());
    assert!(!Role::Checkbox.auto_watch_on_focus());
  }
}
