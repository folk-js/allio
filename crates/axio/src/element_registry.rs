//! Element registry - manages UIElement lifecycle, lookup, and AXObserver watching.

use crate::types::{AxioError, AxioResult, ElementId, WindowId};
use crate::ui_element::UIElement;
use accessibility_sys::AXObserverRef;
use once_cell::sync::Lazy;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use uuid::Uuid;

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
                let observer_ref = crate::platform::macos::create_observer_for_pid(pid)?;
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
}
