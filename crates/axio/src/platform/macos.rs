/**
 * macOS Platform Implementation
 *
 * Converts macOS Accessibility API elements to AXElement format.
 * All macOS-specific knowledge is encapsulated here.
 */
use accessibility::*;
use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use uuid::Uuid;

use crate::types::{
    AXElement, AXRole, AXValue, AxioError, AxioResult, Bounds, ElementId, WindowId,
};

// ============================================================================
// macOS Accessibility Notifications
// ============================================================================

/// Type-safe representation of macOS accessibility notifications
///
/// These map to `kAX*Notification` constants from the Accessibility API.
/// Using an enum prevents typos and enables compile-time checking.
///
/// Note: This is macOS-specific. Other platforms will have their own
/// notification types in their respective platform modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AXNotification {
    /// Element's value attribute changed (text fields, sliders, etc.)
    ValueChanged,
    /// Element's title attribute changed (windows, buttons with labels)
    TitleChanged,
    /// Element was destroyed (removed from the UI)
    UIElementDestroyed,
    /// Focus moved to this element
    FocusedUIElementChanged,
    /// Selected children changed (lists, tables)
    SelectedChildrenChanged,
}

impl AXNotification {
    /// Get the macOS notification name string
    ///
    /// These strings match the `kAX*Notification` constants.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ValueChanged => "AXValueChanged",
            Self::TitleChanged => "AXTitleChanged",
            Self::UIElementDestroyed => "AXUIElementDestroyed",
            Self::FocusedUIElementChanged => "AXFocusedUIElementChanged",
            Self::SelectedChildrenChanged => "AXSelectedChildrenChanged",
        }
    }

    /// Get notifications appropriate for a given macOS accessibility role
    ///
    /// Conservative approach: only subscribe to essential, reliable notifications.
    pub fn for_role(role: &str) -> Vec<Self> {
        match role {
            // Text input elements - watch value changes
            "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSearchField" => {
                vec![Self::ValueChanged, Self::UIElementDestroyed]
            }
            // Windows - watch title changes
            "AXWindow" => vec![Self::TitleChanged, Self::UIElementDestroyed],
            // Everything else - no subscriptions
            // Note: AXStaticText does NOT reliably emit value changed notifications
            _ => vec![],
        }
    }

    /// Parse from notification name string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "AXValueChanged" => Some(Self::ValueChanged),
            "AXTitleChanged" => Some(Self::TitleChanged),
            "AXUIElementDestroyed" => Some(Self::UIElementDestroyed),
            "AXFocusedUIElementChanged" => Some(Self::FocusedUIElementChanged),
            "AXSelectedChildrenChanged" => Some(Self::SelectedChildrenChanged),
            _ => None,
        }
    }
}

// ============================================================================
// AXObserver Creation and Callbacks
// ============================================================================

use accessibility_sys::{AXObserverCreate, AXObserverGetRunLoopSource, AXObserverRef};
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};

/// Context passed to AX observer callbacks
#[derive(Clone)]
#[repr(C)]
pub struct ObserverContext {
    pub element_id: ElementId,
}

/// Create an AXObserver for a process and add it to the main run loop.
pub fn create_observer_for_pid(pid: u32) -> AxioResult<AXObserverRef> {
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
        let run_loop_source = CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);
        let main_run_loop = CFRunLoop::get_main();
        main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);
    }

    Ok(observer_ref)
}

unsafe extern "C" fn observer_callback(
    _observer: AXObserverRef,
    _element: accessibility_sys::AXUIElementRef,
    notification: core_foundation::string::CFStringRef,
    refcon: *mut std::ffi::c_void,
) {
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

fn handle_notification(element_id: &ElementId, notification: &str, ax_element: &AXUIElement) {
    use crate::element_registry::ElementRegistry;

    let Some(notification_type) = AXNotification::from_str(notification) else {
        return;
    };

    match notification_type {
        AXNotification::ValueChanged => {
            // Get fresh value and update cached element
            if let Ok(value_attr) = ax_element.attribute(&AXAttribute::value()) {
                let role = ax_element
                    .attribute(&AXAttribute::role())
                    .ok()
                    .map(|r| r.to_string());

                if let Some(value) = extract_value(&value_attr, role.as_deref()) {
                    // Update cached element and emit
                    if let Ok(mut element) = ElementRegistry::get(element_id) {
                        element.value = Some(value);
                        let _ = ElementRegistry::update(element_id, element.clone());
                        crate::events::emit_element_changed(&element);
                    }
                }
            }
        }

        AXNotification::TitleChanged => {
            if let Ok(title) = ax_element.attribute(&AXAttribute::title()) {
                let label = title.to_string();
                if !label.is_empty() {
                    if let Ok(mut element) = ElementRegistry::get(element_id) {
                        element.label = Some(label);
                        let _ = ElementRegistry::update(element_id, element.clone());
                        crate::events::emit_element_changed(&element);
                    }
                }
            }
        }

        AXNotification::UIElementDestroyed => {
            // Get element before removing (for event data)
            if let Ok(element) = ElementRegistry::get(element_id) {
                ElementRegistry::remove_element(element_id);
                crate::events::emit_element_removed(&element);
            } else {
                ElementRegistry::remove_element(element_id);
            }
        }

        _ => {}
    }
}

// ============================================================================
// AXUIElement to AXElement Conversion
// ============================================================================

/// Build an AXElement from a macOS AXUIElement and register it.
/// Returns the registered element (may be existing if duplicate).
pub fn build_element(
    ax_element: &AXUIElement,
    window_id: &WindowId,
    pid: u32,
    parent_id: Option<&ElementId>,
) -> AXElement {
    use crate::element_registry::ElementRegistry;

    let platform_role = ax_element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let role = map_platform_role(&platform_role);

    let subrole = if matches!(role, AXRole::Unknown) {
        Some(platform_role.clone())
    } else {
        ax_element
            .attribute(&AXAttribute::subrole())
            .ok()
            .map(|sr| sr.to_string())
            .filter(|s| !s.is_empty())
    };

    let label = ax_element
        .attribute(&AXAttribute::title())
        .ok()
        .and_then(|t| {
            let s = t.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let value = ax_element
        .attribute(&AXAttribute::value())
        .ok()
        .and_then(|v| extract_value(&v, Some(&platform_role)));

    let description = ax_element
        .attribute(&AXAttribute::description())
        .ok()
        .and_then(|d| {
            let s = d.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let placeholder = ax_element
        .attribute(&AXAttribute::placeholder_value())
        .ok()
        .and_then(|p| {
            let s = p.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let bounds = get_element_bounds(ax_element);
    let focused = ax_element
        .attribute(&AXAttribute::focused())
        .ok()
        .and_then(|f| f.try_into().ok());
    let enabled = ax_element
        .attribute(&AXAttribute::enabled())
        .ok()
        .and_then(|e| e.try_into().ok());

    let element = AXElement {
        id: ElementId::new(Uuid::new_v4().to_string()),
        window_id: window_id.0.clone(),
        parent_id: parent_id.cloned(),
        children: None, // Discovered separately
        role,
        subrole,
        label,
        value,
        description,
        placeholder,
        bounds,
        focused,
        enabled,
    };

    // Register (returns existing if duplicate)
    ElementRegistry::register(element, ax_element.clone(), pid, &platform_role)
}

/// Discover and register children of an element. Updates parent's children.
/// Returns the child elements.
pub fn discover_children(parent_id: &ElementId, max_children: usize) -> AxioResult<Vec<AXElement>> {
    use crate::element_registry::ElementRegistry;

    let (ax_element, window_id, pid) = ElementRegistry::with_stored(parent_id, |stored| {
        (
            stored.ax_element.clone(),
            WindowId::new(stored.element.window_id.clone()),
            stored.pid,
        )
    })?;

    let children_array = match ax_element.attribute(&AXAttribute::children()) {
        Ok(children) => children,
        Err(_) => {
            ElementRegistry::set_children(parent_id, vec![])?;
            return Ok(vec![]);
        }
    };

    let child_count = children_array.len();
    let mut children = Vec::new();
    let mut child_ids = Vec::new();

    for i in 0..child_count.min(max_children as isize) {
        if let Some(child_ref) = children_array.get(i) {
            let child = build_element(&child_ref, &window_id, pid, Some(parent_id));
            child_ids.push(child.id.clone());
            children.push(child);
        }
    }

    ElementRegistry::set_children(parent_id, child_ids.clone())?;

    // Emit element:added for each new child
    for child in &children {
        crate::events::emit_element_added(child);
    }

    // Emit element:changed for the parent so client knows it now has children
    if let Ok(updated_parent) = ElementRegistry::get(parent_id) {
        crate::events::emit_element_changed(&updated_parent);
    }

    Ok(children)
}

/// Refresh an element's attributes from macOS.
pub fn refresh_element(element_id: &ElementId) -> AxioResult<AXElement> {
    use crate::element_registry::ElementRegistry;

    let (ax_element, window_id, _pid, parent_id, children, platform_role) =
        ElementRegistry::with_stored(element_id, |stored| {
            (
                stored.ax_element.clone(),
                stored.element.window_id.clone(),
                stored.pid,
                stored.element.parent_id.clone(),
                stored.element.children.clone(),
                stored.platform_role.clone(),
            )
        })?;

    let role = map_platform_role(&platform_role);

    let subrole = if matches!(role, AXRole::Unknown) {
        Some(platform_role.clone())
    } else {
        ax_element
            .attribute(&AXAttribute::subrole())
            .ok()
            .map(|sr| sr.to_string())
            .filter(|s| !s.is_empty())
    };

    let label = ax_element
        .attribute(&AXAttribute::title())
        .ok()
        .and_then(|t| {
            let s = t.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let value = ax_element
        .attribute(&AXAttribute::value())
        .ok()
        .and_then(|v| extract_value(&v, Some(&platform_role)));

    let description = ax_element
        .attribute(&AXAttribute::description())
        .ok()
        .and_then(|d| {
            let s = d.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let placeholder = ax_element
        .attribute(&AXAttribute::placeholder_value())
        .ok()
        .and_then(|p| {
            let s = p.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let bounds = get_element_bounds(&ax_element);
    let focused = ax_element
        .attribute(&AXAttribute::focused())
        .ok()
        .and_then(|f| f.try_into().ok());
    let enabled = ax_element
        .attribute(&AXAttribute::enabled())
        .ok()
        .and_then(|e| e.try_into().ok());

    let updated = AXElement {
        id: element_id.clone(),
        window_id,
        parent_id,
        children,
        role,
        subrole,
        label,
        value,
        description,
        placeholder,
        bounds,
        focused,
        enabled,
    };

    ElementRegistry::update(element_id, updated.clone())?;
    Ok(updated)
}

/// Map macOS AX* roles to ARIA-based AXIO roles
fn map_platform_role(platform_role: &str) -> AXRole {
    // Remove "AX" prefix if present
    let role = platform_role
        .strip_prefix("AX")
        .unwrap_or(platform_role)
        .to_lowercase();

    match role.as_str() {
        // Document structure
        "application" => AXRole::Application,
        "window" | "standardwindow" => AXRole::Window,
        "group" | "scrollarea" => AXRole::Group,

        // Interactive elements
        "button" | "defaultbutton" => AXRole::Button,
        "checkbox" => AXRole::Checkbox,
        "radiobutton" => AXRole::Radio,
        "toggle" => AXRole::Toggle,
        "textfield" | "textarea" | "textbox" | "securetextfield" | "combobox" => AXRole::Textbox,
        "searchfield" => AXRole::Searchbox,
        "slider" => AXRole::Slider,
        "menu" => AXRole::Menu,
        "menuitem" => AXRole::Menuitem,
        "menubar" => AXRole::Menubar,
        "link" => AXRole::Link,
        "tab" => AXRole::Tab,
        "tabgroup" => AXRole::Tablist,

        // Static content
        "statictext" | "text" => AXRole::Text,
        "heading" => AXRole::Heading,
        "image" => AXRole::Image,
        "list" => AXRole::List,
        "listitem" | "row" => AXRole::Listitem,
        "table" => AXRole::Table,
        "cell" => AXRole::Cell,

        // Other
        "progressindicator" => AXRole::Progressbar,
        "scrollbar" => AXRole::Scrollbar,

        _ => AXRole::Unknown,
    }
}

/// Extract geometry (position and size) from element
fn get_element_bounds(element: &AXUIElement) -> Option<Bounds> {
    // Get position
    let position_attr = CFString::new(kAXPositionAttribute);
    let ax_position_attr = AXAttribute::new(&position_attr);

    let position = element
        .attribute(&ax_position_attr)
        .ok()
        .and_then(|p| extract_position(&p))?;

    // Get size
    let size_attr = CFString::new(kAXSizeAttribute);
    let ax_size_attr = AXAttribute::new(&size_attr);

    let size = element
        .attribute(&ax_size_attr)
        .ok()
        .and_then(|s| extract_size(&s))?;

    Some(Bounds {
        x: position.0,
        y: position.1,
        w: size.0,
        h: size.1,
    })
}

/// Get accessibility tree for an application by PID
///
/// This is the main entry point for getting an accessibility tree
/// in AXIO format from a macOS application.
///
/// If `load_children` is false, returns only the root node with children_count populated.
/// Get all window AXUIElements for a given PID
///
/// Returns a vector of AXUIElements for each window (no CGWindowID).
/// We match windows by bounds instead of using private APIs.
pub fn get_window_elements(pid: u32) -> AxioResult<Vec<AXUIElement>> {
    use core_foundation::string::CFString;

    let app_element = AXUIElement::application(pid as i32);

    // Get children of the application element
    let children_array = match app_element.attribute(&AXAttribute::children()) {
        Ok(children) => children,
        Err(_) => return Ok(Vec::new()),
    };

    let child_count = children_array.len();

    let mut result = Vec::new();

    // Filter children by role = "AXWindow"
    for i in 0..child_count {
        if let Some(child_element) = children_array.get(i) {
            // Check if role is "AXWindow"
            if let Ok(role) = child_element.attribute(&AXAttribute::role()) {
                let role_str = unsafe {
                    let cf_string = CFString::wrap_under_get_rule(role.as_CFTypeRef() as *const _);
                    cf_string.to_string()
                };

                if role_str == "AXWindow" {
                    result.push((*child_element).clone());
                }
            }
        }
    }

    Ok(result)
}

/// Get the root element for a window.
pub fn get_window_root(window_id: &WindowId) -> AxioResult<AXElement> {
    use crate::window_manager::WindowManager;

    let managed_window = WindowManager::get_window(window_id)
        .ok_or_else(|| AxioError::WindowNotFound(window_id.clone()))?;

    let window_element = managed_window
        .ax_element
        .ok_or_else(|| AxioError::Internal(format!("Window {} has no AX element", window_id)))?;

    Ok(build_element(
        &window_element,
        window_id,
        managed_window.info.process_id,
        None,
    ))
}

/// Get the accessibility element at a specific screen position.
/// Element must belong to a tracked window.
pub fn get_element_at_position(x: f64, y: f64) -> AxioResult<AXElement> {
    use accessibility_sys::{AXUIElementGetPid, AXUIElementRef};
    use core_foundation::base::TCFType;
    use std::ptr;

    unsafe {
        let system_element = AXUIElement::system_wide();

        let mut element_ref: AXUIElementRef = ptr::null_mut();
        let result = AXUIElementCopyElementAtPosition(
            system_element.as_concrete_TypeRef(),
            x as f32,
            y as f32,
            &mut element_ref,
        );

        if result != 0 {
            return Err(AxioError::AccessibilityError(format!(
                "AXUIElementCopyElementAtPosition failed at ({}, {}) with code {}",
                x, y, result
            )));
        }

        if element_ref.is_null() {
            return Err(AxioError::AccessibilityError(format!(
                "No element found at position ({}, {})",
                x, y
            )));
        }

        let mut ax_element = AXUIElement::wrap_under_create_rule(element_ref);

        let mut pid: i32 = 0;
        let pid_result = AXUIElementGetPid(element_ref, &mut pid);
        if pid_result != 0 {
            return Err(AxioError::AccessibilityError(format!(
                "Failed to get PID for element at ({}, {})",
                x, y
            )));
        }

        // Traverse down to find the leafmost (deepest) element
        ax_element = find_leafmost_element_at_position(&ax_element, x, y);

        // Element must belong to a tracked window
        let window_id_str = get_window_id_for_element(&ax_element).ok_or_else(|| {
            AxioError::WindowNotFound(WindowId::new(format!(
                "untracked-window-at-{:.0}-{:.0}",
                x, y
            )))
        })?;
        let window_id = WindowId::new(window_id_str);

        Ok(build_element(&ax_element, &window_id, pid as u32, None))
    }
}

/// Recursively find the deepest (leafmost) element at a given position
fn find_leafmost_element_at_position(element: &AXUIElement, x: f64, y: f64) -> AXUIElement {
    // Try to get children
    let children = match element.attribute(&AXAttribute::children()) {
        Ok(children_array) => children_array,
        Err(_) => return element.clone(),
    };

    let child_count = children.len();
    if child_count == 0 {
        return element.clone();
    }

    // Check each child to see if it contains the point
    for i in 0..child_count {
        if let Some(child) = children.get(i) {
            // Check if child has bounds and contains the point
            if element_contains_point(&child, x, y) {
                // Recursively check this child's children
                return find_leafmost_element_at_position(&child, x, y);
            }
        }
    }

    // No child contains the point, return this element
    element.clone()
}

/// Check if an element's bounds contain a point
fn element_contains_point(element: &AXUIElement, x: f64, y: f64) -> bool {
    use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
    use core_foundation::string::CFString;

    // Get position
    let position_attr = CFString::new(kAXPositionAttribute);
    let ax_position_attr = AXAttribute::new(&position_attr);
    let position = match element
        .attribute(&ax_position_attr)
        .ok()
        .and_then(|p| extract_position(&p))
    {
        Some(pos) => pos,
        None => return false,
    };

    // Get size
    let size_attr = CFString::new(kAXSizeAttribute);
    let ax_size_attr = AXAttribute::new(&size_attr);
    let size = match element
        .attribute(&ax_size_attr)
        .ok()
        .and_then(|s| extract_size(&s))
    {
        Some(sz) => sz,
        None => return false,
    };

    // Check if point is within bounds
    x >= position.0 && x <= position.0 + size.0 && y >= position.1 && y <= position.1 + size.1
}

/// Get the window ID for an element by traversing up to find its parent window.
/// Matches the found AXWindow to a tracked window in WindowManager.
fn get_window_id_for_element(element: &AXUIElement) -> Option<String> {
    use crate::window_manager::WindowManager;

    let mut current = element.clone();

    for _ in 0..20 {
        if let Ok(role) = current.attribute(&AXAttribute::role()) {
            if role.to_string() == "AXWindow" {
                // Found the window - try to match it to a tracked window by bounds
                if let Some(bounds) = get_element_bounds(&current) {
                    if let Some(window_id) = WindowManager::find_window_id_by_bounds(
                        bounds.x, bounds.y, bounds.w, bounds.h,
                    ) {
                        return Some(window_id.0);
                    }
                }
                // Window not tracked by WindowManager - will become orphan
                break;
            }
        }

        match current.attribute(&AXAttribute::parent()) {
            Ok(parent) => current = parent,
            Err(_) => break,
        }
    }

    None
}

// ============================================================================
// AXValue Extraction (merged from ax_value.rs)
// ============================================================================
//
// FFI bindings for AXValue to properly extract CGPoint and CGSize from
// accessibility attributes. This provides safe wrappers around the macOS
// Accessibility API's AXValue functions which are not exposed by the
// `accessibility` crate.

use core_foundation::base::{CFType, CFTypeRef};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use std::os::raw::c_void;

// CGPoint and CGSize structs matching macOS CoreGraphics definitions
#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct CGSize {
    width: f64,
    height: f64,
}

// AXValueType enum values
#[allow(non_upper_case_globals)]
const kAXValueTypeCGPoint: i32 = 1;
#[allow(non_upper_case_globals)]
const kAXValueTypeCGSize: i32 = 2;

// AXValue type (it's actually just a CFTypeRef under the hood)
type AXValueRef = CFTypeRef;

// External declarations for AXValue functions
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXValueGetType(value: AXValueRef) -> i32;
    fn AXValueGetValue(value: AXValueRef, value_type: i32, value_ptr: *mut c_void) -> bool;
    fn AXUIElementCopyElementAtPosition(
        application: accessibility_sys::AXUIElementRef,
        x: f32,
        y: f32,
        element: *mut accessibility_sys::AXUIElementRef,
    ) -> i32;
}

/// Safely extract a CGPoint from a CFType that should be an AXValue
pub fn extract_position(cf_value: &impl TCFType) -> Option<(f64, f64)> {
    unsafe {
        let ax_value: AXValueRef = cf_value.as_CFTypeRef();

        // Check if this is a CGPoint type
        let value_type = AXValueGetType(ax_value);
        if value_type != kAXValueTypeCGPoint {
            return None;
        }

        // Extract the CGPoint
        let mut point = CGPoint { x: 0.0, y: 0.0 };
        let success = AXValueGetValue(
            ax_value,
            kAXValueTypeCGPoint,
            &mut point as *mut CGPoint as *mut c_void,
        );

        if success {
            Some((point.x, point.y))
        } else {
            None
        }
    }
}

/// Safely extract a CGSize from a CFType that should be an AXValue
pub fn extract_size(cf_value: &impl TCFType) -> Option<(f64, f64)> {
    unsafe {
        let ax_value: AXValueRef = cf_value.as_CFTypeRef();

        // Check if this is a CGSize type
        let value_type = AXValueGetType(ax_value);
        if value_type != kAXValueTypeCGSize {
            return None;
        }

        // Extract the CGSize
        let mut size = CGSize {
            width: 0.0,
            height: 0.0,
        };
        let success = AXValueGetValue(
            ax_value,
            kAXValueTypeCGSize,
            &mut size as *mut CGSize as *mut c_void,
        );

        if success {
            Some((size.width, size.height))
        } else {
            None
        }
    }
}

/// Properly extract a typed value from a CFType
/// Handles CFString, CFNumber, CFBoolean, and returns the appropriate typed value
///
/// For certain roles (toggles, checkboxes, radio buttons), 0/1 integers are converted to booleans
pub fn extract_value(cf_value: &impl TCFType, role: Option<&str>) -> Option<AXValue> {
    unsafe {
        let type_ref = cf_value.as_CFTypeRef();
        let cf_type = CFType::wrap_under_get_rule(type_ref);
        let type_id = cf_type.type_of();

        // Try CFString first (most common for values)
        if type_id == CFString::type_id() {
            let cf_string = CFString::wrap_under_get_rule(type_ref as *const _);
            let s = cf_string.to_string();
            // Filter out empty strings
            return if s.is_empty() {
                None
            } else {
                Some(AXValue::String(s))
            };
        }

        // Try CFNumber
        if type_id == CFNumber::type_id() {
            let cf_number = CFNumber::wrap_under_get_rule(type_ref as *const _);

            // For toggle-like elements, convert 0/1 integers to booleans
            if let Some(r) = role {
                if r == "AXToggle"
                    || r == "AXCheckBox"
                    || r == "AXRadioButton"
                    || r.contains("Toggle")
                    || r.contains("CheckBox")
                    || r.contains("RadioButton")
                {
                    if let Some(int_val) = cf_number.to_i64() {
                        return Some(AXValue::Boolean(int_val != 0));
                    }
                }
            }

            // Try to get as i64 first, then f64 if that fails
            if let Some(int_val) = cf_number.to_i64() {
                return Some(AXValue::Integer(int_val));
            } else if let Some(float_val) = cf_number.to_f64() {
                return Some(AXValue::Float(float_val));
            }
        }

        // Try CFBoolean
        if type_id == CFBoolean::type_id() {
            let cf_bool = CFBoolean::wrap_under_get_rule(type_ref as *const _);
            // CFBoolean can be converted to bool via Into trait
            let bool_val: bool = cf_bool.into();
            return Some(AXValue::Boolean(bool_val));
        }

        // For other types, we can't reliably extract them
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cgpoint_size() {
        // Verify that CGPoint has the expected layout
        assert_eq!(std::mem::size_of::<CGPoint>(), 16); // 2 * f64
    }

    #[test]
    fn test_cgsize_size() {
        // Verify that CGSize has the expected layout
        assert_eq!(std::mem::size_of::<CGSize>(), 16); // 2 * f64
    }
}
