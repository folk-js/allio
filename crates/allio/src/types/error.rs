/*! Error types for Allio operations. */

use super::{ElementId, ProcessId, WindowId};
use crate::a11y::{Action, ValueType};

/// Errors that can occur during Allio operations.
#[derive(Debug, thiserror::Error)]
pub enum AllioError {
  #[error("Accessibility permissions not granted")]
  PermissionDenied,

  #[error("Element not found: {0}")]
  ElementNotFound(ElementId),

  #[error("Window not found: {0}")]
  WindowNotFound(WindowId),

  #[error("Process not found: {0}")]
  ProcessNotFound(ProcessId),

  #[error("Action '{action:?}' failed: {reason}")]
  ActionFailed { action: Action, reason: String },

  #[error("Failed to set value: {reason}")]
  SetValueFailed { reason: String },

  #[error("Type mismatch: expected {expected:?}, got {got:?}")]
  TypeMismatch { expected: ValueType, got: ValueType },

  #[error("No element at position ({x}, {y})")]
  NoElementAtPosition { x: f64, y: f64 },

  #[error("Observer error: {0}")]
  ObserverError(String),

  #[error("Operation not supported: {0}")]
  NotSupported(String),

  #[error("Internal error: {0}")]
  Internal(String),
}

/// Result type for Allio operations.
pub type AllioResult<T> = Result<T, AllioError>;

#[cfg(test)]
mod tests {
  use super::*;

  mod display_formatting {
    use super::*;

    #[test]
    fn permission_denied() {
      let err = AllioError::PermissionDenied;
      assert_eq!(err.to_string(), "Accessibility permissions not granted");
    }

    #[test]
    fn element_not_found() {
      let err = AllioError::ElementNotFound(ElementId(42));
      assert_eq!(err.to_string(), "Element not found: 42");
    }

    #[test]
    fn window_not_found() {
      let err = AllioError::WindowNotFound(WindowId(123));
      assert_eq!(err.to_string(), "Window not found: 123");
    }

    #[test]
    fn process_not_found() {
      let err = AllioError::ProcessNotFound(ProcessId(9999));
      assert_eq!(err.to_string(), "Process not found: 9999");
    }

    #[test]
    fn action_failed() {
      let err = AllioError::ActionFailed {
        action: Action::Press,
        reason: "element not actionable".into(),
      };
      assert_eq!(
        err.to_string(),
        "Action 'Press' failed: element not actionable"
      );
    }

    #[test]
    fn set_value_failed() {
      let err = AllioError::SetValueFailed {
        reason: "element is read-only".into(),
      };
      assert_eq!(err.to_string(), "Failed to set value: element is read-only");
    }

    #[test]
    fn type_mismatch() {
      let err = AllioError::TypeMismatch {
        expected: ValueType::String,
        got: ValueType::Number,
      };
      assert_eq!(err.to_string(), "Type mismatch: expected String, got Number");
    }

    #[test]
    fn no_element_at_position() {
      let err = AllioError::NoElementAtPosition { x: 100.5, y: 200.5 };
      assert_eq!(err.to_string(), "No element at position (100.5, 200.5)");
    }

    #[test]
    fn observer_error() {
      let err = AllioError::ObserverError("failed to create observer".into());
      assert_eq!(err.to_string(), "Observer error: failed to create observer");
    }

    #[test]
    fn not_supported() {
      let err = AllioError::NotSupported("action not available on this element".into());
      assert_eq!(
        err.to_string(),
        "Operation not supported: action not available on this element"
      );
    }

    #[test]
    fn internal_error() {
      let err = AllioError::Internal("unexpected state".into());
      assert_eq!(err.to_string(), "Internal error: unexpected state");
    }
  }

  mod error_properties {
    use super::*;

    #[test]
    fn errors_are_debug() {
      let err = AllioError::PermissionDenied;
      let debug = format!("{:?}", err);
      assert!(debug.contains("PermissionDenied"));
    }

    #[test]
    fn errors_implement_std_error() {
      fn assert_std_error<E: std::error::Error>() {}
      assert_std_error::<AllioError>();
    }

    #[test]
    fn all_variants_constructible() {
      // Ensure all error variants can be created
      let errors: Vec<AllioError> = vec![
        AllioError::PermissionDenied,
        AllioError::ElementNotFound(ElementId(0)),
        AllioError::WindowNotFound(WindowId(0)),
        AllioError::ProcessNotFound(ProcessId(0)),
        AllioError::ActionFailed {
          action: Action::Press,
          reason: String::new(),
        },
        AllioError::SetValueFailed {
          reason: String::new(),
        },
        AllioError::TypeMismatch {
          expected: ValueType::None,
          got: ValueType::None,
        },
        AllioError::NoElementAtPosition { x: 0.0, y: 0.0 },
        AllioError::ObserverError(String::new()),
        AllioError::NotSupported(String::new()),
        AllioError::Internal(String::new()),
      ];
      assert_eq!(errors.len(), 11, "all error variants should be covered");
    }
  }

  mod result_type {
    use super::*;

    #[test]
    fn result_ok() {
      let result: AllioResult<i32> = Ok(42);
      assert!(result.is_ok());
      assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn result_err() {
      let result: AllioResult<i32> = Err(AllioError::PermissionDenied);
      assert!(result.is_err());
    }

    #[test]
    fn result_maps_correctly() {
      let result: AllioResult<i32> = Ok(21);
      let doubled = result.map(|n| n * 2);
      assert_eq!(doubled.unwrap(), 42);
    }
  }
}
