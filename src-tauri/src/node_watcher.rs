/**
 * Node Watcher - Event-Driven Accessibility Notifications
 *
 * Uses macOS AXObserver API to receive real-time notifications when nodes change.
 * Keeps frontend handles "live" - when you hold a node reference in JS, it auto-updates.
 *
 * Implementation uses raw C FFI via accessibility-sys for maximum compatibility.
 */
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

use accessibility::AXUIElement;
use accessibility_sys::{
    kAXMovedNotification, kAXResizedNotification, kAXUIElementDestroyedNotification,
    kAXValueChangedNotification, AXObserverAddNotification, AXObserverCreate,
    AXObserverGetRunLoopSource, AXObserverRef, AXObserverRemoveNotification, AXUIElementRef,
};
use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};
use core_foundation::string::{CFString, CFStringRef};

use tokio::sync::broadcast;

use crate::axio::{AXValue, Bounds};

/// Unique identifier for a node
/// Phase 3: Now supports element_id (preferred) or path (legacy)
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeIdentifier {
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>, // NEW: Preferred (Phase 3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<usize>>, // LEGACY: Fallback
}

/// Context passed to observer callbacks
struct ObserverContext {
    node_id: NodeIdentifier,
    sender: Arc<broadcast::Sender<String>>,
}

/// Delta update for a node (only changed fields included)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUpdate {
    pub id: String, // Stable element_id for frontend to match
    pub pid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub element_id: Option<String>, // NEW: Element ID from registry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<usize>>, // LEGACY: Deprecated
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<AXValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub focused: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Shared state for node watching
pub struct NodeWatcher {
    observers: Arc<Mutex<HashMap<u32, AXObserverRef>>>, // PID -> Observer
    watched_nodes: Arc<Mutex<HashMap<NodeIdentifier, (AXUIElement, *mut c_void)>>>, // NodeId -> (Element, Context)
    sender: Arc<broadcast::Sender<String>>,
}

unsafe impl Send for NodeWatcher {}
unsafe impl Sync for NodeWatcher {}

impl NodeWatcher {
    pub fn new(sender: Arc<broadcast::Sender<String>>) -> Arc<Self> {
        Arc::new(Self {
            observers: Arc::new(Mutex::new(HashMap::new())),
            watched_nodes: Arc::new(Mutex::new(HashMap::new())),
            sender,
        })
    }

    /// Watch a node for changes (registers for accessibility notifications)
    /// Supports both element_id (Phase 3) and path (legacy) for backwards compatibility
    pub fn watch_node_by_id(
        &self,
        pid: u32,
        element_id: String,
        node_id: String,
    ) -> Result<(), String> {
        use crate::element_registry::ElementRegistry;

        println!(
            "🔍 watch_node called: PID {} element_id {} ID {}",
            pid, element_id, node_id
        );

        let node_identifier = NodeIdentifier {
            pid,
            element_id: Some(element_id.clone()),
            path: None,
        };

        // Get element from registry
        println!("  🗺️  Getting element from registry...");
        let element = match ElementRegistry::get(&element_id) {
            Some(el) => {
                println!("  ✅ Element found in registry");
                el
            }
            None => {
                println!("  ❌ Element not found in registry");
                return Err("Element not found in registry".to_string());
            }
        };

        self.watch_element_internal(node_identifier, element, node_id)
    }

    /// Internal method to watch an element (used by both path and element_id versions)
    fn watch_element_internal(
        &self,
        node_identifier: NodeIdentifier,
        element: AXUIElement,
        node_id: String,
    ) -> Result<(), String> {
        let pid = node_identifier.pid;

        // Get or create observer for this PID
        let observer = {
            let mut observers = self.observers.lock().unwrap();
            if !observers.contains_key(&pid) {
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

                println!("✅ Created AXObserver for PID {}", pid);

                // Add observer to the MAIN run loop (not current, which might be a Tokio thread)
                unsafe {
                    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
                    if run_loop_source_ref.is_null() {
                        return Err("Failed to get run loop source from observer".to_string());
                    }

                    let run_loop_source =
                        CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);

                    // Use the main run loop instead of current
                    let main_run_loop = CFRunLoop::get_main();
                    main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);

                    println!("✅ Added observer to main run loop");
                }

                observers.insert(pid, observer_ref);
                observer_ref
            } else {
                *observers.get(&pid).unwrap()
            }
        };

        // Create context for this specific node
        let context = Box::new(ObserverContext {
            node_id: node_identifier.clone(),
            sender: self.sender.clone(),
        });
        let context_ptr = Box::into_raw(context) as *mut c_void;

        // Register for notifications
        let notifications = vec![
            kAXValueChangedNotification,
            kAXMovedNotification,
            kAXResizedNotification,
            kAXUIElementDestroyedNotification,
        ];

        let element_ref = element.as_concrete_TypeRef() as AXUIElementRef;

        for notification in &notifications {
            let notif_cfstring = CFString::new(notification);
            let result = unsafe {
                AXObserverAddNotification(
                    observer,
                    element_ref,
                    notif_cfstring.as_concrete_TypeRef() as _,
                    context_ptr,
                )
            };

            if result != 0 {
                println!(
                    "⚠️  Failed to add notification {} for node {}: error code {}",
                    notification, node_id, result
                );
            } else {
                println!(
                    "✅ Registered notification {} for node {}",
                    notification, node_id
                );
            }
        }

        // Store element and context pointer
        self.watched_nodes
            .lock()
            .unwrap()
            .insert(node_identifier.clone(), (element, context_ptr));

        let path_str = node_identifier
            .path
            .as_ref()
            .map(|p| format!("{:?}", p))
            .unwrap_or_else(|| "N/A".to_string());
        let element_id_str = node_identifier
            .element_id
            .as_ref()
            .map(|id| id.clone())
            .unwrap_or_else(|| "N/A".to_string());

        println!(
            "👁️  Successfully watching node: ID {} PID {} element_id: {} path: {}",
            node_id, pid, element_id_str, path_str
        );

        // Store the watch for debugging
        println!(
            "📋 Total watched nodes: {}",
            self.watched_nodes.lock().unwrap().len()
        );

        Ok(())
    }

    /// Stop watching a node
    pub fn unwatch_node(&self, pid: u32, path: Vec<usize>) {
        let node_id = NodeIdentifier {
            pid,
            element_id: None,
            path: Some(path.clone()),
        };

        // Remove from watch list
        if let Some((element, context_ptr)) = self.watched_nodes.lock().unwrap().remove(&node_id) {
            let element_ref = element.as_concrete_TypeRef() as AXUIElementRef;

            // Remove notifications (if we still have the observer)
            if let Some(observer) = self.observers.lock().unwrap().get(&pid) {
                let notifications = vec![
                    kAXValueChangedNotification,
                    kAXResizedNotification,
                    kAXMovedNotification,
                    kAXUIElementDestroyedNotification,
                ];

                for notification in &notifications {
                    let notif_cfstring = CFString::new(notification);
                    unsafe {
                        let _ = AXObserverRemoveNotification(
                            *observer,
                            element_ref,
                            notif_cfstring.as_concrete_TypeRef() as _,
                        );
                    }
                }
            }

            // Free context
            unsafe {
                let _ = Box::from_raw(context_ptr as *mut ObserverContext);
            }

            println!("🚫 Stopped watching node: PID {} path {:?}", pid, path);
        }
    }

    /// Clear all watches (called on client disconnect to prevent stale observers)
    pub fn clear_all(&self) {
        println!("🧹 Clearing all watched nodes...");

        // Get all node identifiers before clearing
        let nodes: Vec<NodeIdentifier> =
            self.watched_nodes.lock().unwrap().keys().cloned().collect();

        // Unwatch each node
        for node_id in nodes {
            if let Some(path) = node_id.path {
                self.unwatch_node(node_id.pid, path);
            }
            // If element_id is present, we would use unwatch_node_by_id (future enhancement)
        }

        // Clear observers (they should all be removed by now, but just in case)
        self.observers.lock().unwrap().clear();

        println!("✨ All watches cleared");
    }
}

/// C callback for AXObserver notifications
unsafe extern "C" fn observer_callback(
    _observer: AXObserverRef,
    element: AXUIElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
) {
    if refcon.is_null() {
        return;
    }

    let context = &*(refcon as *const ObserverContext);
    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();

    // Convert the actual changed element to AXUIElement
    use core_foundation::base::TCFType;
    let changed_element = AXUIElement::wrap_under_get_rule(element);

    println!(
        "🔔 Notification: {} (PID: {}, registered watch: {:?})",
        notification_name, context.node_id.pid, context.node_id.path
    );

    // Extract data directly from the changed element and broadcast it
    handle_notification_direct(context, &notification_name, &changed_element);
}

/// Generate a stable ID for an element
/// Priority: kAXIdentifierAttribute > role:title:index composite
fn generate_stable_id(element: &AXUIElement, pid: u32) -> String {
    use accessibility::AXAttribute;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    // Try kAXIdentifierAttribute first (native stable ID)
    if let Ok(identifier_attr) =
        element.attribute(&AXAttribute::new(&CFString::new("AXIdentifier")))
    {
        if let Some(id_str) = unsafe {
            let cf_string =
                CFString::wrap_under_get_rule(identifier_attr.as_CFTypeRef() as *const _);
            let s = cf_string.to_string();
            if !s.is_empty() {
                Some(s)
            } else {
                None
            }
        } {
            return format!("{}::id:{}", pid, id_str);
        }
    }

    // Fallback: role:title:index composite
    let role: String = element
        .attribute(&AXAttribute::role())
        .ok()
        .and_then(|r| unsafe {
            let cf_string = CFString::wrap_under_get_rule(r.as_CFTypeRef() as *const _);
            Some(cf_string.to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());

    let title: String = element
        .attribute(&AXAttribute::title())
        .ok()
        .and_then(|t| unsafe {
            let cf_string = CFString::wrap_under_get_rule(t.as_CFTypeRef() as *const _);
            let s = cf_string.to_string();
            if !s.is_empty() {
                Some(s)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "".to_string());

    // Get index among siblings (if we can find parent)
    let index_str = if let Ok(parent) = element.attribute(&AXAttribute::parent()) {
        if let Ok(children) = parent.attribute(&AXAttribute::children()) {
            let element_ref = element.as_concrete_TypeRef();
            let mut found_index = None;
            for i in 0..children.len() {
                if let Some(sibling) = children.get(i) {
                    if std::ptr::eq(
                        element_ref as *const _,
                        sibling.as_concrete_TypeRef() as *const _,
                    ) {
                        found_index = Some(i);
                        break;
                    }
                }
            }
            found_index.map(|i| format!(":{}", i)).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    if title.is_empty() {
        format!("{}::{}:{}{}", pid, role, "untitled", index_str)
    } else {
        // Sanitize title for use in ID
        let safe_title = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .take(30)
            .collect::<String>();
        format!("{}::{}:{}{}", pid, role, safe_title, index_str)
    }
}

/// Handle notification by extracting data directly from the changed element (NEW APPROACH)
/// This avoids path lookups entirely - we work with the element macOS gives us
fn handle_notification_direct(
    context: &ObserverContext,
    notification: &str,
    element: &AXUIElement,
) {
    use accessibility::AXAttribute;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    // Generate stable ID for this element
    let stable_id = generate_stable_id(element, context.node_id.pid);

    // Extract element's role for context
    let role: String = element
        .attribute(&AXAttribute::role())
        .ok()
        .and_then(|r| unsafe {
            let cf_string = CFString::wrap_under_get_rule(r.as_CFTypeRef() as *const _);
            Some(cf_string.to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());

    println!(
        "  📍 Changed element: {} (Frontend ID: {})",
        role, stable_id
    );

    // Build update based on notification type
    let mut update = NodeUpdate {
        id: stable_id.clone(),
        pid: context.node_id.pid,
        element_id: context.node_id.element_id.clone(),
        path: context.node_id.path.clone(),
        value: None,
        bounds: None,
        focused: None,
        enabled: None,
    };

    let mut has_changes = false;

    match notification {
        "AXValueChanged" => {
            // Extract value directly from element
            if let Ok(value_attr) = element.attribute(&AXAttribute::value()) {
                if let Some(typed_value) = crate::ax_value::extract_value(&value_attr, Some(&role))
                {
                    update.value = Some(typed_value.clone());
                    println!("  📝 Value changed to: {:?}", typed_value);
                    has_changes = true;
                } else {
                    println!("  ⚠️  AXValueChanged but couldn't extract value, ignoring");
                    return;
                }
            } else {
                println!("  ⚠️  AXValueChanged but element has no value attribute, ignoring");
                return;
            }
        }
        "AXMoved" | "AXResized" => {
            // Extract bounds directly from element
            if let Ok(pos_attr) = element.attribute(&AXAttribute::new(&CFString::new("AXPosition")))
            {
                if let Some((x, y)) = crate::ax_value::extract_position(&pos_attr) {
                    if let Ok(size_attr) =
                        element.attribute(&AXAttribute::new(&CFString::new("AXSize")))
                    {
                        if let Some((width, height)) = crate::ax_value::extract_size(&size_attr) {
                            update.bounds = Some(crate::axio::Bounds {
                                position: crate::axio::Position { x, y },
                                size: crate::axio::Size { width, height },
                            });
                            println!("  📐 Bounds changed");
                            has_changes = true;
                        }
                    }
                }
            }
        }
        "AXUIElementDestroyed" => {
            println!("🗑️  Element destroyed: {}", stable_id);
            // TODO: Send a "node destroyed" event
            return;
        }
        _ => {
            println!("  ℹ️  Unhandled notification type: {}", notification);
            return;
        }
    }

    if !has_changes {
        return;
    }

    // Broadcast update to frontend
    let message = serde_json::json!({
        "event_type": "node_updated",
        "update": update,
    });

    if let Ok(json) = serde_json::to_string(&message) {
        let _ = context.sender.send(json);
        println!("📤 Broadcasted update for element {}", stable_id);
    }
}
