//! Cross-platform accessibility abstractions.
//!
//! These types are decoupled from platform-specific implementations and represent
//! the semantic model of UI accessibility across macOS, Windows, and Linux.

mod action;
mod notification;
mod role;
mod value;

pub use action::Action;
pub use notification::Notification;
pub use role::{Role, WritableAs};
pub use value::Value;
