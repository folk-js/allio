/**
 * Element Registry - Reference-based AX Element Management
 *
 * Replaces path-based navigation with a stable reference system.
 * Maps unique IDs to AXUIElement pointers for stable node identity.
 */
use accessibility::AXUIElement;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use uuid::Uuid;

/// Global registry mapping element IDs to AXUIElement references
/// Note: AXUIElement doesn't implement Send/Sync directly, but operations are thread-safe
/// when protected by Mutex
static ELEMENT_REGISTRY: Lazy<Mutex<ElementRegistry>> =
    Lazy::new(|| Mutex::new(ElementRegistry::new()));

pub struct ElementRegistry {
    /// Map of element ID -> (AXUIElement, PID)
    /// PID is stored for internal backend operations (watch/unwatch)
    /// This will be removed in Phase 3.1 when windows own their elements
    elements: HashMap<String, (AXUIElement, u32)>,
}

// Manual implementation - AXUIElement is actually thread-safe behind Mutex
unsafe impl Send for ElementRegistry {}
unsafe impl Sync for ElementRegistry {}

impl ElementRegistry {
    fn new() -> Self {
        Self {
            elements: HashMap::new(),
        }
    }

    /// Register an element with its PID and return a unique ID for it
    /// PID is stored for internal backend operations (watch/unwatch need it for AXObserver)
    pub fn register(element: AXUIElement, pid: u32) -> String {
        let id = Uuid::new_v4().to_string();
        let mut registry = ELEMENT_REGISTRY.lock().unwrap();
        registry.elements.insert(id.clone(), (element, pid));
        id
    }

    /// Get an element by its ID
    pub fn get(id: &str) -> Option<AXUIElement> {
        let registry = ELEMENT_REGISTRY.lock().unwrap();
        registry
            .elements
            .get(id)
            .map(|(element, _pid)| element.clone())
    }

    /// Get the PID associated with an element ID
    /// Used internally by watch/unwatch operations
    pub fn get_pid(id: &str) -> Option<u32> {
        let registry = ELEMENT_REGISTRY.lock().unwrap();
        registry.elements.get(id).map(|(_element, pid)| *pid)
    }
}
