//! UIElement - wraps an AXUIElement with operations and watch state.

use accessibility::AXUIElement;
use accessibility_sys::AXObserverRef;
use std::ffi::c_void;

use crate::platform::macos::{AXNotification, ObserverContext};
use crate::types::{AXValue, AxioError, AxioResult, ElementId, WindowId};

pub struct WatchState {
    pub observer_context: *mut c_void,
    pub notifications: Vec<AXNotification>,
}

pub struct UIElement {
    id: ElementId,
    window_id: WindowId,
    #[allow(dead_code)]
    parent_id: Option<ElementId>,
    ax_element: AXUIElement,
    pid: u32,
    role: String,
    watch_state: Option<WatchState>,
}

// SAFETY: AXUIElement is a CFTypeRef (reference-counted, immutable).
// All mutable state is behind ElementRegistry's Mutex.
unsafe impl Send for UIElement {}
unsafe impl Sync for UIElement {}

impl UIElement {
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

    #[allow(dead_code)]
    pub fn id(&self) -> &ElementId {
        &self.id
    }

    pub fn window_id(&self) -> &WindowId {
        &self.window_id
    }

    #[allow(dead_code)]
    pub fn parent_id(&self) -> Option<&ElementId> {
        self.parent_id.as_ref()
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    #[allow(dead_code)]
    pub fn role(&self) -> &str {
        &self.role
    }

    pub fn ax_element(&self) -> &AXUIElement {
        &self.ax_element
    }

    #[allow(dead_code)]
    pub fn is_watched(&self) -> bool {
        self.watch_state.is_some()
    }

    #[allow(dead_code)]
    pub fn get_value(&self) -> AxioResult<AXValue> {
        use accessibility::AXAttribute;

        let value_attr = self
            .ax_element
            .attribute(&AXAttribute::value())
            .map_err(|e| AxioError::AccessibilityError(format!("Failed to get value: {:?}", e)))?;

        crate::platform::macos::extract_value(&value_attr, Some(&self.role))
            .ok_or_else(|| AxioError::AccessibilityError("Failed to extract value".to_string()))
    }

    #[allow(dead_code)]
    pub fn to_axnode(
        &self,
        load_children: bool,
        max_depth: usize,
        max_children: usize,
    ) -> Option<crate::types::AXNode> {
        crate::platform::macos::element_to_axnode(
            &self.ax_element,
            &self.window_id,
            self.pid,
            self.parent_id.as_ref(),
            0,
            max_depth,
            max_children,
            load_children,
        )
    }

    pub fn set_value(&self, text: &str) -> AxioResult<()> {
        use accessibility::AXAttribute;
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        if !Self::is_writable_role(&self.role) {
            return Err(AxioError::NotSupported(format!(
                "Element with role '{}' is not writable",
                self.role
            )));
        }

        let cf_string = CFString::new(text);
        self.ax_element
            .set_attribute(&AXAttribute::value(), cf_string.as_CFType())
            .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {:?}", e)))?;

        Ok(())
    }

    // TODO: Implement with low-level accessibility-sys calls
    #[allow(dead_code)]
    pub fn call_action(&self, _action: &str) -> AxioResult<()> {
        Err(AxioError::NotSupported(
            "Action calls not yet implemented".to_string(),
        ))
    }

    fn is_writable_role(role: &str) -> bool {
        matches!(
            role,
            "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSecureTextField" | "AXSearchField"
        )
    }

    pub(crate) fn watch(&mut self, observer: AXObserverRef) -> AxioResult<()> {
        use accessibility_sys::{AXObserverAddNotification, AXUIElementRef};
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        if self.watch_state.is_some() {
            return Ok(());
        }

        let notifications = AXNotification::for_role(&self.role);
        if notifications.is_empty() {
            return Ok(());
        }

        let context = Box::new(ObserverContext {
            element_id: self.id.clone(),
        });
        let context_ptr = Box::into_raw(context) as *mut c_void;
        let element_ref = self.ax_element.as_concrete_TypeRef() as AXUIElementRef;

        let mut registered_notifications = Vec::new();

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
            }
        }

        if registered_notifications.is_empty() {
            unsafe {
                let _ = Box::from_raw(context_ptr);
            }
            return Err(AxioError::ObserverError(format!(
                "Failed to register notifications for element {} (role: {})",
                self.id, self.role
            )));
        }

        self.watch_state = Some(WatchState {
            observer_context: context_ptr,
            notifications: registered_notifications,
        });

        Ok(())
    }

    pub(crate) fn unwatch(&mut self, observer: AXObserverRef) {
        use accessibility_sys::{AXObserverRemoveNotification, AXUIElementRef};
        use core_foundation::base::TCFType;
        use core_foundation::string::CFString;

        if let Some(watch_state) = self.watch_state.take() {
            let element_ref = self.ax_element.as_concrete_TypeRef() as AXUIElementRef;

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

            unsafe {
                let _ = Box::from_raw(watch_state.observer_context);
            }
        }
    }
}
