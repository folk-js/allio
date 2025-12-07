//! Element values.
//!
//! Values represent the current state of interactive elements:
//! text content, numeric positions, boolean states, etc.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Typed value for an accessibility element.
///
/// Different roles have different value types:
/// - TextField, TextArea: String
/// - Checkbox, Switch: Boolean
/// - Slider: Float
/// - Stepper: Integer
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value")]
#[ts(export, export_to = "packages/axio-client/src/types/generated/")]
pub enum Value {
  /// Text content (text fields, labels)
  String(String),

  /// Integer value (steppers, discrete controls)
  Integer(i64),

  /// Floating point value (sliders, progress bars)
  Float(f64),

  /// Boolean state (checkboxes, switches)
  Boolean(bool),
}

impl Value {
  // === Type-specific accessors ===

  /// Get as string reference if this is a String value.
  pub fn as_str(&self) -> Option<&str> {
    match self {
      Self::String(s) => Some(s),
      _ => None,
    }
  }

  /// Get as owned String, converting numbers/bools to their string representation.
  pub fn into_string(self) -> String {
    match self {
      Self::String(s) => s,
      Self::Integer(i) => i.to_string(),
      Self::Float(f) => f.to_string(),
      Self::Boolean(b) => b.to_string(),
    }
  }

  /// Get as i64 if this is an Integer or can be converted from Float.
  pub fn as_i64(&self) -> Option<i64> {
    match self {
      Self::Integer(i) => Some(*i),
      Self::Float(f) => Some(*f as i64),
      _ => None,
    }
  }

  /// Get as f64 if this is a Float or Integer.
  pub fn as_f64(&self) -> Option<f64> {
    match self {
      Self::Float(f) => Some(*f),
      Self::Integer(i) => Some(*i as f64),
      _ => None,
    }
  }

  /// Get as bool if this is a Boolean value.
  pub fn as_bool(&self) -> Option<bool> {
    match self {
      Self::Boolean(b) => Some(*b),
      _ => None,
    }
  }

  // === Type checks ===

  pub fn is_string(&self) -> bool {
    matches!(self, Self::String(_))
  }

  pub fn is_integer(&self) -> bool {
    matches!(self, Self::Integer(_))
  }

  pub fn is_float(&self) -> bool {
    matches!(self, Self::Float(_))
  }

  pub fn is_boolean(&self) -> bool {
    matches!(self, Self::Boolean(_))
  }

  pub fn is_numeric(&self) -> bool {
    matches!(self, Self::Integer(_) | Self::Float(_))
  }
}

// === From impls for ergonomic construction ===

impl From<String> for Value {
  fn from(s: String) -> Self {
    Self::String(s)
  }
}

impl From<&str> for Value {
  fn from(s: &str) -> Self {
    Self::String(s.to_owned())
  }
}

impl From<i64> for Value {
  fn from(i: i64) -> Self {
    Self::Integer(i)
  }
}

impl From<i32> for Value {
  fn from(i: i32) -> Self {
    Self::Integer(i as i64)
  }
}

impl From<f64> for Value {
  fn from(f: f64) -> Self {
    Self::Float(f)
  }
}

impl From<f32> for Value {
  fn from(f: f32) -> Self {
    Self::Float(f as f64)
  }
}

impl From<bool> for Value {
  fn from(b: bool) -> Self {
    Self::Boolean(b)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn string_accessors() {
    let v = Value::String("hello".into());
    assert_eq!(v.as_str(), Some("hello"));
    assert_eq!(v.as_i64(), None);
  }

  #[test]
  fn numeric_conversions() {
    let int = Value::Integer(42);
    assert_eq!(int.as_i64(), Some(42));
    assert_eq!(int.as_f64(), Some(42.0));

    let float = Value::Float(3.14);
    assert_eq!(float.as_f64(), Some(3.14));
    assert_eq!(float.as_i64(), Some(3)); // truncates
  }

  #[test]
  fn into_string_converts() {
    assert_eq!(Value::String("test".into()).into_string(), "test");
    assert_eq!(Value::Integer(42).into_string(), "42");
    assert_eq!(Value::Float(3.14).into_string(), "3.14");
    assert_eq!(Value::Boolean(true).into_string(), "true");
  }

  #[test]
  fn from_impls() {
    let _: Value = "test".into();
    let _: Value = String::from("test").into();
    let _: Value = 42i64.into();
    let _: Value = 42i32.into();
    let _: Value = 3.14f64.into();
    let _: Value = true.into();
  }
}
