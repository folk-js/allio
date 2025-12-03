//! AXIO - Accessibility I/O Layer
//!
//! A platform-agnostic accessibility API providing:
//! - Type-safe element and window identifiers
//! - Element lifecycle management
//! - AXObserver-based change notifications
//! - Clean public API
//!
//! # Example
//!
//! ```ignore
//! use axio_core::{api, ElementId};
//!
//! // Get element at screen position
//! let element = api::element_at(100.0, 200.0)?;
//!
//! // Watch for changes
//! api::watch(&element.id)?;
//!
//! // Write to text field
//! api::write(&element.id, "Hello, world!")?;
//! ```

// Core types
mod types;
pub use types::*;

// Internal modules (pub for now, may be made private later)
pub mod element_registry;
mod ui_element;
pub mod window_manager;

// Platform-specific implementations
pub mod platform;

// Public API
pub mod api;

// Re-export commonly used items at crate root
pub use api::{click, element_at, tree, unwatch, watch, write};
