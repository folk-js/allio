/*! Error types for AXIO operations. */

use super::{ElementId, WindowId};

/// Errors that can occur during AXIO operations.
#[derive(Debug, thiserror::Error)]
pub enum AxioError {
  #[error("Element not found: {0}")]
  ElementNotFound(ElementId),

  #[error("Window not found: {0}")]
  WindowNotFound(WindowId),

  #[error("Accessibility operation failed: {0}")]
  AccessibilityError(String),

  #[error("Observer error: {0}")]
  ObserverError(String),

  #[error("Operation not supported: {0}")]
  NotSupported(String),

  #[error("Internal error: {0}")]
  Internal(String),
}

/// Result type for AXIO operations.
pub type AxioResult<T> = Result<T, AxioError>;

