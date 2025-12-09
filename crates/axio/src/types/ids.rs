/*! Branded ID types for type-safe entity references. */

use derive_more::{Display, From, Into};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use ts_rs::TS;

/// Window identifier.
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Display, From, Into,
)]
#[ts(export)]
pub struct WindowId(pub u32);

/// Element identifier.
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Display, From, Into,
)]
#[ts(export)]
pub struct ElementId(pub u32);

/// Global counter for `ElementId` generation. Starts at 1 (0 could be confused with "null").
static ELEMENT_COUNTER: AtomicU32 = AtomicU32::new(1);

impl ElementId {
  /// Generate a new unique `ElementId`.
  pub fn new() -> Self {
    Self(ELEMENT_COUNTER.fetch_add(1, Ordering::Relaxed))
  }
}

impl Default for ElementId {
  fn default() -> Self {
    Self::new()
  }
}

/// Process ID - branded type to distinguish from other u32 values.
#[derive(
  Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Display, From, Into,
)]
#[ts(export)]
pub struct ProcessId(pub u32);

