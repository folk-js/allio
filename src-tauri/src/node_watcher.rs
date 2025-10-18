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
use crate::platform;

/// Unique identifier for a node (PID + path)
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeIdentifier {
    pub pid: u32,
    pub path: Vec<usize>,
}

/// Context passed to observer callbacks
struct ObserverContext {
    node_id: NodeIdentifier,
    sender: Arc<broadcast::Sender<String>>,
}

/// Delta update for a node (only changed fields included)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUpdate {
    pub id: String, // Stable node ID for frontend to match
    pub pid: u32,
    pub path: Vec<usize>,
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
    pub fn watch_node(&self, pid: u32, path: Vec<usize>, node_id: String) -> Result<(), String> {
        println!(
            "🔍 watch_node called: PID {} path {:?} ID {}",
            pid, path, node_id
        );

        let node_identifier = NodeIdentifier {
            pid,
            path: path.clone(),
        };

        // Navigate to the target element
        println!("  🗺️  Navigating to element...");
        let element = match self.navigate_to_element(pid, &path) {
            Some(el) => {
                println!("  ✅ Navigation successful");
                el
            }
            None => {
                println!("  ❌ Navigation failed");
                return Err("Could not navigate to element".to_string());
            }
        };

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

        println!(
            "👁️  Successfully watching node: ID {} PID {} path {:?}",
            node_id, pid, path
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
            path: path.clone(),
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
            self.unwatch_node(node_id.pid, node_id.path);
        }

        // Clear observers (they should all be removed by now, but just in case)
        self.observers.lock().unwrap().clear();

        println!("✨ All watches cleared");
    }

    /// Navigate to an element by path
    fn navigate_to_element(&self, pid: u32, path: &[usize]) -> Option<AXUIElement> {
        let app_element = AXUIElement::application(pid as i32);

        if path.is_empty() {
            return Some(app_element);
        }

        // Use platform function to navigate
        platform::navigate_to_element(&app_element, path)
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
        path: vec![], // We're not using paths anymore for identification
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
