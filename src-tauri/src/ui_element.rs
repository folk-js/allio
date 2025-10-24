/**
 * UI Element - First-Class Element Primitive
 *
 * Represents an accessibility element with all its operations bundled together.
 * Elements know their window association but are managed independently.
 */
use accessibility::AXUIElement;
use accessibility_sys::AXObserverRef;
use std::ffi::c_void;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::axio::AXValue;

/// Watch state for an element (merged from NodeWatcher)
pub struct WatchState {
    /// Context pointer for AX observer callbacks
    pub observer_context: *mut c_void,
    /// Which notifications are registered for this element
    pub notifications: Vec<String>,
}

/// A UI element with all operations bundled together
pub struct UIElement {
    // ============================================================================
    // Identity
    // ============================================================================
    /// Unique UUID for this element (unchanging)
    id: String,

    /// Which window this element belongs to
    window_id: String,

    /// Parent element ID (None for root)
    parent_id: Option<String>,

    // ============================================================================
    // macOS References (internal)
    // ============================================================================
    /// macOS accessibility reference
    ax_element: AXUIElement,

    /// Cached PID (needed for AXObserver)
    pid: u32,

    // ============================================================================
    // Metadata (cached for performance)
    // ============================================================================
    /// AX role (e.g., "AXTextField")
    role: String,

    // ============================================================================
    // Watch State (merged from NodeWatcher)
    // ============================================================================
    /// Optional watch state (only set if element is being watched)
    watch_state: Option<WatchState>,
}

// Manual Send/Sync implementation - AXUIElement operations are thread-safe behind Mutex
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}

impl UIElement {
    // ============================================================================
    // Constructor
    // ============================================================================

    /// Create a new UI element
    pub fn new(
        id: String,
        window_id: String,
        parent_id: Option<String>,
        ax_element: AXUIElement,
        pid: u32,
        role: String,
    ) -> Self {
        Self {
            id,
            window_id,
            parent_id,
            ax_element,
            pid,
            role,
            watch_state: None,
        }
    }

    // ============================================================================
    // Getters
    // ============================================================================

    /// Get the element's unique ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get the window ID this element belongs to
    pub fn window_id(&self) -> &str {
        &self.window_id
    }

    /// Get the parent element ID (None for root)
    pub fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }

    /// Get the element's PID
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Get the element's role
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Get the underlying AXUIElement reference
    pub fn ax_element(&self) -> &AXUIElement {
        &self.ax_element
    }

    /// Check if this element is currently being watched
    pub fn is_watched(&self) -> bool {
        self.watch_state.is_some()
    }

    // ============================================================================
    // Read Operations
    // ============================================================================

    /// Get the current value of this element
    pub fn get_value(&self) -> Result<AXValue, String> {
        use accessibility::AXAttribute;

        let value_attr = self
            .ax_element
            .attribute(&AXAttribute::value())
            .map_err(|e| format!("Failed to get value attribute: {:?}", e))?;

        // Use the platform-specific value extraction
        crate::platform::macos::extract_value(&value_attr, Some(&self.role))
            .ok_or_else(|| "Failed to extract value".to_string())
    }

    /// Convert this element to an AXNode
    ///
    /// This delegates to platform-specific conversion but uses this element's metadata.
    /// If load_children is true, children will be loaded up to max_depth.
    pub fn to_axnode(
        &self,
        load_children: bool,
        max_depth: usize,
        max_children: usize,
    ) -> Option<crate::axio::AXNode> {
        // Delegate to platform-specific conversion
        // This will call back into ElementRegistry to register child elements
        crate::platform::macos::element_to_axnode(
            &self.ax_element,
            self.window_id.clone(),
            self.pid,
            self.parent_id.clone(),
            0, // Current depth (this is the starting point)
            max_depth,
            max_children,
            load_children,
        )
    }

    // ============================================================================
    // Write Operations
    // ============================================================================

    /// Set the value of this element (write text)
    pub fn set_value(&self, text: &str) -> Result<(), String> {
        use accessibility::AXAttribute;
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        // Check if element is writable
        if !Self::is_writable_role(&self.role) {
            return Err(format!("Element with role '{}' is not writable", self.role));
        }

        // Set the value using the AXValue attribute
        let cf_string = CFString::new(text);
        self.ax_element
            .set_attribute(&AXAttribute::value(), cf_string.as_CFType())
            .map_err(|e| format!("Failed to set value: {:?}", e))?;

        Ok(())
    }

    /// Call an accessibility action on this element
    pub fn call_action(&self, _action: &str) -> Result<(), String> {
        // TODO: Implement action calls
        // The accessibility crate doesn't have a direct perform_action method
        // This needs to be implemented with low-level accessibility-sys calls
        Err("Action calls not yet implemented".to_string())
    }

    /// Check if a role represents a writable element
    fn is_writable_role(role: &str) -> bool {
        matches!(
            role,
            "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSecureTextField" | "AXSearchField"
        )
    }

    // ============================================================================
    // Watch Operations
    // ============================================================================

    /// Start watching this element for changes
    ///
    /// Registers for accessibility notifications appropriate for this element's role.
    /// Requires mutable access since it modifies watch_state.
    pub fn watch(
        &mut self,
        observer: AXObserverRef,
        sender: Arc<broadcast::Sender<String>>,
    ) -> Result<(), String> {
        use accessibility_sys::{AXObserverAddNotification, AXUIElementRef};
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        // Don't watch if already watching
        if self.watch_state.is_some() {
            return Ok(());
        }

        // Get notifications appropriate for this element type
        let notifications = Self::get_notifications_for_role(&self.role);

        // Skip watching if element doesn't support any notifications
        if notifications.is_empty() {
            return Ok(());
        }

        // Create context for this specific element
        #[derive(Clone)]
        struct ObserverContext {
            element_id: String,
            sender: Arc<broadcast::Sender<String>>,
        }

        let context = Box::new(ObserverContext {
            element_id: self.id.clone(),
            sender: sender.clone(),
        });
        let context_ptr = Box::into_raw(context) as *mut c_void;

        let element_ref = self.ax_element.as_concrete_TypeRef() as AXUIElementRef;

        let mut registered_notifications = Vec::new();
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

            if result == 0 {
                registered_notifications.push(notification.to_string());
                registered_count += 1;
            }
        }

        if registered_count == 0 {
            // Free the context since we didn't use it
            unsafe {
                let _ = Box::from_raw(context_ptr);
            }
            return Err(format!(
                "Failed to register notifications for element {} (role: {})",
                self.id, self.role
            ));
        }

        // Store watch state
        self.watch_state = Some(WatchState {
            observer_context: context_ptr,
            notifications: registered_notifications,
        });

        Ok(())
    }

    /// Stop watching this element for changes
    pub fn unwatch(&mut self, observer: AXObserverRef) {
        use accessibility_sys::{AXObserverRemoveNotification, AXUIElementRef};
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        if let Some(watch_state) = self.watch_state.take() {
            let element_ref = self.ax_element.as_concrete_TypeRef() as AXUIElementRef;

            // Remove all registered notifications
            for notification in &watch_state.notifications {
                let notif_cfstring = CFString::new(notification);
                unsafe {
                    let _ = AXObserverRemoveNotification(
                        observer,
                        element_ref,
                        notif_cfstring.as_concrete_TypeRef() as _,
                    );
                }
            }

            // Free context
            unsafe {
                let _ = Box::from_raw(watch_state.observer_context);
            }
        }
    }

    /// Determines which notifications to register for a given element role
    /// Conservative approach: only subscribe to essential notifications
    fn get_notifications_for_role(role: &str) -> Vec<&'static str> {
        // Note: Using string literals since not all constants are in accessibility_sys
        // kAXValueChangedNotification is available, others we use strings
        use accessibility_sys::kAXValueChangedNotification;

        match role {
            // TEXT INPUT ELEMENTS - watch value changes
            "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" => {
                vec![
                    kAXValueChangedNotification,
                    "AXUIElementDestroyed", // Know when element is destroyed
                ]
            }

            // WINDOWS - watch title changes
            "AXWindow" => {
                vec!["AXTitleChanged", "AXUIElementDestroyed"]
            }

            // Everything else - no subscriptions
            // Note: AXStaticText does NOT reliably emit value changed notifications
            // (apps must manually post them, which most don't do)
            _ => {
                vec![] // Conservative: only reliable elements
            }
        }
    }
}
