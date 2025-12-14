/*!
Recency specifies how up-to-date a value should be when retrieved.

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
/// let elem = allio.get(id, Recency::Any)?;
///
/// // Always fetch from OS
/// let elem = allio.get(id, Recency::Current)?;
///
/// // Fetch if older than 100ms
/// let elem = allio.get(id, Recency::max_age_ms(100))?;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recency {
  /// Use cached value. No OS calls. Might be arbitrarily stale.
  ///
  /// Use for: bulk reads, non-critical data, when you know the cache is fresh.
  Any,

  /// Always fetch from OS. Guaranteed current.
  ///
  /// Use for: hit testing, discovery, when you need current truth.
  Current,

  /// Value must be at most this old. Fetch from OS if stale.
  ///
  /// Use for: observed elements, when you can tolerate bounded staleness.
  MaxAge(Duration),
}

impl Recency {
  /// Convenience constructor for max age in milliseconds.
  ///
  /// # Example
  ///
  /// ```
  /// use allio::Recency;
  /// use std::time::Duration;
  ///
  /// let recency = Recency::max_age_ms(100);
  /// assert!(recency.is_satisfied_by(Duration::from_millis(50)));
  /// assert!(!recency.is_satisfied_by(Duration::from_millis(150)));
  /// ```
  #[inline]
  pub const fn max_age_ms(ms: u32) -> Self {
    Self::MaxAge(Duration::from_millis(ms as u64))
  }

  /// Convenience constructor for max age in seconds.
  ///
  /// # Example
  ///
  /// ```
  /// use allio::Recency;
  /// use std::time::Duration;
  ///
  /// let recency = Recency::max_age_secs(5);
  /// assert!(recency.is_satisfied_by(Duration::from_secs(3)));
  /// ```
  #[inline]
  pub const fn max_age_secs(secs: u32) -> Self {
    Self::MaxAge(Duration::from_secs(secs as u64))
  }

  /// Check if a value with the given age satisfies this recency requirement.
  ///
  /// Returns `true` if the value is fresh enough, `false` if it needs refresh.
  ///
  /// # Example
  ///
  /// ```
  /// use allio::Recency;
  /// use std::time::Duration;
  ///
  /// // Any accepts any age
  /// assert!(Recency::Any.is_satisfied_by(Duration::from_secs(1000)));
  ///
  /// // Current never accepts any age
  /// assert!(!Recency::Current.is_satisfied_by(Duration::ZERO));
  ///
  /// // MaxAge checks the duration
  /// let recency = Recency::max_age_ms(100);
  /// assert!(recency.is_satisfied_by(Duration::from_millis(100)));
  /// assert!(!recency.is_satisfied_by(Duration::from_millis(101)));
  /// ```
  #[inline]
  pub fn is_satisfied_by(&self, age: Duration) -> bool {
    match self {
      Recency::Any => true,      // Any age is fine
      Recency::Current => false, // Always needs refresh
      Recency::MaxAge(max) => age <= *max,
    }
  }

  /// Whether this recency level requires an OS call.
  ///
  /// # Example
  ///
  /// ```
  /// use allio::Recency;
  ///
  /// assert!(!Recency::Any.requires_fetch());
  /// assert!(Recency::Current.requires_fetch());
  /// assert!(!Recency::max_age_ms(100).requires_fetch());
  /// ```
  #[inline]
  pub const fn requires_fetch(&self) -> bool {
    matches!(self, Recency::Current)
  }

  /// Whether this recency level might require an OS call (depends on age).
  ///
  /// # Example
  ///
  /// ```
  /// use allio::Recency;
  ///
  /// assert!(!Recency::Any.might_require_fetch());
  /// assert!(Recency::Current.might_require_fetch());
  /// assert!(Recency::max_age_ms(100).might_require_fetch());
  /// ```
  #[inline]
  pub const fn might_require_fetch(&self) -> bool {
    !matches!(self, Recency::Any)
  }
}

impl Default for Recency {
  /// Default recency is `Cached` - fast, might be stale.
  fn default() -> Self {
    Self::Any
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn cached_accepts_any_age() {
    let recency = Recency::Any;
    assert!(recency.is_satisfied_by(Duration::ZERO));
    assert!(recency.is_satisfied_by(Duration::from_secs(1000)));
  }

  #[test]
  fn fresh_rejects_any_age() {
    let recency = Recency::Current;
    assert!(!recency.is_satisfied_by(Duration::ZERO));
    assert!(!recency.is_satisfied_by(Duration::from_secs(1)));
  }

  #[test]
  fn max_age_checks_duration() {
    let recency = Recency::max_age_ms(100);
    assert!(recency.is_satisfied_by(Duration::from_millis(50)));
    assert!(recency.is_satisfied_by(Duration::from_millis(100)));
    assert!(!recency.is_satisfied_by(Duration::from_millis(101)));
  }

  #[test]
  fn default_is_cached() {
    assert_eq!(Recency::default(), Recency::Any);
  }
}
