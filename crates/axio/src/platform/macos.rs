/**
 * macOS Platform Implementation
 *
 * Converts macOS Accessibility API elements directly to AXIO format.
 * All macOS-specific knowledge is encapsulated here.
 */
use accessibility::*;
use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
use core_foundation::base::TCFType;
use core_foundation::string::CFString;

use crate::types::{
    AXNode, AXRole, AXValue, AxioError, AxioResult, Bounds, ElementId, Position, Size, WindowId,
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

use crate::types::ElementUpdate;
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

fn handle_notification(element_id: &ElementId, notification: &str, element: &AXUIElement) {
    use crate::element_registry::ElementRegistry;

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

                extract_value(&value_attr, role.as_deref()).map(|value| {
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

// ============================================================================
// AXUIElement to AXNode Conversion
// ============================================================================

/// Convert macOS AXUIElement to AXIO AXNode
///
/// This is the main conversion function that maps macOS accessibility
/// elements to our platform-agnostic AXIO format.
///
/// Phase 3: Now registers elements in ElementRegistry and uses element_id instead of path.
///
/// If `load_children` is false, children_count is populated but children array is empty.
pub fn element_to_axnode(
    element: &AXUIElement,
    window_id: &WindowId,
    pid: u32,
    parent_id: Option<&ElementId>,
    depth: usize,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> Option<AXNode> {
    use crate::element_registry::ElementRegistry;

    // Stop traversal past max depth
    // Note: max_depth is inclusive (max_depth=1 means depths 0 and 1 are allowed)
    if depth > max_depth {
        return None;
    }

    // Get role for registration
    let platform_role = element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    // Register this element and get its UUID
    let element_id =
        ElementRegistry::register(element.clone(), window_id, pid, parent_id, &platform_role);

    // Convert role to AXIO format
    let role = map_platform_role(&platform_role);

    // Get subrole (platform-specific subtype)
    // For unknown roles, use the platform role as the subrole for debugging
    let subrole = if matches!(role, AXRole::Unknown) {
        Some(platform_role.clone())
    } else {
        element
            .attribute(&AXAttribute::subrole())
            .ok()
            .map(|sr| sr.to_string())
            .filter(|s| !s.is_empty())
    };

    // Get label (ARIA term for title/label attribute)
    let label = element.attribute(&AXAttribute::title()).ok().and_then(|t| {
        let s = t.to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });

    // Get value (with role context for proper type conversion)
    let value = element
        .attribute(&AXAttribute::value())
        .ok()
        .and_then(|v| extract_value(&v, Some(&platform_role)));

    // Get enabled state
    let enabled = element
        .attribute(&AXAttribute::enabled())
        .ok()
        .and_then(|e| e.try_into().ok());

    // Get description
    let description = element
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

    // Get placeholder text
    let placeholder = element
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

    // Get focused state
    let focused = element
        .attribute(&AXAttribute::focused())
        .ok()
        .and_then(|f| f.try_into().ok());

    // Get selected state (not available in all versions of accessibility crate)
    let selected = None;

    // Get geometry (position and size)
    let bounds = get_element_bounds(element);

    // Get children count (always, regardless of load_children flag)
    let children_count = element
        .attribute(&AXAttribute::children())
        .ok()
        .map(|children_array| children_array.len() as usize);

    // Get children (only if load_children is true)
    let children = if load_children {
        Some(get_element_children(
            element,
            window_id,
            pid,
            Some(&element_id), // Pass element_id as parent_id
            depth,
            max_depth,
            max_children_per_level,
            load_children,
        ))
    } else {
        None
    };

    Some(AXNode {
        id: element_id,
        parent_id: parent_id.cloned(),
        role,
        subrole,
        label,
        value,
        description,
        placeholder,
        focused,
        enabled,
        selected,
        bounds,
        children_count,
        children,
    })
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
        position: Position {
            x: position.0,
            y: position.1,
        },
        size: Size {
            width: size.0,
            height: size.1,
        },
    })
}

/// Get children of an element, recursively converting to AXNode
fn get_element_children(
    element: &AXUIElement,
    window_id: &WindowId,
    pid: u32,
    parent_id: Option<&ElementId>,
    depth: usize,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> Vec<AXNode> {
    let children_array = match element.attribute(&AXAttribute::children()) {
        Ok(children) => children,
        Err(_) => return Vec::new(),
    };

    let child_count = children_array.len();
    let mut result = Vec::new();

    for i in 0..child_count.min(max_children_per_level as isize) {
        if let Some(child_ref) = children_array.get(i) {
            if let Some(child_node) = element_to_axnode(
                &(*child_ref),
                window_id,
                pid,
                parent_id,
                depth + 1,
                max_depth,
                max_children_per_level,
                load_children,
            ) {
                result.push(child_node);
            }
        }
    }

    result
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

/// Get accessibility tree using a window element from the cache
///
/// This is the NEW approach - uses the cached window element as root.
/// The window element is the correct root for a window's accessibility tree.
pub fn get_ax_tree_from_element(
    window_element: &AXUIElement,
    window_id: &WindowId,
    pid: u32,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> AxioResult<AXNode> {
    element_to_axnode(
        window_element,
        window_id,
        pid,
        None, // Root element has no parent
        0,
        max_depth,
        max_children_per_level,
        load_children,
    )
    .ok_or_else(|| AxioError::Internal("Failed to convert window element to AXNode".to_string()))
}

/// Get accessibility tree by window ID (uses cached window element)
///
/// This is the preferred method - looks up the window in the cache and uses its element.
pub fn get_ax_tree_by_window_id(
    window_id: &WindowId,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> AxioResult<AXNode> {
    use crate::window_manager::WindowManager;

    // Get the cached window (includes the AX element)
    let managed_window = WindowManager::get_window(window_id)
        .ok_or_else(|| AxioError::WindowNotFound(window_id.clone()))?;

    // Get the window element
    let window_element = managed_window
        .ax_element
        .ok_or_else(|| AxioError::Internal(format!("Window {} has no AX element", window_id)))?;

    // Build tree from the window element (not app element!)
    get_ax_tree_from_element(
        &window_element,
        window_id,
        managed_window.info.process_id,
        max_depth,
        max_children_per_level,
        load_children,
    )
}

/// Get children of a specific node by element ID
///
/// Returns the children of the node, with their own children_count populated
/// but not their children (unless max_depth > 1).
pub fn get_children_by_element_id(
    element_id: &str,
    max_depth: usize,
    max_children_per_level: usize,
) -> AxioResult<Vec<AXNode>> {
    use crate::element_registry::ElementRegistry;

    let element_id = ElementId::new(element_id.to_string());

    // Get the window_id and pid from the element
    let (ax_element, window_id, pid) = ElementRegistry::with_element(&element_id, |element| {
        (
            element.ax_element().clone(),
            element.window_id().clone(),
            element.pid(),
        )
    })?;

    // Get children of this node
    // Depth = 0 means we're getting immediate children with their counts, but not grandchildren
    let children = get_element_children(
        &ax_element,
        &window_id,
        pid,
        Some(&element_id), // Pass element_id as parent_id
        0,                 // Start depth at 0 for children
        max_depth,
        max_children_per_level,
        max_depth > 1, // Only load grandchildren if max_depth > 1
    );

    Ok(children)
}

/// Click/press a specific element (identified by element ID)
/// Performs the AXPress action on the element
pub fn click_element_by_id(element_id: &str) -> AxioResult<()> {
    use crate::element_registry::ElementRegistry;
    use accessibility_sys::{kAXPressAction, AXUIElementPerformAction};
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    let element_id = ElementId::new(element_id.to_string());

    // Get the element from registry and perform press action
    ElementRegistry::with_element(&element_id, |element| {
        let ax_element = element.ax_element();
        let action = CFString::new(kAXPressAction);

        unsafe {
            let result = AXUIElementPerformAction(
                ax_element.as_concrete_TypeRef(),
                action.as_concrete_TypeRef(),
            );
            if result == 0 {
                Ok(())
            } else {
                Err(AxioError::AccessibilityError(format!(
                    "AXUIElementPerformAction failed with code {}",
                    result
                )))
            }
        }
    })?
}

/// Get the accessibility element at a specific screen position.
///
/// The element must belong to a tracked window. Returns an error if the
/// element's window is not being tracked by WindowManager.
pub fn get_element_at_position(x: f64, y: f64) -> AxioResult<AXNode> {
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

        let mut element = AXUIElement::wrap_under_create_rule(element_ref);

        let mut pid: i32 = 0;
        let pid_result = AXUIElementGetPid(element_ref, &mut pid);
        if pid_result != 0 {
            return Err(AxioError::AccessibilityError(format!(
                "Failed to get PID for element at ({}, {})",
                x, y
            )));
        }

        // Traverse down to find the leafmost (deepest) element
        element = find_leafmost_element_at_position(&element, x, y);

        // Element must belong to a tracked window
        let window_id_str = get_window_id_for_element(&element).ok_or_else(|| {
            AxioError::WindowNotFound(WindowId::new(format!(
                "untracked-window-at-{:.0}-{:.0}",
                x, y
            )))
        })?;
        let window_id = WindowId::new(window_id_str);

        element_to_axnode(&element, &window_id, pid as u32, None, 0, 0, 100, false).ok_or_else(
            || {
                AxioError::Internal(format!(
                    "Failed to convert element at ({}, {}) to AXNode",
                    x, y
                ))
            },
        )
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
                        bounds.position.x,
                        bounds.position.y,
                        bounds.size.width,
                        bounds.size.height,
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
