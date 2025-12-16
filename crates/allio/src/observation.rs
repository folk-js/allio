/*!
Subtree observation system.

Keeps observed subtrees fresh via background polling on a separate thread.
Each observed subtree is swept recursively, and changes emit both element-level
events (for granular tracking) and a single `subtree:changed` event (for easy re-querying).

## Architecture

- Observation state lives in `Allio` (shared via `Arc`)
- A dedicated thread checks observed subtrees periodically
- Sweeps are executed on a rayon thread pool (bounded concurrency)
- Each sweep runs to completion (no partial work)
- Timing uses "wait after completion" model (not fixed interval)

## Usage

```ignore
let handle = allio.observe(element_id, ObserveConfig { depth: Some(3), ..default() })?;
// Subtree is now polled automatically
// Listen to "subtree:changed" events

handle.dispose(); // Or let it drop
```
*/

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::core::Allio;
use crate::platform::{Handle, PlatformHandle};
use crate::types::{ElementId, Event, ProcessId, WindowId};

/// Default wait time between sweeps (after completion).
const DEFAULT_WAIT_BETWEEN_MS: u64 = 100;

/// How often the observation thread checks if any subtrees need sweeping.
const CHECK_INTERVAL_MS: u64 = 10;

/// Configuration for observing a subtree.
#[derive(Debug, Clone, Copy)]
pub struct ObserveConfig {
  /// Maximum depth to traverse. None = infinite.
  pub depth: Option<usize>,
  /// Wait time after sweep completes before starting next. Default: 100ms.
  pub wait_between: Option<Duration>,
}

impl Default for ObserveConfig {
  fn default() -> Self {
    Self {
      depth: None,
      wait_between: Some(Duration::from_millis(DEFAULT_WAIT_BETWEEN_MS)),
    }
  }
}

/// Internal state for an observed subtree.
pub(crate) struct ObservedSubtree {
  pub(crate) root_id: ElementId,
  pub(crate) depth: Option<usize>,
  pub(crate) wait_between: Duration,

  /// Prevents overlapping sweeps.
  pub(crate) in_progress: AtomicBool,
  /// When the last sweep completed.
  pub(crate) last_completed: Mutex<Instant>,

  /// Changes accumulated during current sweep cycle.
  pub(crate) changes: Mutex<SweepChanges>,
}

/// Changes detected during a single sweep cycle.
#[derive(Debug, Default)]
pub(crate) struct SweepChanges {
  pub(crate) added: Vec<ElementId>,
  pub(crate) removed: Vec<ElementId>,
  pub(crate) modified: Vec<ElementId>,
}

impl SweepChanges {
  fn is_empty(&self) -> bool {
    self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
  }

  fn clear(&mut self) {
    self.added.clear();
    self.removed.clear();
    self.modified.clear();
  }
}

/// Handle to an observed subtree. Stops observation on drop.
#[derive(Debug)]
pub struct ObservationHandle {
  root_id: ElementId,
  allio: Allio,
}

impl ObservationHandle {
  /// Stop observing this subtree.
  pub fn dispose(self) {
    // Drop will handle cleanup
  }
}

impl Drop for ObservationHandle {
  fn drop(&mut self) {
    self.allio.unobserve(self.root_id);
  }
}

/// Shared state for all observations.
pub(crate) struct ObservationState {
  /// Map of observed subtrees by root element ID.
  pub(crate) subtrees: Mutex<HashMap<ElementId, Arc<ObservedSubtree>>>,
}

impl ObservationState {
  pub(crate) fn new() -> Self {
    Self {
      subtrees: Mutex::new(HashMap::new()),
    }
  }
}

/// Handle to the observation thread. Stops on drop.
pub(crate) struct ObservationThreadHandle {
  stop_signal: Arc<AtomicBool>,
  thread: Option<JoinHandle<()>>,
}

impl Drop for ObservationThreadHandle {
  fn drop(&mut self) {
    self.stop_signal.store(true, Ordering::SeqCst);
    if let Some(t) = self.thread.take() {
      drop(t.join());
    }
  }
}

/// Start the observation thread.
pub(crate) fn start_observation_thread(allio: Allio) -> ObservationThreadHandle {
  let stop_signal = Arc::new(AtomicBool::new(false));
  let stop_signal_clone = Arc::clone(&stop_signal);

  let thread = thread::spawn(move || {
    observation_loop(allio, &stop_signal_clone);
  });

  ObservationThreadHandle {
    stop_signal,
    thread: Some(thread),
  }
}

/// Main observation loop - checks subtrees and spawns sweeps.
fn observation_loop(allio: Allio, stop_signal: &AtomicBool) {
  let pool = rayon::ThreadPoolBuilder::new()
    .num_threads(4)
    .thread_name(|i| format!("allio-sweep-{i}"))
    .build()
    .expect("Failed to create rayon thread pool");

  while !stop_signal.load(Ordering::SeqCst) {
    thread::sleep(Duration::from_millis(CHECK_INTERVAL_MS));

    // Get all observed subtrees
    let subtrees: Vec<Arc<ObservedSubtree>> = allio
      .observation_state()
      .subtrees
      .lock()
      .values()
      .cloned()
      .collect();

    for subtree in subtrees {
      // Skip if already sweeping
      if subtree.in_progress.load(Ordering::SeqCst) {
        continue;
      }

      // Skip if not enough time has passed since last completion
      let elapsed = subtree.last_completed.lock().elapsed();
      if elapsed < subtree.wait_between {
        continue;
      }

      // Mark as in-progress and spawn sweep
      subtree.in_progress.store(true, Ordering::SeqCst);

      let allio_clone = allio.clone();
      let subtree_clone = Arc::clone(&subtree);

      pool.spawn(move || {
        sweep_subtree(&allio_clone, &subtree_clone);
      });
    }
  }
}

/// Sweep an observed subtree recursively.
fn sweep_subtree(allio: &Allio, obs: &ObservedSubtree) {
  let start = Instant::now();

  // Clear changes from previous sweep
  obs.changes.lock().clear();

  // Get root element info from cache
  let root_info = allio.read(|r| {
    r.element(obs.root_id).map(|e| {
      (
        e.handle.clone(),
        e.window_id,
        e.pid,
        r.tree_children(obs.root_id).to_vec(),
      )
    })
  });

  let Some((root_handle, window_id, pid, _cached_children)) = root_info else {
    // Root element not in cache - nothing to sweep
    obs.in_progress.store(false, Ordering::SeqCst);
    *obs.last_completed.lock() = Instant::now();
    return;
  };

  // Sweep recursively starting from root
  sweep_element_recursive(allio, obs, obs.root_id, &root_handle, window_id, pid, 0);

  // Emit subtree:changed event if anything changed
  let changes = obs.changes.lock();
  if !changes.is_empty() {
    let event = Event::SubtreeChanged {
      root_id: obs.root_id,
      added: changes.added.clone(),
      removed: changes.removed.clone(),
      modified: changes.modified.clone(),
    };
    allio.emit_event(event);
  }

  log::debug!(
    "Swept subtree {} in {}ms (added={}, removed={}, modified={})",
    obs.root_id,
    start.elapsed().as_millis(),
    changes.added.len(),
    changes.removed.len(),
    changes.modified.len(),
  );

  // Mark complete
  obs.in_progress.store(false, Ordering::SeqCst);
  *obs.last_completed.lock() = Instant::now();
}

/// Recursively sweep a single element and its descendants.
fn sweep_element_recursive(
  allio: &Allio,
  obs: &ObservedSubtree,
  element_id: ElementId,
  handle: &Handle,
  window_id: WindowId,
  pid: ProcessId,
  depth: usize,
) {
  // Check depth limit
  if let Some(max_depth) = obs.depth {
    if depth >= max_depth {
      return;
    }
  }

  // Fetch current attributes from OS
  let attrs = handle.fetch_attributes();

  // Check if element is dead (no role = invalid element)
  if attrs.role == crate::a11y::Role::Unknown && attrs.platform_role.is_empty() {
    // Element is dead - remove from cache
    allio.write(|r| r.remove_element(element_id));
    obs.changes.lock().removed.push(element_id);
    return;
  }

  // Compare with cached state and update
  let changed = allio
    .write(|r| r.refresh_element(element_id, attrs))
    .unwrap_or(false);

  if changed {
    obs.changes.lock().modified.push(element_id);
  }

  // Fetch children from OS
  let child_handles = handle.fetch_children();

  // Get currently cached children
  let cached_children: HashSet<ElementId> =
    allio.read(|r| r.tree_children(element_id).iter().copied().collect());

  // Process current children
  let mut current_children: Vec<ElementId> = Vec::with_capacity(child_handles.len());

  for child_handle in child_handles {
    // Check if child already exists in cache
    let child_id = allio.read(|r| r.find_element(&child_handle));

    let child_id = if let Some(existing_id) = child_id {
      existing_id
    } else {
      // New child discovered - add to cache
      let entry =
        crate::core::adapters::build_entry_from_handle(child_handle.clone(), window_id, pid);
      let new_id = allio.write(|r| r.upsert_element(entry));
      obs.changes.lock().added.push(new_id);
      new_id
    };

    current_children.push(child_id);

    // Recurse into child
    sweep_element_recursive(
      allio,
      obs,
      child_id,
      &child_handle,
      window_id,
      pid,
      depth + 1,
    );
  }

  // Detect removed children
  let current_set: HashSet<_> = current_children.iter().copied().collect();
  for removed_id in cached_children.difference(&current_set) {
    allio.write(|r| r.remove_element(*removed_id));
    obs.changes.lock().removed.push(*removed_id);
  }

  // Update tree structure if children changed
  if cached_children != current_set {
    allio.write(|r| r.set_children(element_id, current_children));
  }
}

impl Allio {
  /// Observe a subtree for changes.
  ///
  /// The subtree will be polled periodically (default: every 100ms after each sweep completes).
  /// Changes emit both element-level events and a single `subtree:changed` event per cycle.
  ///
  /// Returns a handle that stops observation when dropped.
  pub fn observe(
    &self,
    root_id: ElementId,
    config: ObserveConfig,
  ) -> crate::types::AllioResult<ObservationHandle> {
    // Verify element exists
    if !self.read(|r| r.element(root_id).is_some()) {
      return Err(crate::types::AllioError::ElementNotFound(root_id));
    }

    let subtree = Arc::new(ObservedSubtree {
      root_id,
      depth: config.depth,
      wait_between: config
        .wait_between
        .unwrap_or(Duration::from_millis(DEFAULT_WAIT_BETWEEN_MS)),
      in_progress: AtomicBool::new(false),
      last_completed: Mutex::new(Instant::now() - Duration::from_secs(1)), // Trigger immediate first sweep
      changes: Mutex::new(SweepChanges::default()),
    });

    self
      .observation_state()
      .subtrees
      .lock()
      .insert(root_id, subtree);

    log::debug!(
      "Started observing subtree {} (depth: {:?})",
      root_id,
      config.depth
    );

    Ok(ObservationHandle {
      root_id,
      allio: self.clone(),
    })
  }

  /// Stop observing a subtree.
  pub fn unobserve(&self, root_id: ElementId) {
    self.observation_state().subtrees.lock().remove(&root_id);
    log::debug!("Stopped observing subtree {}", root_id);
  }

  /// Check if a subtree is being observed.
  pub fn is_observed(&self, root_id: ElementId) -> bool {
    self
      .observation_state()
      .subtrees
      .lock()
      .contains_key(&root_id)
  }
}
