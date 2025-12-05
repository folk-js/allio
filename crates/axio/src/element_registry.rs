//! Element registry - stores AXElement with macOS handles. Windows own elements.

use crate::platform::macos::{AXNotification, ObserverContext};
use crate::types::{AXElement, AxioError, AxioResult, ElementId, WindowId};
use accessibility::AXUIElement;
use accessibility_sys::AXObserverRef;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

fn ax_elements_equal(elem1: &AXUIElement, elem2: &AXUIElement) -> bool {
    use accessibility_sys::AXUIElementRef;
    use core_foundation::base::{CFEqual, TCFType};

    let ref1 = elem1.as_concrete_TypeRef() as AXUIElementRef;
    let ref2 = elem2.as_concrete_TypeRef() as AXUIElementRef;

    unsafe { CFEqual(ref1 as _, ref2 as _) != 0 }
}

/// Watch state for an element (notification subscriptions).
struct WatchState {
    observer_context: *mut c_void,
    notifications: Vec<AXNotification>,
}

/// Internal storage - AXElement plus macOS handle and watch state.
pub struct StoredElement {
    /// The element data (what we return)
    pub element: AXElement,
    /// macOS accessibility handle
    pub ax_element: AXUIElement,
    /// Process ID
    pub pid: u32,
    /// Platform role string (for watch notifications)
    pub platform_role: String,
    /// Watch state if subscribed
    watch_state: Option<WatchState>,
}

// SAFETY: AXUIElement is a CFTypeRef (reference-counted, immutable).
// All mutable state is behind Mutex.
unsafe impl Send for StoredElement {}
unsafe impl Sync for StoredElement {}

/// Per-window state: elements and shared observer.
struct WindowState {
    elements: HashMap<ElementId, StoredElement>,
    observer: Option<AXObserverRef>,
}

static ELEMENT_REGISTRY: Lazy<Mutex<Option<ElementRegistry>>> = Lazy::new(|| Mutex::new(None));

pub struct ElementRegistry {
    windows: HashMap<WindowId, WindowState>,
    element_to_window: HashMap<ElementId, WindowId>,
}

unsafe impl Send for ElementRegistry {}
unsafe impl Sync for ElementRegistry {}

impl ElementRegistry {
    pub fn initialize() {
        let mut registry = ELEMENT_REGISTRY.lock().unwrap();
        *registry = Some(ElementRegistry {
            windows: HashMap::new(),
            element_to_window: HashMap::new(),
        });
    }

    fn with<F, R>(f: F) -> R
    where
        F: FnOnce(&mut ElementRegistry) -> R,
    {
        let mut guard = ELEMENT_REGISTRY.lock().unwrap();
        let registry = guard.as_mut().expect("ElementRegistry not initialized");
        f(registry)
    }

    /// Register element, returning existing if equivalent (stable IDs).
    // TODO: Duplicate check is O(n) per window. Investigate CFHash for O(1).
    pub fn register(
        element: AXElement,
        ax_element: AXUIElement,
        pid: u32,
        platform_role: &str,
    ) -> AXElement {
        Self::with(|registry| {
            let window_id = WindowId::new(element.window_id.clone());

            let window_state = registry
                .windows
                .entry(window_id.clone())
                .or_insert_with(|| WindowState {
                    elements: HashMap::new(),
                    observer: None,
                });

            // Return existing if equivalent element found
            for stored in window_state.elements.values() {
                if ax_elements_equal(&stored.ax_element, &ax_element) {
                    return stored.element.clone();
                }
            }

            let stored = StoredElement {
                element: element.clone(),
                ax_element,
                pid,
                platform_role: platform_role.to_string(),
                watch_state: None,
            };

            window_state.elements.insert(element.id.clone(), stored);
            registry
                .element_to_window
                .insert(element.id.clone(), window_id);

            element
        })
    }

    /// Get element by ID (cached).
    pub fn get(element_id: &ElementId) -> AxioResult<AXElement> {
        Self::with(|registry| {
            let window_id = registry
                .element_to_window
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            registry
                .windows
                .get(window_id)
                .and_then(|w| w.elements.get(element_id))
                .map(|s| s.element.clone())
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))
        })
    }

    /// Get multiple elements by ID.
    pub fn get_many(element_ids: &[ElementId]) -> Vec<AXElement> {
        Self::with(|registry| {
            element_ids
                .iter()
                .filter_map(|id| {
                    let window_id = registry.element_to_window.get(id)?;
                    registry
                        .windows
                        .get(window_id)
                        .and_then(|w| w.elements.get(id))
                        .map(|s| s.element.clone())
                })
                .collect()
        })
    }

    /// Get all elements in registry (for initial sync).
    pub fn get_all() -> Vec<AXElement> {
        Self::with(|registry| {
            registry
                .windows
                .values()
                .flat_map(|w| w.elements.values().map(|s| s.element.clone()))
                .collect()
        })
    }

    /// Access stored element (for internal ops like click, write).
    pub fn with_stored<F, R>(element_id: &ElementId, f: F) -> AxioResult<R>
    where
        F: FnOnce(&StoredElement) -> R,
    {
        Self::with(|registry| {
            let window_id = registry
                .element_to_window
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            registry
                .windows
                .get(window_id)
                .and_then(|w| w.elements.get(element_id))
                .map(f)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))
        })
    }

    /// Update element's cached data (e.g., after refresh).
    pub fn update(element_id: &ElementId, updated: AXElement) -> AxioResult<()> {
        Self::with(|registry| {
            let window_id = registry
                .element_to_window
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?
                .clone();

            let stored = registry
                .windows
                .get_mut(&window_id)
                .and_then(|w| w.elements.get_mut(element_id))
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            stored.element = updated;
            Ok(())
        })
    }

    /// Update children for an element.
    pub fn set_children(element_id: &ElementId, children: Vec<ElementId>) -> AxioResult<()> {
        Self::with(|registry| {
            let window_id = registry
                .element_to_window
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?
                .clone();

            let stored = registry
                .windows
                .get_mut(&window_id)
                .and_then(|w| w.elements.get_mut(element_id))
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            stored.element.children = Some(children);
            Ok(())
        })
    }

    pub fn remove_element(element_id: &ElementId) {
        Self::with(|registry| {
            let Some(window_id) = registry.element_to_window.remove(element_id) else {
                return;
            };

            let Some(window_state) = registry.windows.get_mut(&window_id) else {
                return;
            };

            if let Some(mut stored) = window_state.elements.remove(element_id) {
                if let Some(observer) = window_state.observer {
                    unwatch_element(&mut stored, observer);
                }
            }
        });
    }

    pub fn remove_window_elements(window_id: &WindowId) {
        Self::with(|registry| {
            let Some(mut window_state) = registry.windows.remove(window_id) else {
                return;
            };

            if let Some(observer) = window_state.observer {
                for (_, mut stored) in window_state.elements.drain() {
                    unwatch_element(&mut stored, observer);
                }
            }

            registry.element_to_window.retain(|_, wid| wid != window_id);
        });
    }

    pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
        Self::with_stored(element_id, |stored| write_value(stored, text))?
    }

    pub fn click(element_id: &ElementId) -> AxioResult<()> {
        Self::with_stored(element_id, |stored| click_element(stored))?
    }

    pub fn watch(element_id: &ElementId) -> AxioResult<()> {
        Self::with(|registry| {
            let window_id = registry
                .element_to_window
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?
                .clone();

            let window_state = registry
                .windows
                .get_mut(&window_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            let stored = window_state
                .elements
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            let pid = stored.pid;

            // Get or create observer
            let observer = if let Some(obs) = window_state.observer {
                obs
            } else {
                let obs = crate::platform::macos::create_observer_for_pid(pid)?;
                window_state.observer = Some(obs);
                obs
            };

            let stored = window_state.elements.get_mut(element_id).unwrap();
            watch_element(stored, observer)
        })
    }

    pub fn unwatch(element_id: &ElementId) {
        Self::with(|registry| {
            let Some(window_id) = registry.element_to_window.get(element_id) else {
                return;
            };

            let Some(window_state) = registry.windows.get_mut(window_id) else {
                return;
            };

            let Some(observer) = window_state.observer else {
                return;
            };

            if let Some(stored) = window_state.elements.get_mut(element_id) {
                unwatch_element(stored, observer);
            }
        });
    }
}

// --- Element operations ---

fn write_value(stored: &StoredElement, text: &str) -> AxioResult<()> {
    use accessibility::AXAttribute;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    const WRITABLE_ROLES: &[&str] = &[
        "AXTextField",
        "AXTextArea",
        "AXComboBox",
        "AXSecureTextField",
        "AXSearchField",
    ];

    if !WRITABLE_ROLES.contains(&stored.platform_role.as_str()) {
        return Err(AxioError::NotSupported(format!(
            "Element with role '{}' is not writable",
            stored.platform_role
        )));
    }

    let cf_string = CFString::new(text);
    stored
        .ax_element
        .set_attribute(&AXAttribute::value(), cf_string.as_CFType())
        .map_err(|e| AxioError::AccessibilityError(format!("Failed to set value: {:?}", e)))?;

    Ok(())
}

fn click_element(stored: &StoredElement) -> AxioResult<()> {
    use accessibility_sys::{kAXPressAction, AXUIElementPerformAction};
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    let action = CFString::new(kAXPressAction);
    let result = unsafe {
        AXUIElementPerformAction(
            stored.ax_element.as_concrete_TypeRef(),
            action.as_concrete_TypeRef(),
        )
    };

    if result == 0 {
        Ok(())
    } else {
        Err(AxioError::AccessibilityError(format!(
            "AXUIElementPerformAction failed with code {}",
            result
        )))
    }
}

fn watch_element(stored: &mut StoredElement, observer: AXObserverRef) -> AxioResult<()> {
    use accessibility_sys::{AXObserverAddNotification, AXUIElementRef};
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    if stored.watch_state.is_some() {
        return Ok(());
    }

    let notifications = AXNotification::for_role(&stored.platform_role);
    if notifications.is_empty() {
        return Ok(());
    }

    let context = Box::new(ObserverContext {
        element_id: stored.element.id.clone(),
    });
    let context_ptr = Box::into_raw(context) as *mut c_void;
    let element_ref = stored.ax_element.as_concrete_TypeRef() as AXUIElementRef;

    let mut registered = Vec::new();
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
            registered.push(*notification);
        }
    }

    if registered.is_empty() {
        unsafe {
            let _ = Box::from_raw(context_ptr);
        }
        return Err(AxioError::ObserverError(format!(
            "Failed to register notifications for element {} (role: {})",
            stored.element.id, stored.platform_role
        )));
    }

    stored.watch_state = Some(WatchState {
        observer_context: context_ptr,
        notifications: registered,
    });

    Ok(())
}

fn unwatch_element(stored: &mut StoredElement, observer: AXObserverRef) {
    use accessibility_sys::{AXObserverRemoveNotification, AXUIElementRef};
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    let Some(watch_state) = stored.watch_state.take() else {
        return;
    };

    let element_ref = stored.ax_element.as_concrete_TypeRef() as AXUIElementRef;

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
