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

/// Convert macOS AXUIElement to AXIO AXNode
///
/// This is the main conversion function that maps macOS accessibility
/// elements to our platform-agnostic AXIO format.
///
/// If `load_children` is false, children_count is populated but children array is empty.
pub fn element_to_axnode(
    element: &AXUIElement,
    pid: u32,
    path: Vec<usize>,
    depth: usize,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> Option<AXNode> {
    // Stop traversal past max depth
    // Note: max_depth is inclusive (max_depth=1 means depths 0 and 1 are allowed)
    if depth > max_depth {
        return None;
    }

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

    // Generate unique ID for this element
    // Note: In the future we might use proper accessibility identifiers if available
    // For now, using depth and a random ID
    let id = format!("depth{}-{}", depth, uuid::Uuid::new_v4().simple());

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
            path.clone(),
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
        path,
        id,
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
    parent_path: Vec<usize>,
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
            // Build path for this child
            let mut child_path = parent_path.clone();
            child_path.push(i as usize);

            if let Some(child_node) = element_to_axnode(
                &(*child_ref),
                pid,
                child_path,
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
pub fn get_ax_tree_by_pid(
    pid: u32,
    max_depth: usize,
    max_children_per_level: usize,
    load_children: bool,
) -> Result<AXNode, String> {
    let app_element = AXUIElement::application(pid as i32);

    element_to_axnode(
        &app_element,
        pid,
        vec![],
        0,
        max_depth,
        max_children_per_level,
        load_children,
    )
    .ok_or_else(|| format!("Failed to get accessibility tree for PID {}", pid))
}

/// Get children of a specific node by path
///
/// Returns the children of the node at the given path, with their own children_count populated
/// but not their children (unless max_depth > 1).
pub fn get_children_by_path(
    pid: u32,
    path: &[usize],
    max_depth: usize,
    max_children_per_level: usize,
) -> Result<Vec<AXNode>, String> {
    let app_element = AXUIElement::application(pid as i32);

    // Navigate to the target node
    let target_element = navigate_to_element(&app_element, path)
        .ok_or_else(|| "Could not find target element".to_string())?;

    // Get children of this node
    // Depth = 0 means we're getting immediate children with their counts, but not grandchildren
    let children = get_element_children(
        &target_element,
        pid,
        path.to_vec(),
        0, // Start depth at 0 for children
        max_depth,
        max_children_per_level,
        max_depth > 1, // Only load grandchildren if max_depth > 1
    );

    Ok(children)
}

/// Write text to a specific element (identified by path through the tree)
///
/// Path is a sequence of child indices from root to target element.
pub fn write_to_element(pid: u32, element_path: &[usize], text: &str) -> Result<(), String> {
    let app_element = AXUIElement::application(pid as i32);

    // Navigate to target element
    let target_element = navigate_to_element(&app_element, element_path)
        .ok_or_else(|| "Could not find target element".to_string())?;

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

/// Navigate to an element using a path of child indices
fn navigate_to_element(root: &AXUIElement, path: &[usize]) -> Option<AXUIElement> {
    let mut current = root.clone();

    for &index in path {
        let children = current.attribute(&AXAttribute::children()).ok()?;

        if index >= children.len() as usize {
            return None;
        }

        let child_ref = children.get(index as isize)?;
        current = (*child_ref).clone();
    }

    Some(current)
}

/// Check if a role represents a writable element
fn is_writable_role(role: &str) -> bool {
    matches!(
        role,
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSecureTextField" | "AXSearchField"
    )
}
