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

/// Determines which notifications to register for a given element role
/// Currently focused on TEXT ELEMENTS ONLY - watching value changes
fn get_notifications_for_role(role: &str) -> Vec<&'static str> {
    match role {
        // TEXT INPUT ELEMENTS - our primary focus!
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" => {
            vec![kAXValueChangedNotification]
        }

        // STATIC TEXT - labels that might update
        "AXStaticText" => {
            vec![kAXValueChangedNotification]
        }

        // Future: Interactive elements
        // "AXPopUpButton" | "AXButton" | "AXMenuButton" | "AXRadioButton" => {
        //     vec![kAXValueChangedNotification]
        // }

        // Future: CheckBoxes
        // "AXCheckBox" => {
        //     vec![kAXValueChangedNotification]
        // }

        // Future: Sliders, scrollbars
        // "AXSlider" | "AXScrollBar" | "AXIncrementor" | "AXValueIndicator" => {
        //     vec![kAXValueChangedNotification]
        // }

        // Future: Containers
        // "AXScrollArea" | "AXGroup" | "AXSplitGroup" => {
        //     vec![kAXValueChangedNotification]
        // }

        // Future: Windows - for position/size tracking
        // "AXWindow" | "AXSheet" | "AXDrawer" => {
        //     vec![
        //         kAXMovedNotification,
        //         kAXResizedNotification,
        //     ]
        // }

        // Everything else - don't subscribe
        _ => {
            vec![] // Only watching text elements right now
        }
    }
}

/// Unique identifier for a node
#[derive(Debug, Clone, Hash, Eq, PartialEq, Serialize, Deserialize)]
pub struct NodeIdentifier {
    pub pid: u32,
    pub element_id: String,
}

/// Context passed to observer callbacks
struct ObserverContext {
    node_id: NodeIdentifier,
    sender: Arc<broadcast::Sender<String>>,
}

/// Delta update for a node (only changed fields included)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeUpdate {
    pub id: String, // Element ID from registry for frontend to match
    pub pid: u32,
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
            element_id: element_id.clone(),
        };

        // Get element from registry
        println!("  🗺️  Getting element from registry...");
        let element = match ElementRegistry::get(&element_id) {
            Some(el) => {
                println!("  ✅ Element found in registry");

                // Verify the element is still valid
                use accessibility::AXAttribute;
                match el.attribute(&AXAttribute::role()) {
                    Ok(role_attr) => {
                        use core_foundation::base::TCFType;
                        use core_foundation::string::CFString;
                        let role = unsafe {
                            let cf_string =
                                CFString::wrap_under_get_rule(role_attr.as_CFTypeRef() as *const _);
                            cf_string.to_string()
                        };
                        println!("  ✅ Element is valid, role: {}", role);
                        println!("  📌 Element CFTypeRef: {:?}", el.as_concrete_TypeRef());
                    }
                    Err(e) => {
                        println!("  ⚠️  Element may be invalid: {:?}", e);
                    }
                }
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

                // Add observer to the MAIN run loop
                unsafe {
                    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
                    if run_loop_source_ref.is_null() {
                        return Err("Failed to get run loop source from observer".to_string());
                    }

                    let run_loop_source =
                        CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);

                    // MUST use main run loop - that's where Tauri processes events
                    let main_run_loop = CFRunLoop::get_main();
                    main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);

                    println!("✅ Added observer to MAIN run loop");
                    println!("🔍 Main RunLoop: {:?}", main_run_loop.as_concrete_TypeRef());
                }

                observers.insert(pid, observer_ref);
                observer_ref
            } else {
                *observers.get(&pid).unwrap()
            }
        };

        // Get element's role to determine which notifications to register
        let role = match element.attribute(&accessibility::AXAttribute::role()) {
            Ok(role_attr) => {
                use core_foundation::string::CFString;
                let role = unsafe {
                    let cf_string =
                        CFString::wrap_under_get_rule(role_attr.as_CFTypeRef() as *const _);
                    cf_string.to_string()
                };
                role
            }
            Err(_) => "Unknown".to_string(),
        };

        // Get notifications appropriate for this element type
        let notifications = get_notifications_for_role(&role);

        // Skip watching if element doesn't support any notifications
        if notifications.is_empty() {
            println!(
                "⏭️  Skipping {} (role: {}) - decorative element with no relevant notifications",
                node_id, role
            );
            return Ok(());
        }

        println!(
            "📍 Watching {} (role: {}) with {} notification(s)",
            node_id,
            role,
            notifications.len()
        );

        // Create context for this specific node
        let context = Box::new(ObserverContext {
            node_id: node_identifier.clone(),
            sender: self.sender.clone(),
        });
        let context_ptr = Box::into_raw(context) as *mut c_void;

        let element_ref = element.as_concrete_TypeRef() as AXUIElementRef;

        let mut registered_count = 0;
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
                // We shouldn't get -25207 anymore since we pre-filter by role
                // But if we do, or get other errors, log them
                println!(
                    "⚠️  Failed to add notification {} for {} (role: {}): error code {}",
                    notification, node_id, role, result
                );
            } else {
                registered_count += 1;
            }
        }

        if registered_count > 0 {
            println!(
                "✅ Registered {} notification(s) for {} (role: {})",
                registered_count, node_id, role
            );
        } else {
            println!(
                "⚠️  Failed to register any notifications for {} (role: {})",
                node_id, role
            );
        }

        // Store element and context pointer
        self.watched_nodes
            .lock()
            .unwrap()
            .insert(node_identifier.clone(), (element.clone(), context_ptr));

        println!(
            "🔐 Stored element in watched_nodes: {:?}",
            element.as_concrete_TypeRef()
        );

        println!(
            "👁️  Successfully watching node: ID {} PID {} element_id: {}",
            node_id, pid, &node_identifier.element_id
        );

        // Store the watch for debugging
        println!(
            "📋 Total watched nodes: {}",
            self.watched_nodes.lock().unwrap().len()
        );

        Ok(())
    }

    /// Stop watching a node by element ID
    pub fn unwatch_node_by_id(&self, pid: u32, element_id: String) {
        let node_id = NodeIdentifier {
            pid,
            element_id: element_id.clone(),
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

            println!(
                "🚫 Stopped watching node: PID {} element_id {}",
                pid, element_id
            );
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
            self.unwatch_node_by_id(node_id.pid, node_id.element_id);
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
    println!("🔔 OBSERVER CALLBACK FIRED!");

    if refcon.is_null() {
        println!("❌ refcon is null!");
        return;
    }

    let context = &*(refcon as *const ObserverContext);
    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();

    // Convert the actual changed element to AXUIElement
    use core_foundation::base::TCFType;
    let changed_element = AXUIElement::wrap_under_get_rule(element);

    println!(
        "🔔 Notification: {} (PID: {}, element_id: {})",
        notification_name, context.node_id.pid, context.node_id.element_id
    );

    // Extract data directly from the changed element and broadcast it
    handle_notification_direct(context, &notification_name, &changed_element);
}

/// Handle notification by extracting data directly from the changed element
/// Uses the element_id from the registry to identify the element
fn handle_notification_direct(
    context: &ObserverContext,
    notification: &str,
    element: &AXUIElement,
) {
    use accessibility::AXAttribute;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    // Use the element_id from context (which is from the registry)
    let element_id = &context.node_id.element_id;

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
        "  📍 Changed element: {} (Element ID: {})",
        role, element_id
    );

    // Build update based on notification type
    let mut update = NodeUpdate {
        id: element_id.clone(), // Use element_id so frontend can match it
        pid: context.node_id.pid,
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
            println!("🗑️  Element destroyed: {}", element_id);
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
        println!("📤 Broadcasted update for element {}", element_id);
    }
}
