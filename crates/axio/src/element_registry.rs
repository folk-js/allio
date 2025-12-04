//! Element registry - windows own elements, with reverse index for lookup.

use crate::types::{AxioError, AxioResult, ElementId, WindowId};
use crate::ui_element::UIElement;
use accessibility_sys::AXObserverRef;
use once_cell::sync::Lazy;
use std::collections::HashMap;
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

/// Per-window state: elements and their shared observer.
struct WindowState {
    elements: HashMap<ElementId, UIElement>,
    observer: Option<AXObserverRef>,
}

// TODO: Could be moved to app state instead of global singleton
static ELEMENT_REGISTRY: Lazy<Mutex<Option<ElementRegistry>>> = Lazy::new(|| Mutex::new(None));

pub struct ElementRegistry {
    /// Windows own their elements
    windows: HashMap<WindowId, WindowState>,
    /// Reverse index for O(1) element lookup
    element_to_window: HashMap<ElementId, WindowId>,
}

// SAFETY: All access is synchronized via Mutex
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

    /// Register element, returning existing ID if equivalent element exists (via CFEqual).
    /// This ensures stable IDs across multiple tree queries.
    // TODO: Duplicate check is O(n) per window. Investigate if CFHash could enable O(1) lookup.
    pub fn register(
        ax_element: accessibility::AXUIElement,
        window_id: &WindowId,
        pid: u32,
        parent_id: Option<&ElementId>,
        role: &str,
    ) -> ElementId {
        Self::with(|registry| {
            let window_state = registry
                .windows
                .entry(window_id.clone())
                .or_insert_with(|| WindowState {
                    elements: HashMap::new(),
                    observer: None,
                });

            // Check for existing equivalent element (stable IDs)
            for (element_id, existing) in &window_state.elements {
                if ax_elements_equal(existing.ax_element(), &ax_element) {
                    return element_id.clone();
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

            window_state.elements.insert(id.clone(), ui_element);
            registry
                .element_to_window
                .insert(id.clone(), window_id.clone());

            id
        })
    }

    pub fn with_element<F, R>(element_id: &ElementId, f: F) -> AxioResult<R>
    where
        F: FnOnce(&UIElement) -> R,
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

    pub fn remove_element(element_id: &ElementId) {
        Self::with(|registry| {
            let Some(window_id) = registry.element_to_window.remove(element_id) else {
                return;
            };

            let Some(window_state) = registry.windows.get_mut(&window_id) else {
                return;
            };

            if let Some(mut element) = window_state.elements.remove(element_id) {
                if let Some(observer) = window_state.observer {
                    element.unwatch(observer);
                }
            }
        });
    }

    /// Called when a window closes - cleans up all elements and observer.
    pub fn remove_window_elements(window_id: &WindowId) {
        Self::with(|registry| {
            let Some(mut window_state) = registry.windows.remove(window_id) else {
                return;
            };

            // Unwatch all elements if observer exists
            if let Some(observer) = window_state.observer {
                for (_, mut element) in window_state.elements.drain() {
                    element.unwatch(observer);
                }
            }

            // Clean up reverse index
            registry.element_to_window.retain(|_, wid| wid != window_id);
        });
    }

    pub fn write(element_id: &ElementId, text: &str) -> AxioResult<()> {
        Self::with_element(element_id, |element| element.set_value(text))?
    }

    /// Subscribe to notifications for element's role. One AXObserver per window.
    pub fn watch(element_id: &ElementId) -> AxioResult<()> {
        Self::with(|registry| {
            // Get window_id and pid from element
            let (window_id, pid) = {
                let window_id = registry
                    .element_to_window
                    .get(element_id)
                    .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?
                    .clone();

                let window_state = registry
                    .windows
                    .get(&window_id)
                    .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

                let element = window_state
                    .elements
                    .get(element_id)
                    .ok_or_else(|| AxioError::ElementNotFound(element_id.clone()))?;

                (window_id, element.pid())
            };

            // Get or create observer for this window
            let window_state = registry.windows.get_mut(&window_id).unwrap();
            let observer = if let Some(obs) = window_state.observer {
                obs
            } else {
                let obs = crate::platform::macos::create_observer_for_pid(pid)?;
                window_state.observer = Some(obs);
                obs
            };

            // Watch the element
            let element = window_state.elements.get_mut(element_id).unwrap();
            element.watch(observer)
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

            if let Some(element) = window_state.elements.get_mut(element_id) {
                element.unwatch(observer);
            }
        });
    }
}
