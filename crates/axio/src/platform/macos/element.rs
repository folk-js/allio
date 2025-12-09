/*!
Element operations for macOS accessibility.

Handles:
- Hash-based deduplication
*/

use objc2_core_foundation::CFHash;

use super::handles::ElementHandle;

/// Get hash for element handle (for O(1) dedup lookup).
pub(crate) fn element_hash(handle: &ElementHandle) -> u64 {
  CFHash(Some(handle.inner())) as u64
}

// Note: Element building, children, parent, refresh have moved to core/element_ops.rs
// Click and write operations use PlatformHandle traits directly.
