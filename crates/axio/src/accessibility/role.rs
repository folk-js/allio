//! Semantic UI roles.
//!
//! Roles describe what an element *is* in the UI hierarchy.
//! Platform-specific role strings are mapped in `platform/*/mapping.rs`.

use serde::{Deserialize, Serialize};

/// Semantic UI role (cross-platform).
///
/// These roles are inspired by WAI-ARIA but simplified for our use case.
/// Platform mappings (macOS AXRole strings, Windows UIA ControlTypes) are
/// handled by the platform layer. See `platform::map_role_from_platform`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

/// What kind of value can be written to an element with this role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritableAs {
  /// Element does not accept value input
  NotWritable,
  /// Text input (TextField, TextArea, SearchField, ComboBox)
  String,
  /// Integer stepper
  Integer,
  /// Floating point slider
  Float,
  /// Boolean toggle (Checkbox, Switch, RadioButton)
  Boolean,
}

impl Role {
  /// What kind of value can be written to elements with this role.
  pub fn writable_as(&self) -> WritableAs {
    match self {
      Self::TextField | Self::TextArea | Self::SearchField | Self::ComboBox => WritableAs::String,
      Self::Checkbox | Self::Switch | Self::RadioButton => WritableAs::Boolean,
      Self::Slider | Self::ProgressBar => WritableAs::Float,
      Self::Stepper => WritableAs::Integer,
      _ => WritableAs::NotWritable,
    }
  }

  /// Can values be written to elements with this role?
  pub fn is_writable(&self) -> bool {
    !matches!(self.writable_as(), WritableAs::NotWritable)
  }

  /// Should we automatically watch for value changes when this element is focused?
  ///
  /// True for text inputs where we want to track typing in real-time.
  pub fn auto_watch_on_focus(&self) -> bool {
    matches!(self.writable_as(), WritableAs::String)
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
  fn text_fields_are_writable_as_string() {
    assert_eq!(Role::TextField.writable_as(), WritableAs::String);
    assert_eq!(Role::TextArea.writable_as(), WritableAs::String);
    assert_eq!(Role::SearchField.writable_as(), WritableAs::String);
    assert!(Role::TextField.is_writable());
  }

  #[test]
  fn checkboxes_are_writable_as_boolean() {
    assert_eq!(Role::Checkbox.writable_as(), WritableAs::Boolean);
    assert_eq!(Role::Switch.writable_as(), WritableAs::Boolean);
    assert!(Role::Checkbox.is_writable());
  }

  #[test]
  fn buttons_are_not_writable() {
    assert_eq!(Role::Button.writable_as(), WritableAs::NotWritable);
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
