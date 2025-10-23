/**
 * Node Watcher - Event-Driven Accessibility Notifications
 *
 * Uses macOS AXObserver API to receive real-time notifications when nodes change.
 * Keeps frontend handles "live" - when you hold a node reference in JS, it auto-updates.
 *
 * Implementation uses raw C FFI via accessibility-sys for maximum compatibility.
 */
use colored::Colorize;
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

        let node_identifier = NodeIdentifier {
            pid,
            element_id: element_id.clone(),
        };

        // Get element from registry
        let element = match ElementRegistry::get(&element_id) {
            Some(el) => el,
            None => {
                println!("{}", "ERROR: Element not found in registry".red());
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
                }

                println!(
                    "{}",
                    format!("Observer created for PID {}", pid).bright_black()
                );

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
            // Silently skip - no need to log skipped decorative elements
            return Ok(());
        }

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
                println!(
                    "{}",
                    format!(
                        "WARNING: Failed to register {} for {} (role: {}): error {}",
                        notification, node_id, role, result
                    )
                    .yellow()
                );
            } else {
                registered_count += 1;
            }
        }

        if registered_count == 0 {
            println!(
                "{}",
                format!(
                    "ERROR: No notifications registered for {} (role: {})",
                    node_id, role
                )
                .red()
            );
            return Err("Failed to register notifications".to_string());
        }

        // Store element and context pointer
        self.watched_nodes
            .lock()
            .unwrap()
            .insert(node_identifier.clone(), (element.clone(), context_ptr));

        println!("{}", format!("Watching: {} ({})", role, node_id).green());

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
        let count = self.watched_nodes.lock().unwrap().len();
        if count > 0 {
            println!("{}", format!("Clearing {} watches", count).bright_black());
        }

        // Get all node identifiers before clearing
        let nodes: Vec<NodeIdentifier> =
            self.watched_nodes.lock().unwrap().keys().cloned().collect();

        // Unwatch each node
        for node_id in nodes {
            self.unwatch_node_by_id(node_id.pid, node_id.element_id);
        }

        // Clear observers
        self.observers.lock().unwrap().clear();
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
        println!("{}", "ERROR: Observer callback received null context".red());
        return;
    }

    let context = &*(refcon as *const ObserverContext);
    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();

    // Convert the actual changed element to AXUIElement
    use core_foundation::base::TCFType;
    let changed_element = AXUIElement::wrap_under_get_rule(element);

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
                // Get role for proper value extraction
                let role = element
                    .attribute(&AXAttribute::role())
                    .ok()
                    .and_then(|r| unsafe {
                        let cf_string = CFString::wrap_under_get_rule(r.as_CFTypeRef() as *const _);
                        Some(cf_string.to_string())
                    });

                if let Some(typed_value) =
                    crate::ax_value::extract_value(&value_attr, role.as_deref())
                {
                    update.value = Some(typed_value.clone());
                    has_changes = true;

                    // Only log text changes
                    if matches!(typed_value, crate::axio::AXValue::String(_)) {
                        println!(
                            "{}",
                            format!("Text changed: {} → {:?}", element_id, typed_value)
                                .bright_blue()
                        );
                    }
                } else {
                    return; // Couldn't extract value
                }
            } else {
                return; // No value attribute
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
                            has_changes = true;
                        }
                    }
                }
            }
        }
        "AXUIElementDestroyed" => {
            // Element destroyed - could send event in future
            return;
        }
        _ => {
            return; // Unhandled notification type
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
    }
}
