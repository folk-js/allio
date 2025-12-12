/*!
Element values.

Values represent the current state of interactive elements:
text content, numeric positions, boolean states, etc.
*/

#![allow(missing_docs)]
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use super::ValueType;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Typed value for an accessibility element.
///
/// Role provides semantic context for how to interpret values:
/// - `TextField`, `TextArea` → String
/// - Checkbox, Switch → Boolean
/// - Slider, `ProgressBar` → Number (float)
/// - Stepper → Number (integer, as whole f64)
///
/// Number is unified f64 for JSON/TypeScript compatibility.
/// Use `Role::expects_integer()` to know if display should truncate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value")]
#[ts(export)]
pub enum Value {
  /// Text content (text fields, labels)
  String(String),

  /// Numeric value (sliders, steppers, progress bars)
  /// Integers are stored as whole f64 values.
  Number(f64),

  /// Boolean state (checkboxes, switches)
  Boolean(bool),
}

impl Value {
  /// Get as string reference if this is a String value.
  pub fn as_str(&self) -> Option<&str> {
    match self {
      Self::String(s) => Some(s),
      Self::Number(_) | Self::Boolean(_) => None,
    }
  }

  /// Get as owned String, converting numbers/bools to their string representation.
  pub fn into_string(self) -> String {
    match self {
      Self::String(s) => s,
      Self::Number(n) => {
        // Format integers without decimal point
        if n.fract() == 0.0 {
          format!("{}", n as i64)
        } else {
          n.to_string()
        }
      }
      Self::Boolean(b) => b.to_string(),
    }
  }

  /// Get as f64 if this is a Number value.
  pub const fn as_f64(&self) -> Option<f64> {
    match self {
      Self::Number(n) => Some(*n),
      Self::String(_) | Self::Boolean(_) => None,
    }
  }

  /// Get as i64 (truncated) if this is a Number value.
  pub const fn as_i64(&self) -> Option<i64> {
    match self {
      Self::Number(n) => Some(*n as i64),
      Self::String(_) | Self::Boolean(_) => None,
    }
  }

  /// Get as bool if this is a Boolean value.
  pub const fn as_bool(&self) -> Option<bool> {
    match self {
      Self::Boolean(b) => Some(*b),
      Self::String(_) | Self::Number(_) => None,
    }
  }

  pub const fn is_string(&self) -> bool {
    matches!(self, Self::String(_))
  }

  pub const fn is_number(&self) -> bool {
    matches!(self, Self::Number(_))
  }

  pub const fn is_boolean(&self) -> bool {
    matches!(self, Self::Boolean(_))
  }

  /// Get the `ValueType` for this value.
  pub const fn value_type(&self) -> ValueType {
    match self {
      Self::String(_) => ValueType::String,
      Self::Number(_) => ValueType::Number,
      Self::Boolean(_) => ValueType::Boolean,
    }
  }
}

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

impl From<f64> for Value {
  fn from(n: f64) -> Self {
    Self::Number(n)
  }
}

impl From<f32> for Value {
  fn from(n: f32) -> Self {
    Self::Number(f64::from(n))
  }
}

impl From<i64> for Value {
  fn from(n: i64) -> Self {
    Self::Number(n as f64)
  }
}

impl From<i32> for Value {
  fn from(n: i32) -> Self {
    Self::Number(f64::from(n))
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
    assert_eq!(v.as_f64(), None);
  }

  #[test]
  fn number_accessors() {
    let int = Value::Number(42.0);
    assert_eq!(int.as_i64(), Some(42));
    assert_eq!(int.as_f64(), Some(42.0));

    let float = Value::Number(3.14);
    assert_eq!(float.as_f64(), Some(3.14));
    assert_eq!(float.as_i64(), Some(3)); // truncates
  }

  #[test]
  fn into_string_converts() {
    assert_eq!(Value::String("test".into()).into_string(), "test");
    assert_eq!(Value::Number(42.0).into_string(), "42");
    assert_eq!(Value::Number(3.14).into_string(), "3.14");
    assert_eq!(Value::Boolean(true).into_string(), "true");
  }

  #[test]
  #[allow(let_underscore_drop)]
  fn from_impls() {
    let _: Value = "test".into();
    let _: Value = String::from("test").into();
    let _: Value = 42i64.into();
    let _: Value = 42i32.into();
    let _: Value = 3.14f64.into();
    let _: Value = true.into();
  }

  #[test]
  fn integer_display() {
    // Integers should format without decimal
    assert_eq!(Value::Number(42.0).into_string(), "42");
    assert_eq!(Value::Number(0.0).into_string(), "0");
    assert_eq!(Value::Number(-5.0).into_string(), "-5");
  }
}
