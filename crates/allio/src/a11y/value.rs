/*!
Element values.

Values represent the current state of interactive elements:
text content, numeric positions, boolean states, etc.
*/

#![allow(missing_docs)]

use super::ValueType;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// RGBA color with float components (0.0-1.0).
///
/// Used for color picker elements (`ColorWell`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Color {
  /// Red component (0.0-1.0)
  pub r: f64,
  /// Green component (0.0-1.0)
  pub g: f64,
  /// Blue component (0.0-1.0)
  pub b: f64,
  /// Alpha/opacity component (0.0-1.0)
  pub a: f64,
}

impl Color {
  /// Create a new color from RGBA components (0.0-1.0 range).
  pub const fn new(r: f64, g: f64, b: f64, a: f64) -> Self {
    Self { r, g, b, a }
  }

  /// Create an opaque color (alpha = 1.0).
  pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
    Self { r, g, b, a: 1.0 }
  }
}

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

  /// Color value (color wells/pickers)
  Color(Color),
}

impl Value {
  /// Get as string reference if this is a String value.
  pub fn as_str(&self) -> Option<&str> {
    match self {
      Self::String(s) => Some(s),
      Self::Number(_) | Self::Boolean(_) | Self::Color(_) => None,
    }
  }

  /// Get as owned String, converting numbers/bools/colors to their string representation.
  #[allow(clippy::cast_possible_truncation)] // Intentional: formatting display value
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
      #[allow(clippy::cast_sign_loss)] // Color components are clamped to 0.0-1.0
      Self::Color(c) => {
        // Format as CSS rgba()
        let r = (c.r * 255.0).round() as u8;
        let g = (c.g * 255.0).round() as u8;
        let b = (c.b * 255.0).round() as u8;
        format!("rgba({r}, {g}, {b}, {})", c.a)
      }
    }
  }

  /// Get as f64 if this is a Number value.
  pub const fn as_f64(&self) -> Option<f64> {
    match self {
      Self::Number(n) => Some(*n),
      Self::String(_) | Self::Boolean(_) | Self::Color(_) => None,
    }
  }

  /// Get as i64 (truncated) if this is a Number value.
  #[allow(clippy::cast_possible_truncation)] // Intentional: caller expects truncation
  pub const fn as_i64(&self) -> Option<i64> {
    match self {
      Self::Number(n) => Some(*n as i64),
      Self::String(_) | Self::Boolean(_) | Self::Color(_) => None,
    }
  }

  /// Get as bool if this is a Boolean value.
  pub const fn as_bool(&self) -> Option<bool> {
    match self {
      Self::Boolean(b) => Some(*b),
      Self::String(_) | Self::Number(_) | Self::Color(_) => None,
    }
  }

  /// Get as Color if this is a Color value.
  pub const fn as_color(&self) -> Option<&Color> {
    match self {
      Self::Color(c) => Some(c),
      Self::String(_) | Self::Number(_) | Self::Boolean(_) => None,
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

  pub const fn is_color(&self) -> bool {
    matches!(self, Self::Color(_))
  }

  /// Get the `ValueType` for this value.
  pub const fn value_type(&self) -> ValueType {
    match self {
      Self::String(_) => ValueType::String,
      Self::Number(_) => ValueType::Number,
      Self::Boolean(_) => ValueType::Boolean,
      Self::Color(_) => ValueType::Color,
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
  #[allow(clippy::cast_precision_loss)] // Acceptable: i64 range rarely needs full precision
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

impl From<Color> for Value {
  fn from(c: Color) -> Self {
    Self::Color(c)
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

  #[test]
  fn color_accessors() {
    let c = Color::new(1.0, 0.5, 0.0, 0.8);
    let v = Value::Color(c);
    assert!(v.is_color());
    assert_eq!(v.as_color(), Some(&c));
    assert_eq!(v.as_str(), None);
    assert_eq!(v.as_f64(), None);
    assert_eq!(v.as_bool(), None);
  }

  #[test]
  fn color_into_string() {
    let c = Color::new(1.0, 0.5, 0.0, 0.8);
    let v = Value::Color(c);
    assert_eq!(v.into_string(), "rgba(255, 128, 0, 0.8)");
  }

  #[test]
  fn color_from_impl() {
    let c = Color::rgb(0.5, 0.5, 0.5);
    let v: Value = c.into();
    assert!(v.is_color());
  }

  #[test]
  fn value_type_returns_correct_variant() {
    assert_eq!(Value::String("".into()).value_type(), ValueType::String);
    assert_eq!(Value::Number(0.0).value_type(), ValueType::Number);
    assert_eq!(Value::Boolean(false).value_type(), ValueType::Boolean);
    assert_eq!(
      Value::Color(Color::rgb(0.0, 0.0, 0.0)).value_type(),
      ValueType::Color
    );
  }

  mod edge_cases {
    use super::*;

    #[test]
    fn empty_string() {
      let v = Value::String(String::new());
      assert_eq!(v.as_str(), Some(""));
      assert_eq!(v.into_string(), "");
    }

    #[test]
    fn negative_numbers() {
      let v = Value::Number(-42.5);
      assert_eq!(v.as_f64(), Some(-42.5));
      assert_eq!(v.as_i64(), Some(-42));
    }

    #[test]
    fn large_numbers() {
      let v = Value::Number(1e15);
      assert_eq!(v.as_f64(), Some(1e15));
      // Should format as integer since fract() == 0.0
      assert_eq!(v.into_string(), "1000000000000000");
    }

    #[test]
    fn special_floats() {
      // NaN - as_i64 behavior
      let nan = Value::Number(f64::NAN);
      assert!(nan.as_f64().unwrap().is_nan());

      // Infinity
      let inf = Value::Number(f64::INFINITY);
      assert_eq!(inf.as_f64(), Some(f64::INFINITY));
      assert_eq!(inf.clone().into_string(), "inf");

      // Negative infinity
      let neg_inf = Value::Number(f64::NEG_INFINITY);
      assert_eq!(neg_inf.as_f64(), Some(f64::NEG_INFINITY));
    }
  }
}

#[cfg(test)]
mod proptests {
  use super::*;
  use proptest::prelude::*;

  proptest! {
    /// String values roundtrip through as_str
    #[test]
    fn string_roundtrip(s in ".*") {
      let v = Value::from(s.clone());
      prop_assert_eq!(v.as_str(), Some(s.as_str()));
    }

    /// Boolean values roundtrip through as_bool
    #[test]
    fn bool_roundtrip(b in any::<bool>()) {
      let v = Value::from(b);
      prop_assert_eq!(v.as_bool(), Some(b));
    }

    /// Finite f64 values roundtrip through as_f64
    #[test]
    fn f64_roundtrip(n in any::<f64>().prop_filter("finite", |n| n.is_finite())) {
      let v = Value::from(n);
      prop_assert_eq!(v.as_f64(), Some(n));
    }

    /// i32 values roundtrip through Number -> as_f64 -> cast
    #[test]
    fn i32_roundtrip(n in any::<i32>()) {
      let v = Value::from(n);
      let back = v.as_f64().map(|f| f as i32);
      prop_assert_eq!(back, Some(n));
    }

    /// value_type matches the variant
    #[test]
    fn value_type_consistency(s in ".*", n in any::<f64>().prop_filter("finite", |n| n.is_finite()), b in any::<bool>()) {
      prop_assert!(Value::from(s).is_string());
      prop_assert!(Value::from(n).is_number());
      prop_assert!(Value::from(b).is_boolean());
    }

    /// String values are never confused with other types
    #[test]
    fn string_type_exclusivity(s in ".*") {
      let v = Value::from(s);
      prop_assert!(v.is_string());
      prop_assert!(!v.is_number());
      prop_assert!(!v.is_boolean());
      prop_assert!(!v.is_color());
      prop_assert!(v.as_f64().is_none());
      prop_assert!(v.as_bool().is_none());
      prop_assert!(v.as_color().is_none());
    }

    /// Color components are preserved
    #[test]
    fn color_roundtrip(
      r in 0.0..=1.0f64,
      g in 0.0..=1.0f64,
      b in 0.0..=1.0f64,
      a in 0.0..=1.0f64
    ) {
      let c = Color::new(r, g, b, a);
      let v = Value::from(c);
      let back = v.as_color().unwrap();
      prop_assert_eq!(back.r, r);
      prop_assert_eq!(back.g, g);
      prop_assert_eq!(back.b, b);
      prop_assert_eq!(back.a, a);
    }
  }
}
