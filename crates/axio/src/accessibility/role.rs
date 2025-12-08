//! Semantic UI roles.
//!
//! Roles describe what an element *is* in the UI hierarchy.
//! Platform-specific role strings are mapped in `platform/*/mapping.rs`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Semantic UI role (cross-platform).
///
/// These roles are inspired by WAI-ARIA but simplified for our use case.
/// Platform mappings (macOS AXRole strings, Windows UIA ControlTypes) are
/// handled by the platform layer. See `platform::map_role_from_platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
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

  // === Static content ===
  StaticText,
  Heading,
  Image,
  Separator,

  // === Fallback ===
  /// Generic container - has children but no specific semantic meaning
  GenericContainer,
  /// Unknown role - platform role didn't map to anything known
  Unknown,
}

/// Expected value type for elements with a given role.
/// Used for type-safe value handling in TypeScript.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum ValueType {
  /// Element does not have a meaningful value
  None,
  /// Text value (TextField, TextArea, SearchField, ComboBox)
  String,
  /// Integer value (Stepper)
  Integer,
  /// Floating point value (Slider, ProgressBar)
  Float,
  /// Boolean value (Checkbox, Switch, RadioButton)
  Boolean,
}

impl Role {
  /// Expected value type for elements with this role.
  pub fn value_type(&self) -> ValueType {
    match self {
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox => ValueType::String,
      Self::Checkbox | Self::Switch | Self::RadioButton => ValueType::Boolean,
      Self::Slider | Self::ProgressBar => ValueType::Float,
      Self::Stepper => ValueType::Integer,
      _ => ValueType::None,
    }
  }

  /// Can values be written to elements with this role?
  pub fn is_writable(&self) -> bool {
    !matches!(self.value_type(), ValueType::None)
  }

  /// Should we automatically watch for value changes when this element is focused?
  ///
  /// True for text inputs where we want to track typing in real-time.
  pub fn auto_watch_on_focus(&self) -> bool {
    matches!(self.value_type(), ValueType::String)
  }

  /// Can elements with this role typically receive keyboard focus?
  pub fn is_focusable(&self) -> bool {
    matches!(
      self,
      // Windows and documents
      Self::Application | Self::Window | Self::Document |
      // Interactive controls
      Self::Button | Self::Link | Self::MenuItem |
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox |
      Self::Checkbox | Self::Switch | Self::RadioButton |
      Self::Slider | Self::Stepper |
      Self::Tab |
      // Collections (for keyboard navigation)
      Self::List | Self::Table | Self::Tree
    )
  }

  /// Does this role typically contain other elements?
  pub fn is_container(&self) -> bool {
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
        | Self::GenericContainer
    )
  }

  /// Is this an interactive element that users can click/activate?
  pub fn is_interactive(&self) -> bool {
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
        | Self::ListItem
        | Self::TreeItem
        | Self::Cell
    )
  }

  /// Is this a text input element?
  pub fn is_text_input(&self) -> bool {
    matches!(
      self,
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox
    )
  }

  /// Can elements with this role have a meaningful value attribute?
  pub fn can_have_value(&self) -> bool {
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
