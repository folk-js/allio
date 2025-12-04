/**
 * UI Element - First-Class Element Primitive
 *
 * Represents an accessibility element with all its operations bundled together.
 * Elements know their window association but are managed independently.
 */
use accessibility::AXUIElement;
use accessibility_sys::AXObserverRef;
use std::ffi::c_void;

use crate::platform::macos::AXNotification;
use crate::types::{AXValue, AxioError, AxioResult, ElementId, WindowId};

/// Context passed to AX observer callbacks
/// Must match the definition used in element_registry.rs observer_callback
#[derive(Clone)]
#[repr(C)]
pub struct ObserverContext {
    pub element_id: ElementId,
}

/// Watch state for an element (merged from NodeWatcher)
pub struct WatchState {
    /// Context pointer for AX observer callbacks
    pub observer_context: *mut c_void,
    /// Which notifications are registered for this element
    pub notifications: Vec<AXNotification>,
}

/// A UI element with all operations bundled together
pub struct UIElement {
    // ============================================================================
    // Identity
    // ============================================================================
    /// Unique UUID for this element (unchanging)
    id: ElementId,

    /// Which window this element belongs to
    window_id: WindowId,

    /// Parent element ID (None for root)
    #[allow(dead_code)]
    parent_id: Option<ElementId>,

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

// SAFETY: UIElement can be sent between threads and accessed concurrently because:
// 1. AXUIElement is a CFTypeRef (Core Foundation reference) which is reference-counted
//    and immutable. The underlying accessibility object is managed by the system.
// 2. All mutable state (watch_state) is only modified through ElementRegistry which
//    holds the UIElement behind a Mutex, ensuring exclusive access.
// 3. The macOS Accessibility API is thread-safe for read operations, and we only
//    perform write operations (set_value, watch) through the synchronized registry.
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}

impl UIElement {
    // ============================================================================
    // Constructor
    // ============================================================================

    /// Create a new UI element
    pub fn new(
        id: ElementId,
        window_id: WindowId,
        parent_id: Option<ElementId>,
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
    #[allow(dead_code)]
    pub fn id(&self) -> &ElementId {
        &self.id
    }

    /// Get the window ID this element belongs to
    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    /// Get the parent element ID (None for root)
    #[allow(dead_code)]
    pub fn parent_id(&self) -> Option<&ElementId> {
        self.parent_id.as_ref()
    }

    /// Get the element's PID
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Get the element's role
    #[allow(dead_code)]
    pub fn role(&self) -> &str {
        &self.role
    }

    /// Get the underlying AXUIElement reference
    pub fn ax_element(&self) -> &AXUIElement {
        &self.ax_element
    }

    /// Check if this element is currently being watched
    #[allow(dead_code)]
    pub fn is_watched(&self) -> bool {
        self.watch_state.is_some()
    }

    // ============================================================================
    // Read Operations
    // ============================================================================

    /// Get the current value of this element
    #[allow(dead_code)]
    pub fn get_value(&self) -> AxioResult<AXValue> {
        use accessibility::AXAttribute;

        let value_attr = self
            .ax_element
            .attribute(&AXAttribute::value())
            .map_err(|e| AxioError::AccessibilityError(format!("Failed to get value: {:?}", e)))?;

        // Use the platform-specific value extraction
        crate::platform::macos::extract_value(&value_attr, Some(&self.role))
            .ok_or_else(|| AxioError::AccessibilityError("Failed to extract value".to_string()))
    }

    /// Convert this element to an AXNode
    ///
    /// This delegates to platform-specific conversion but uses this element's metadata.
    /// If load_children is true, children will be loaded up to max_depth.
    #[allow(dead_code)]
    pub fn to_axnode(
        &self,
        load_children: bool,
        max_depth: usize,
        max_children: usize,
    ) -> Option<crate::types::AXNode> {
        // Delegate to platform-specific conversion
        // This will call back into ElementRegistry to register child elements
        crate::platform::macos::element_to_axnode(
            &self.ax_element,
            &self.window_id,
            self.pid,
            self.parent_id.as_ref(),
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
    pub fn set_value(&self, text: &str) -> AxioResult<()> {
        use accessibility::AXAttribute;
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        // Check if element is writable
        if !Self::is_writable_role(&self.role) {
            return Err(AxioError::NotSupported(format!(
                "Element with role '{}' is not writable",
                self.role
            )));
        }

        // Set the value using the AXValue attribute
        let cf_string = CFString::new(text);
        self.ax_element
            .set_attribute(&AXAttribute::value(), cf_string.as_CFType())
            .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {:?}", e)))?;

        Ok(())
    }

    /// Call an accessibility action on this element
    #[allow(dead_code)]
    pub fn call_action(&self, _action: &str) -> AxioResult<()> {
        // TODO: Implement action calls
        // The accessibility crate doesn't have a direct perform_action method
        // This needs to be implemented with low-level accessibility-sys calls
        Err(AxioError::NotSupported(
            "Action calls not yet implemented".to_string(),
        ))
    }

    /// Check if a role represents a writable element
    fn is_writable_role(role: &str) -> bool {
        matches!(
            role,
            "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSecureTextField" | "AXSearchField"
        )
    }

    // ============================================================================
    // Watch Operations (internal - use ElementRegistry::watch for public API)
    // ============================================================================

    /// Register accessibility notifications for this element
    ///
    /// This is an internal method - use `ElementRegistry::watch()` for the public API.
    /// Registers for accessibility notifications appropriate for this element's role.
    pub(crate) fn watch(&mut self, observer: AXObserverRef) -> AxioResult<()> {
        use accessibility_sys::{AXObserverAddNotification, AXUIElementRef};
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        // Don't watch if already watching
        if self.watch_state.is_some() {
            return Ok(());
        }

        // Get notifications appropriate for this element type
        let notifications = AXNotification::for_role(&self.role);

        // Skip watching if element doesn't support any notifications
        if notifications.is_empty() {
            return Ok(());
        }

        // Create context for this specific element (uses shared ObserverContext type)
        let context = Box::new(ObserverContext {
            element_id: self.id.clone(),
        });
        let context_ptr = Box::into_raw(context) as *mut c_void;

        let element_ref = self.ax_element.as_concrete_TypeRef() as AXUIElementRef;

        let mut registered_notifications = Vec::new();
        let mut registered_count = 0;

        for notification in &notifications {
            let notif_cfstring = CFString::new(notification.as_str());
            let result = unsafe {
                AXObserverAddNotification(
                    observer,
                    element_ref,
                    notif_cfstring.as_concrete_TypeRef() as _,
                    context_ptr,
                )
            };

            if result == 0 {
                registered_notifications.push(*notification);
                registered_count += 1;
            }
        }

        if registered_count == 0 {
            // Free the context since we didn't use it
            unsafe {
                let _ = Box::from_raw(context_ptr);
            }
            return Err(AxioError::ObserverError(format!(
                "Failed to register notifications for element {} (role: {})",
                self.id, self.role
            )));
        }

        // Store watch state
        self.watch_state = Some(WatchState {
            observer_context: context_ptr,
            notifications: registered_notifications,
        });

        Ok(())
    }

    /// Remove registered accessibility notifications for this element
    ///
    /// This is an internal method - use `ElementRegistry::unwatch()` for the public API.
    pub(crate) fn unwatch(&mut self, observer: AXObserverRef) {
        use accessibility_sys::{AXObserverRemoveNotification, AXUIElementRef};
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        if let Some(watch_state) = self.watch_state.take() {
            let element_ref = self.ax_element.as_concrete_TypeRef() as AXUIElementRef;

            // Remove all registered notifications
            for notification in &watch_state.notifications {
                let notif_cfstring = CFString::new(notification.as_str());
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
}
