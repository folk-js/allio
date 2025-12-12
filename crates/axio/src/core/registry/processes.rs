/*!
Process operations for the Registry.

CRUD: upsert_process, remove_process (no update needed)
Query: process, has_process
*/

use super::{ProcessEntry, Registry};
use crate::types::ProcessId;

// ============================================================================
// Process CRUD
// ============================================================================

impl Registry {
  /// Insert a process if it doesn't exist.
  ///
  /// Returns the process ID (whether newly inserted or already present).
  /// This handles the TOCTOU race where another thread may have inserted first.
  pub(crate) fn upsert_process(&mut self, id: ProcessId, entry: ProcessEntry) -> ProcessId {
    use std::collections::hash_map::Entry;
    match self.processes.entry(id) {
      Entry::Occupied(_) => {} // Already exists, no-op
      Entry::Vacant(e) => {
        e.insert(entry);
      }
    }
    id
  }

  /// Remove a process.
  pub(crate) fn remove_process(&mut self, id: ProcessId) {
    self.processes.remove(&id);
  }
}

// ============================================================================
// Process Queries
// ============================================================================

impl Registry {
  /// Get process entry by ID.
  pub(crate) fn process(&self, id: ProcessId) -> Option<&ProcessEntry> {
    self.processes.get(&id)
  }

  /// Check if process exists.
  pub(crate) fn has_process(&self, id: ProcessId) -> bool {
    self.processes.contains_key(&id)
  }
}
