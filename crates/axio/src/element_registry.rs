//! Element registry - manages UIElement lifecycle, lookup, and AXObserver watching.

use accessibility_sys::{AXObserverCreate, AXObserverGetRunLoopSource, AXObserverRef};
use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use uuid::Uuid;

use crate::types::{AxioError, AxioResult, ElementId, ElementUpdate, WindowId};
use crate::ui_element::{ObserverContext, UIElement};

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

// TODO: Could be moved to app state instead of global singleton
static ELEMENT_REGISTRY: Lazy<Mutex<Option<ElementRegistry>>> = Lazy::new(|| Mutex::new(None));

pub struct ElementRegistry {
    elements: HashMap<ElementId, UIElement>,
    /// Used for cleanup when windows close
    window_to_elements: HashMap<WindowId, HashSet<ElementId>>,
    /// One observer per window, shared by all watched elements in that window
    observers: HashMap<WindowId, AXObserverRef>,
}

// SAFETY: All access is synchronized via Mutex
unsafe impl Send for ElementRegistry {}
unsafe impl Sync for ElementRegistry {}

impl ElementRegistry {
    pub fn initialize() {
        let mut registry = ELEMENT_REGISTRY.lock().unwrap();
        *registry = Some(ElementRegistry {
            elements: HashMap::new(),
            window_to_elements: HashMap::new(),
            observers: HashMap::new(),
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

    /// Register element, returning existing ID if equivalent element exists (via CFEqual).
    /// This ensures stable IDs across multiple tree queries.
    pub fn register(
        ax_element: accessibility::AXUIElement,
        window_id: &WindowId,
        pid: u32,
        parent_id: Option<&ElementId>,
        role: &str,
    ) -> ElementId {
        Self::with(|registry| {
            // Return existing ID if equivalent element exists
            if let Some(window_elements) = registry.window_to_elements.get(window_id) {
                for element_id in window_elements {
                    if let Some(existing) = registry.elements.get(element_id) {
                        if ax_elements_equal(existing.ax_element(), &ax_element) {
                            return element_id.clone();
                        }
                    }
                }
            }

            let id = ElementId::new(Uuid::new_v4().to_string());
            let ui_element = UIElement::new(
                id.clone(),
                window_id.clone(),
                parent_id.cloned(),
                ax_element,
                pid,
                role.to_string(),
            );

            registry.elements.insert(id.clone(), ui_element);
            registry
                .window_to_elements
                .entry(window_id.clone())
                .or_default()
                .insert(id.clone());

            id
        })
    }

    #[allow(dead_code)]
    pub fn get(element_id: &ElementId) -> Option<ElementId> {
        Self::with(|registry| {
            if registry.elements.contains_key(element_id) {
                Some(element_id.clone())
            } else {
                None
            }
        })
    }

    pub fn with_element<F, R>(element_id: &ElementId, f: F) -> AxioResult<R>
    where
        F: FnOnce(&UIElement) -> R,
    {
        Self::with(|registry| {
            registry
                .elements
                .get(element_id)
                .map(f)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))
        })
    }

    #[allow(dead_code)]
    pub fn with_element_mut<F, R>(element_id: &ElementId, f: F) -> AxioResult<R>
    where
        F: FnOnce(&mut UIElement) -> R,
    {
        Self::with(|registry| {
            registry
                .elements
                .get_mut(element_id)
                .map(f)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))
        })
    }

    #[allow(dead_code)]
    pub fn find_by_window(window_id: &WindowId) -> Vec<ElementId> {
        Self::with(|registry| {
            registry
                .window_to_elements
                .get(window_id)
                .map(|set| set.iter().cloned().collect())
                .unwrap_or_default()
        })
    }

    #[allow(dead_code)]
    pub fn contains(element_id: &ElementId) -> bool {
        Self::with(|registry| registry.elements.contains_key(element_id))
    }

    pub fn remove_element(element_id: &ElementId) {
        Self::with(|registry| {
            if let Some(element) = registry.elements.remove(element_id) {
                let window_id = element.window_id().clone();
                if let Some(&observer) = registry.observers.get(&window_id) {
                    let mut elem = element;
                    elem.unwatch(observer);
                }
                if let Some(window_elements) = registry.window_to_elements.get_mut(&window_id) {
                    window_elements.remove(element_id);
                }
            }
        });
    }

    /// Called when a window closes - cleans up elements and observer.
    pub fn remove_window_elements(window_id: &WindowId) {
        Self::with(|registry| {
            if let Some(element_ids) = registry.window_to_elements.remove(window_id) {
                let observer = registry.observers.get(window_id).copied();
                for element_id in element_ids {
                    if let Some(mut element) = registry.elements.remove(&element_id) {
                        if let Some(obs) = observer {
                            element.unwatch(obs);
                        }
                    }
                }
                registry.observers.remove(window_id);
            }
        });
    }

    pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
        Self::with_element(element_id, |element| element.set_value(text))?
    }

    /// Subscribe to notifications for element's role. One AXObserver per window.
    pub fn watch(element_id: &ElementId) -> AxioResult<()> {
        Self::with(|registry| {
            let element = registry
                .elements
                .get(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            let window_id = element.window_id().clone();
            let pid = element.pid();

            let observer = if let Some(&obs) = registry.observers.get(&window_id) {
                obs
            } else {
                let mut observer_ref: AXObserverRef = std::ptr::null_mut();
                let result = unsafe {
                    AXObserverCreate(
                        pid as i32,
                        observer_callback as _,
                        &mut observer_ref as *mut _,
                    )
                };
                if result != 0 {
                    return Err(AxioError::ObserverError(format!(
                        "AXObserverCreate failed with code {}",
                        result
                    )));
                }

                // Must add to MAIN run loop for callbacks to fire
                unsafe {
                    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
                    if run_loop_source_ref.is_null() {
                        return Err(AxioError::ObserverError(
                            "Failed to get run loop source".to_string(),
                        ));
                    }
                    let run_loop_source =
                        CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);
                    let main_run_loop = CFRunLoop::get_main();
                    main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);
                }

                registry.observers.insert(window_id.clone(), observer_ref);
                observer_ref
            };

            let element = registry
                .elements
                .get_mut(element_id)
                .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

            element.watch(observer)
        })
    }

    pub fn unwatch(element_id: &ElementId) {
        Self::with(|registry| {
            let Some(element) = registry.elements.get_mut(element_id) else {
                return;
            };
            let Some(&observer) = registry.observers.get(element.window_id()) else {
                return;
            };
            element.unwatch(observer);
        });
    }

    #[allow(dead_code)]
    pub fn get_children(
        element_id: &ElementId,
        max_depth: usize,
        max_children: usize,
    ) -> AxioResult<Vec<crate::types::AXNode>> {
        Self::with_element(element_id, |_| ())?;
        crate::platform::macos::get_children_by_element_id(&element_id.0, max_depth, max_children)
    }
}

unsafe extern "C" fn observer_callback(
    _observer: AXObserverRef,
    _element: accessibility_sys::AXUIElementRef,
    notification: core_foundation::string::CFStringRef,
    refcon: *mut std::ffi::c_void,
) {
    use accessibility::AXUIElement;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    assert!(
        !refcon.is_null(),
        "AXObserver callback received null refcon"
    );

    let context = &*(refcon as *const ObserverContext);
    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();
    let changed_element = AXUIElement::wrap_under_get_rule(_element);

    handle_notification(&context.element_id, &notification_name, &changed_element);
}

fn handle_notification(
    element_id: &ElementId,
    notification: &str,
    element: &accessibility::AXUIElement,
) {
    use crate::platform::macos::AXNotification;
    use accessibility::AXAttribute;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    let Some(notification_type) = AXNotification::from_str(notification) else {
        return;
    };

    let update = match notification_type {
        AXNotification::ValueChanged => {
            if let Ok(value_attr) = element.attribute(&AXAttribute::value()) {
                let role = element
                    .attribute(&AXAttribute::role())
                    .ok()
                    .and_then(|r| unsafe {
                        let cf_string = CFString::wrap_under_get_rule(r.as_CFTypeRef() as *const _);
                        Some(cf_string.to_string())
                    });

                crate::platform::macos::extract_value(&value_attr, role.as_deref()).map(|value| {
                    ElementUpdate::ValueChanged {
                        element_id: element_id.to_string(),
                        value,
                    }
                })
            } else {
                None
            }
        }

        AXNotification::TitleChanged => element
            .attribute(&AXAttribute::title())
            .ok()
            .map(|t| t.to_string())
            .filter(|s| !s.is_empty())
            .map(|label| ElementUpdate::LabelChanged {
                element_id: element_id.to_string(),
                label,
            }),

        AXNotification::UIElementDestroyed => {
            ElementRegistry::remove_element(element_id);
            Some(ElementUpdate::ElementDestroyed {
                element_id: element_id.to_string(),
            })
        }

        _ => None,
    };

    if let Some(update) = update {
        crate::events::emit_element_update(update);
    }
}
