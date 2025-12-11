/*!
Freshness specifies how up-to-date a value should be when retrieved.

This is the core concept for the "honest API" - making staleness explicit
rather than hiding it behind get/fetch naming conventions.
*/

use std::time::Duration;

/// How fresh a value should be when retrieved.
///
/// # Examples
///
/// ```ignore
/// // Get from cache, might be stale
/// let elem = axio.get(id, Freshness::Cached)?;
///
/// // Always fetch from OS
/// let elem = axio.get(id, Freshness::Fresh)?;
///
/// // Fetch if older than 100ms
/// let elem = axio.get(id, Freshness::max_age_ms(100))?;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
  /// Use cached value. No OS calls. Might be arbitrarily stale.
  ///
  /// Use for: bulk reads, non-critical data, when you know the cache is fresh.
  Cached,

  /// Always fetch from OS. Guaranteed current.
  ///
  /// Use for: hit testing, discovery, when you need current truth.
  Fresh,

  /// Value must be at most this old. Fetch from OS if stale.
  ///
  /// Use for: observed elements, when you can tolerate bounded staleness.
  MaxAge(Duration),
}

impl Freshness {
  /// Convenience constructor for max age in milliseconds.
  #[inline]
  pub const fn max_age_ms(ms: u64) -> Self {
    Self::MaxAge(Duration::from_millis(ms))
  }

  /// Convenience constructor for max age in seconds.
  #[inline]
  pub const fn max_age_secs(secs: u64) -> Self {
    Self::MaxAge(Duration::from_secs(secs))
  }

  /// Check if a value with the given age satisfies this freshness requirement.
  ///
  /// Returns `true` if the value is fresh enough, `false` if it needs refresh.
  #[inline]
  pub fn is_satisfied_by(&self, age: Duration) -> bool {
    match self {
      Freshness::Cached => true, // Any age is fine
      Freshness::Fresh => false, // Always needs refresh
      Freshness::MaxAge(max) => age <= *max,
    }
  }

  /// Whether this freshness level requires an OS call.
  #[inline]
  pub const fn requires_fetch(&self) -> bool {
    matches!(self, Freshness::Fresh)
  }

  /// Whether this freshness level might require an OS call (depends on age).
  #[inline]
  pub const fn might_require_fetch(&self) -> bool {
    !matches!(self, Freshness::Cached)
  }
}

impl Default for Freshness {
  /// Default freshness is `Cached` - fast, might be stale.
  fn default() -> Self {
    Self::Cached
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cached_accepts_any_age() {
    let freshness = Freshness::Cached;
    assert!(freshness.is_satisfied_by(Duration::ZERO));
    assert!(freshness.is_satisfied_by(Duration::from_secs(1000)));
  }

  #[test]
  fn fresh_rejects_any_age() {
    let freshness = Freshness::Fresh;
    assert!(!freshness.is_satisfied_by(Duration::ZERO));
    assert!(!freshness.is_satisfied_by(Duration::from_secs(1)));
  }

  #[test]
  fn max_age_checks_duration() {
    let freshness = Freshness::max_age_ms(100);
    assert!(freshness.is_satisfied_by(Duration::from_millis(50)));
    assert!(freshness.is_satisfied_by(Duration::from_millis(100)));
    assert!(!freshness.is_satisfied_by(Duration::from_millis(101)));
  }

  #[test]
  fn default_is_cached() {
    assert_eq!(Freshness::default(), Freshness::Cached);
  }
}
