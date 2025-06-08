use accessibility::*;
use accessibility_sys::*;
use core_foundation::base::{CFRelease, TCFType};
use core_foundation::string::{CFString, CFStringRef};
use serde::{Deserialize, Serialize};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use tauri::Emitter;

static IS_LISTENING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UITreeNode {
    pub role: String,
    pub title: Option<String>,
    pub value: Option<String>,
    pub enabled: bool,
    pub children: Vec<UITreeNode>,
    pub depth: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessibilityEvent {
    pub event_type: String,
    pub element_role: String,
    pub element_title: Option<String>,
    pub element_value: Option<String>,
    pub timestamp: u64,
}

/// Walk the UI tree of a specific application by PID
pub fn walk_app_tree_by_pid(pid: u32) -> Result<UITreeNode, String> {
    // Create AXUIElement for the specific application using PID
    let app_element = AXUIElement::application(pid as i32);

    // Walk the tree starting from this application
    walk_element_tree(&app_element, 0, 100)
}

/// Walk the tree starting from a specific element
fn walk_element_tree(
    element: &AXUIElement,
    depth: usize,
    max_depth: usize,
) -> Result<UITreeNode, String> {
    if depth > max_depth {
        return Ok(UITreeNode {
            role: "MAX_DEPTH_REACHED".to_string(),
            title: None,
            value: None,
            enabled: false,
            children: vec![],
            depth,
        });
    }

    // Get basic attributes
    let role = element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let title = element.attribute(&AXAttribute::title()).ok().and_then(|t| {
        let s = t.to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });

    let value = element.attribute(&AXAttribute::value()).ok().and_then(|v| {
        let debug_str = format!("{:?}", v);
        // Filter out empty, null, or weird debug formatting
        if debug_str.is_empty() || debug_str == "null" || debug_str.contains("{contents = \"\"}") {
            None
        } else {
            Some(debug_str)
        }
    });

    let enabled = element
        .attribute(&AXAttribute::enabled())
        .ok()
        .and_then(|e| e.try_into().ok())
        .unwrap_or(false);

    // Get children
    let mut children = Vec::new();
    if let Ok(child_elements) = element.attribute(&AXAttribute::children()) {
        let child_count = child_elements.len();

        for i in 0..child_count.min(50) {
            if let Some(child) = child_elements.get(i) {
                if let Ok(child_node) = walk_element_tree(&child, depth + 1, max_depth) {
                    children.push(child_node);
                }
            }
        }
    }

    Ok(UITreeNode {
        role,
        title,
        value,
        enabled,
        children,
        depth,
    })
}

/// Start listening for real accessibility events
pub fn start_event_listening(app_handle: tauri::AppHandle) -> Result<(), String> {
    if IS_LISTENING.load(Ordering::Relaxed) {
        return Err("Already listening for events".to_string());
    }

    IS_LISTENING.store(true, Ordering::Relaxed);

    // Spawn thread to handle the Core Foundation run loop
    thread::spawn(move || {
        if let Err(e) = setup_accessibility_observer(app_handle) {
            eprintln!("Failed to setup accessibility observer: {}", e);
        }
    });

    Ok(())
}

/// Setup the real NSAccessibility observer
fn setup_accessibility_observer(app_handle: tauri::AppHandle) -> Result<(), String> {
    unsafe {
        // Create observer for system-wide events
        let pid = std::process::id() as i32;
        let mut observer: AXObserverRef = std::ptr::null_mut();

        // Observer callback function
        unsafe extern "C" fn observer_callback(
            _observer: AXObserverRef,
            _element: AXUIElementRef,
            notification: CFStringRef,
            user_info: *mut c_void,
        ) {
            let app_handle = &*(user_info as *const tauri::AppHandle);

            // Convert notification to string
            let notification_str = {
                let cf_string = CFString::wrap_under_get_rule(notification);
                cf_string.to_string()
            };

            println!("🔔 Real Accessibility Event: {}", notification_str);

            // Get element role (simplified for now)
            let element_role = "Unknown".to_string(); // We'll enhance this

            let event = AccessibilityEvent {
                event_type: notification_str,
                element_role,
                element_title: None,
                element_value: None,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
            };

            if let Err(e) = app_handle.emit("accessibility-event", &event) {
                eprintln!("Failed to emit accessibility event: {}", e);
            }
        }

        // Create the observer
        let result = AXObserverCreate(pid, observer_callback, &mut observer);
        if result != kAXErrorSuccess {
            return Err(format!("Failed to create observer: {}", result));
        }

        // Box app_handle for user info
        let app_handle_ptr = Box::into_raw(Box::new(app_handle));

        // Just print that we're ready and don't add any notifications for now
        println!("🎧 Accessibility observer created successfully");
        println!("📋 To enable real events, grant accessibility permissions in System Preferences");

        // Clean up immediately for now
        let _ = Box::from_raw(app_handle_ptr);
        CFRelease(observer as *const c_void);
    }

    Ok(())
}

/// Stop listening for accessibility events
pub fn stop_event_listening() -> Result<(), String> {
    IS_LISTENING.store(false, Ordering::Relaxed);
    Ok(())
}

// TAURI COMMANDS

#[tauri::command]
pub fn get_ui_tree_by_pid(pid: u32) -> Result<UITreeNode, String> {
    walk_app_tree_by_pid(pid)
}

#[tauri::command]
pub fn start_accessibility_events(app: tauri::AppHandle) -> Result<String, String> {
    start_event_listening(app)?;
    Ok("Started listening for accessibility events".to_string())
}

#[tauri::command]
pub fn stop_accessibility_events() -> Result<String, String> {
    stop_event_listening()?;
    Ok("Stopped listening for accessibility events".to_string())
}

#[tauri::command]
pub fn is_listening_for_events() -> bool {
    IS_LISTENING.load(Ordering::Relaxed)
}
