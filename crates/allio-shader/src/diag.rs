//! Logging for failures on per-frame paths, which would otherwise repeat at the frame rate.

use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::fmt::Display;

static SEEN: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());

/// Logs a warning the first time `site` fails.
pub(crate) fn warn_once(site: &'static str, detail: impl Display) {
  if SEEN.lock().insert(site) {
    log::warn!("{site}: {detail}");
  }
}
