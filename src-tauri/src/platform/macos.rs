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

use crate::ax_value::{extract_position, extract_size, extract_value};
use crate::axio::{AXNode, AXRole, Bounds, Position, Size};

/// Generate a stable ID for an element
/// Priority: kAXIdentifierAttribute > role:title:index composite
fn generate_stable_element_id(element: &AXUIElement, pid: u32) -> String {
    // Try kAXIdentifierAttribute first (native stable ID)
    if let Ok(identifier_attr) =
        element.attribute(&AXAttribute::new(&CFString::new("AXIdentifier")))
    {
        if let Some(id_str) = unsafe {
            let cf_string =
                CFString::wrap_under_get_rule(identifier_attr.as_CFTypeRef() as *const _);
            let s = cf_string.to_string();
            if !s.is_empty() {
                Some(s)
            } else {
                None
            }
        } {
            return format!("{}::id:{}", pid, id_str);
        }
    }

    // Fallback: role:title:index composite
    let role: String = element
        .attribute(&AXAttribute::role())
        .ok()
        .and_then(|r| unsafe {
            let cf_string = CFString::wrap_under_get_rule(r.as_CFTypeRef() as *const _);
            Some(cf_string.to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string());

    let title: String = element
        .attribute(&AXAttribute::title())
        .ok()
        .and_then(|t| unsafe {
            let cf_string = CFString::wrap_under_get_rule(t.as_CFTypeRef() as *const _);
            let s = cf_string.to_string();
            if !s.is_empty() {
                Some(s)
            } else {
                None
            }
        })
        .unwrap_or_else(|| "".to_string());

    // Get index among siblings (if we can find parent)
    let index_str = if let Ok(parent) = element.attribute(&AXAttribute::parent()) {
        if let Ok(children) = parent.attribute(&AXAttribute::children()) {
            let element_ref = element.as_concrete_TypeRef();
            let mut found_index = None;
            for i in 0..children.len() {
                if let Some(sibling) = children.get(i) {
                    if std::ptr::eq(
                        element_ref as *const _,
                        sibling.as_concrete_TypeRef() as *const _,
                    ) {
                        found_index = Some(i);
                        break;
                    }
                }
            }
            found_index.map(|i| format!(":{}", i)).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    if title.is_empty() {
        format!("{}::{}:{}{}", pid, role, "untitled", index_str)
    } else {
        // Sanitize title for use in ID
        let safe_title = title
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .take(30)
            .collect::<String>();
        format!("{}::{}:{}{}", pid, role, safe_title, index_str)
    }
}

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
    pid: u32,
    parent_id: Option<String>,
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

    // Register this element and get its UUID
    let element_id = ElementRegistry::register(element.clone());

    // Get role and convert to AXIO format
    let platform_role = element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

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

    // Get title
    let title = element.attribute(&AXAttribute::title()).ok().and_then(|t| {
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
        .and_then(|e| e.try_into().ok())
        .unwrap_or(false);

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
        .and_then(|f| f.try_into().ok())
        .unwrap_or(false);

    // Get selected state (not available in all versions of accessibility crate)
    let selected = None;

    // Get geometry (position and size)
    let bounds = get_element_bounds(element);

    // Get children count (always, regardless of load_children flag)
    let children_count = element
        .attribute(&AXAttribute::children())
        .ok()
        .map(|children_array| children_array.len() as usize)
        .unwrap_or(0);

    // Get children (only if load_children is true)
    let children = if load_children {
        get_element_children(
            element,
            pid,
            Some(element_id.clone()), // Pass element_id as parent_id
            depth,
            max_depth,
            max_children_per_level,
            load_children,
        )
    } else {
        Vec::new()
    };

    Some(AXNode {
        pid,
        element_id: element_id.clone(),
        parent_id,
        path: None,     // Legacy field, no longer used
        id: element_id, // Use element_id as the node ID
        role,
        subrole,
        title,
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
    pid: u32,
    parent_id: Option<String>,
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
                pid,
                parent_id.clone(),
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
pub fn get_window_elements(pid: u32) -> Result<Vec<AXUIElement>, String> {
    use core_foundation::string::CFString;

    let app_element = AXUIElement::application(pid as i32);

    // Get children of the application element
    let children_array = match app_element.attribute(&AXAttribute::children()) {
        Ok(children) => children,
        Err(_) => {
            println!("⚠️  PID {} has no AXChildren attribute", pid);
            return Ok(Vec::new());
        }
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
    pid: u32,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> Result<AXNode, String> {
    element_to_axnode(
        window_element,
        pid,
        None, // Root element has no parent
        0,
        max_depth,
        max_children_per_level,
        load_children,
    )
    .ok_or_else(|| "Failed to get accessibility tree from window element".to_string())
}

/// Get accessibility tree by window ID (uses cached window element)
///
/// This is the preferred method - looks up the window in the cache and uses its element.
pub fn get_ax_tree_by_window_id(
    window_id: &str,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> Result<AXNode, String> {
    use crate::window_manager::WindowManager;

    // Get the cached window (includes the AX element)
    let managed_window = WindowManager::get_window(window_id)
        .ok_or_else(|| format!("Window {} not found in cache", window_id))?;

    // Get the window element
    let window_element = managed_window
        .ax_element
        .ok_or_else(|| format!("Window {} has no AX element", window_id))?;

    // Build tree from the window element (not app element!)
    get_ax_tree_from_element(
        &window_element,
        managed_window.info.process_id,
        max_depth,
        max_children_per_level,
        load_children,
    )
}

/// Get children of a specific node by element ID (NEW - Phase 3)
///
/// Returns the children of the node, with their own children_count populated
/// but not their children (unless max_depth > 1).
pub fn get_children_by_element_id(
    pid: u32,
    element_id: &str,
    max_depth: usize,
    max_children_per_level: usize,
) -> Result<Vec<AXNode>, String> {
    use crate::element_registry::ElementRegistry;

    // Get the element from the registry
    let target_element = ElementRegistry::get(element_id)
        .ok_or_else(|| format!("Element {} not found in registry", element_id))?;

    // Get children of this node
    // Depth = 0 means we're getting immediate children with their counts, but not grandchildren
    let children = get_element_children(
        &target_element,
        pid,
        Some(element_id.to_string()), // Pass element_id as parent_id
        0,                            // Start depth at 0 for children
        max_depth,
        max_children_per_level,
        max_depth > 1, // Only load grandchildren if max_depth > 1
    );

    Ok(children)
}

/// Write text to a specific element (identified by element ID) (NEW - Phase 3)
pub fn write_to_element_by_id(element_id: &str, text: &str) -> Result<(), String> {
    use crate::element_registry::ElementRegistry;

    // Get the element from the registry
    let target_element = ElementRegistry::get(element_id)
        .ok_or_else(|| format!("Element {} not found in registry", element_id))?;

    // Check if element is writable
    let role = target_element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    if !is_writable_role(&role) {
        return Err(format!("Element with role '{}' is not writable", role));
    }

    // Set the value
    let cf_string = CFString::new(text);
    target_element
        .set_value(cf_string.as_CFType())
        .map_err(|e| format!("Failed to set value: {:?}", e))?;

    Ok(())
}

/// Check if a role represents a writable element
fn is_writable_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSecureTextField" | "AXSearchField"
    )
}
