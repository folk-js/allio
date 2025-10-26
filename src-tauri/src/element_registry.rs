/**
 * Element Registry - Lifecycle Management for UI Elements
 *
 * Manages the lifecycle of UIElement instances, including:
 * - Registration and lookup
 * - Window-to-element association
 * - AXObserver management for watching
 * - Automatic cleanup when windows close
 */
use accessibility_sys::{AXObserverCreate, AXObserverGetRunLoopSource, AXObserverRef};
use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::ui_element::UIElement;

/// Check if two AXUIElements refer to the same UI element using CFEqual
fn ax_elements_equal(
    elem1: &accessibility::AXUIElement,
    elem2: &accessibility::AXUIElement,
) -> bool {
    use accessibility_sys::AXUIElementRef;
    use core_foundation::base::{CFEqual, TCFType};

    let ref1 = elem1.as_concrete_TypeRef() as AXUIElementRef;
    let ref2 = elem2.as_concrete_TypeRef() as AXUIElementRef;

    unsafe { CFEqual(ref1 as _, ref2 as _) != 0 }
}

/// Global registry managing all UI elements
/// Note: This is a global singleton for now, but could be moved to app state
static ELEMENT_REGISTRY: Lazy<Mutex<Option<ElementRegistry>>> = Lazy::new(|| Mutex::new(None));

pub struct ElementRegistry {
    // ============================================================================
    // Primary Storage
    // ============================================================================
    /// Map of element_id -> UIElement
    elements: HashMap<String, UIElement>,

    // ============================================================================
    // Indices for Fast Lookup
    // ============================================================================
    /// Map of window_id -> Set<element_id>
    /// Used to find all elements in a window and for cleanup
    window_to_elements: HashMap<String, HashSet<String>>,

    // ============================================================================
    // AXObservers for Watching
    // ============================================================================
    /// Map of window_id -> AXObserverRef
    /// One observer per window (keyed by window_id, not PID, for consistency)
    observers: HashMap<String, AXObserverRef>,

    /// Broadcast sender for notifications (shared across all watches)
    sender: Arc<broadcast::Sender<String>>,
}

// Manual implementation - operations are thread-safe behind Mutex
unsafe impl Send for ElementRegistry {}
unsafe impl Sync for ElementRegistry {}

impl ElementRegistry {
    /// Initialize the global registry
    /// Must be called once at startup with the broadcast sender
    pub fn initialize(sender: Arc<broadcast::Sender<String>>) {
        let mut registry = ELEMENT_REGISTRY.lock().unwrap();
        *registry = Some(ElementRegistry {
            elements: HashMap::new(),
            window_to_elements: HashMap::new(),
            observers: HashMap::new(),
            sender,
        });
    }

    /// Get a reference to the global registry
    fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ElementRegistry) -> R,
    {
        let mut guard = ELEMENT_REGISTRY.lock().unwrap();
        let registry = guard.as_mut().expect("ElementRegistry not initialized");
        f(registry)
    }

    // ============================================================================
    // Registration
    // ============================================================================

    /// Register a new element and return its unique ID
    ///
    /// If an equivalent element already exists for this window (determined via CFEqual),
    /// returns the existing element's ID instead of creating a new one.
    /// This ensures stable IDs across multiple tree queries.
    ///
    /// Called by platform/macos.rs during tree building.
    pub fn register(
        ax_element: accessibility::AXUIElement,
        window_id: String,
        pid: u32,
        parent_id: Option<String>,
        role: String,
    ) -> String {
        Self::with(|registry| {
            // Check if an equivalent element already exists for this window
            if let Some(window_elements) = registry.window_to_elements.get(&window_id) {
                for element_id in window_elements {
                    if let Some(existing) = registry.elements.get(element_id) {
                        if ax_elements_equal(existing.ax_element(), &ax_element) {
                            println!("🔄 Reusing existing element ID: {}", element_id);
                            return element_id.clone();
                        }
                    }
                }
            }

            // No equivalent element found - create new one
            let id = Uuid::new_v4().to_string();

            let ui_element = UIElement::new(
                id.clone(),
                window_id.clone(),
                parent_id,
                ax_element,
                pid,
                role,
            );

            // Store element
            registry.elements.insert(id.clone(), ui_element);

            // Update window-to-elements index
            registry
                .window_to_elements
                .entry(window_id)
                .or_insert_with(HashSet::new)
                .insert(id.clone());

            println!("✨ Created new element ID: {}", id);
            id
        })
    }

    // ============================================================================
    // Lookup
    // ============================================================================

    /// Get an element by its ID (immutable)
    pub fn get(element_id: &str) -> Option<String> {
        // Returns element_id if it exists (for checking existence)
        Self::with(|registry| {
            if registry.elements.contains_key(element_id) {
                Some(element_id.to_string())
            } else {
                None
            }
        })
    }

    /// Execute an operation with an element (immutable access)
    pub fn with_element<F, R>(element_id: &str, f: F) -> Result<R, String>
    where
        F: FnOnce(&UIElement) -> R,
    {
        Self::with(|registry| {
            registry
                .elements
                .get(element_id)
                .map(f)
                .ok_or_else(|| format!("Element {} not found", element_id))
        })
    }

    /// Execute an operation with an element (mutable access)
    pub fn with_element_mut<F, R>(element_id: &str, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut UIElement) -> R,
    {
        Self::with(|registry| {
            registry
                .elements
                .get_mut(element_id)
                .map(f)
                .ok_or_else(|| format!("Element {} not found", element_id))
        })
    }

    /// Find all elements belonging to a window
    pub fn find_by_window(window_id: &str) -> Vec<String> {
        Self::with(|registry| {
            registry
                .window_to_elements
                .get(window_id)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default()
        })
    }

    /// Check if an element exists
    pub fn contains(element_id: &str) -> bool {
        Self::with(|registry| registry.elements.contains_key(element_id))
    }

    // ============================================================================
    // Lifecycle - THE KEY FEATURE!
    // ============================================================================

    /// Remove a single element (e.g., when it's destroyed)
    pub fn remove_element(element_id: &str) {
        Self::with(|registry| {
            if let Some(element) = registry.elements.remove(element_id) {
                let window_id = element.window_id().to_string();

                // Unwatch if observer exists
                if let Some(&observer) = registry.observers.get(&window_id) {
                    let mut elem = element;
                    elem.unwatch(observer);
                }

                // Remove from window index
                if let Some(window_elements) = registry.window_to_elements.get_mut(&window_id) {
                    window_elements.remove(element_id);
                }

                println!("🗑️  Removed destroyed element: {}", element_id);
            }
        });
    }

    /// Remove all elements associated with a window
    ///
    /// Called when a window closes. This ensures:
    /// - Elements are unwatched (notifications removed)
    /// - Observer is cleaned up
    /// - Memory is freed
    pub fn remove_window_elements(window_id: &str) {
        Self::with(|registry| {
            // Get all element IDs for this window
            if let Some(element_ids) = registry.window_to_elements.remove(window_id) {
                // Get the observer for this window (if exists)
                let observer = registry.observers.get(window_id).copied();

                // Unwatch and remove each element
                for element_id in element_ids {
                    if let Some(mut element) = registry.elements.remove(&element_id) {
                        // Unwatch if observer exists
                        if let Some(obs) = observer {
                            element.unwatch(obs);
                        }
                    }
                }

                // Remove observer
                registry.observers.remove(window_id);

                println!("🗑️  Cleaned up elements for window {}", window_id);
            }
        });
    }

    // ============================================================================
    // Operations (Delegate to UIElement)
    // ============================================================================

    /// Write text to an element
    pub fn write(element_id: &str, text: &str) -> Result<(), String> {
        Self::with_element(element_id, |element| element.set_value(text))?
    }

    /// Watch an element for changes
    pub fn watch(element_id: &str) -> Result<(), String> {
        Self::with(|registry| {
            let element = registry
                .elements
                .get(element_id)
                .ok_or_else(|| format!("Element {} not found", element_id))?;

            let window_id = element.window_id().to_string();
            let pid = element.pid();

            // Get or create observer for this window
            let observer = if let Some(&obs) = registry.observers.get(&window_id) {
                obs
            } else {
                // Create new observer
                let mut observer_ref: AXObserverRef = std::ptr::null_mut();

                let result = unsafe {
                    AXObserverCreate(
                        pid as i32,
                        observer_callback as _,
                        &mut observer_ref as *mut _,
                    )
                };

                if result != 0 {
                    return Err(format!("Failed to create observer: error code {}", result));
                }

                // Add observer to the MAIN run loop
                unsafe {
                    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
                    if run_loop_source_ref.is_null() {
                        return Err("Failed to get run loop source from observer".to_string());
                    }

                    let run_loop_source =
                        CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);

                    let main_run_loop = CFRunLoop::get_main();
                    main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);
                }

                registry.observers.insert(window_id.clone(), observer_ref);
                observer_ref
            };

            // Watch the element
            let element = registry
                .elements
                .get_mut(element_id)
                .ok_or_else(|| format!("Element {} not found", element_id))?;

            element.watch(observer, registry.sender.clone())
        })
    }

    /// Unwatch an element
    pub fn unwatch(element_id: &str) {
        Self::with(|registry| {
            if let Some(element) = registry.elements.get(element_id) {
                let window_id = element.window_id();
                if let Some(&observer) = registry.observers.get(window_id) {
                    if let Some(element) = registry.elements.get_mut(element_id) {
                        element.unwatch(observer);
                    }
                }
            }
        });
    }

    /// Get children of an element (builds tree)
    pub fn get_children(
        element_id: &str,
        max_depth: usize,
        max_children: usize,
    ) -> Result<Vec<crate::axio::AXNode>, String> {
        Self::with_element(element_id, |_element| {
            crate::platform::macos::get_children_by_element_id(element_id, max_depth, max_children)
        })?
    }
}

/// C callback for AXObserver notifications
/// This is called by macOS when an element changes
unsafe extern "C" fn observer_callback(
    _observer: AXObserverRef,
    _element: accessibility_sys::AXUIElementRef,
    notification: core_foundation::string::CFStringRef,
    refcon: *mut std::ffi::c_void,
) {
    use accessibility::AXUIElement;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    if refcon.is_null() {
        return;
    }

    // Extract context (defined in ui_element.rs watch())
    #[derive(Clone)]
    #[repr(C)]
    struct ObserverContext {
        element_id: String,
        sender: Arc<broadcast::Sender<String>>,
    }

    let context = &*(refcon as *const ObserverContext);
    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();

    // Convert element to AXUIElement
    let changed_element = AXUIElement::wrap_under_get_rule(_element);

    // Handle the notification
    handle_notification(
        &context.element_id,
        &notification_name,
        &changed_element,
        &context.sender,
    );
}

/// Handle a notification by extracting data and broadcasting typed updates
fn handle_notification(
    element_id: &str,
    notification: &str,
    element: &accessibility::AXUIElement,
    sender: &Arc<broadcast::Sender<String>>,
) {
    use crate::protocol::{ElementUpdate, ServerMessage};
    use accessibility::AXAttribute;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    // Convert notification to typed update
    let update = match notification {
        "AXValueChanged" => {
            // Extract value
            if let Ok(value_attr) = element.attribute(&AXAttribute::value()) {
                let role = element
                    .attribute(&AXAttribute::role())
                    .ok()
                    .and_then(|r| unsafe {
                        let cf_string = CFString::wrap_under_get_rule(r.as_CFTypeRef() as *const _);
                        Some(cf_string.to_string())
                    });

                if let Some(value) =
                    crate::platform::macos::extract_value(&value_attr, role.as_deref())
                {
                    Some(ElementUpdate::ValueChanged {
                        element_id: element_id.to_string(),
                        value,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }

        "AXTitleChanged" => {
            // Extract label (ARIA term for title/label)
            if let Ok(label_attr) = element.attribute(&AXAttribute::title()) {
                let label = label_attr.to_string();
                if !label.is_empty() {
                    Some(ElementUpdate::LabelChanged {
                        element_id: element_id.to_string(),
                        label,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        }

        "AXUIElementDestroyed" => {
            // Element destroyed - remove from registry
            ElementRegistry::remove_element(element_id);

            Some(ElementUpdate::ElementDestroyed {
                element_id: element_id.to_string(),
            })
        }

        _ => {
            // Unhandled notification type
            println!("⚠️  Unhandled notification: {}", notification);
            None
        }
    };

    // Broadcast typed update
    if let Some(update) = update {
        let message = ServerMessage::ElementUpdate { update };

        if let Ok(json_str) = serde_json::to_string(&message) {
            let _ = sender.send(json_str);
        }
    }
}
