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
