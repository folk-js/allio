use accessibility::*;
use core_foundation::base::TCFType;
use core_foundation::string::CFString;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UITreeNode {
    pub role: String,
    pub title: Option<String>,
    pub value: Option<String>,
    pub enabled: bool,
    pub children: Vec<UITreeNode>,
    pub depth: usize,
    // Additional attributes for richer information
    pub description: Option<String>,
    pub help: Option<String>,
    pub placeholder: Option<String>,
    pub role_description: Option<String>,
    pub subrole: Option<String>,
    pub focused: Option<bool>,
    pub selected: Option<bool>,
    pub selected_text: Option<String>,
    pub character_count: Option<usize>,
    // Add element ID for write operations
    pub element_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccessibilityEvent {
    pub event_type: String,
    pub element_role: String,
    pub element_title: Option<String>,
    pub element_value: Option<String>,
    pub timestamp: u64,
}

/// Write text to a specific accessibility element in an application
pub fn write_to_element_by_pid_and_path(
    pid: u32,
    element_path: &[usize],
    text: &str,
) -> Result<(), String> {
    // Create AXUIElement for the specific application using PID
    let app_element = AXUIElement::application(pid as i32);

    // Navigate to the target element using the path
    let target_element = match navigate_to_element(&app_element, element_path) {
        Some(element) => element,
        None => return Err("Could not find target element".to_string()),
    };

    // Check if the element supports value setting
    let role = target_element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    // Only allow writing to text input elements
    if !is_writable_element(&role) {
        return Err(format!("Element with role '{}' is not writable", role));
    }

    // Set the value using set_value
    let cf_string = CFString::new(text);
    match target_element.set_value(cf_string.as_CFType()) {
        Ok(_) => {
            println!("✅ Successfully wrote '{}' to {} element", text, role);
            Ok(())
        }
        Err(e) => {
            let error_msg = format!("Failed to set value: {:?}", e);
            println!("❌ {}", error_msg);
            Err(error_msg)
        }
    }
}

/// Navigate to a specific element using a path of indices
fn navigate_to_element(element: &AXUIElement, path: &[usize]) -> Option<AXUIElement> {
    let mut current_element = element.clone();

    for &index in path {
        if let Ok(children) = current_element.attribute(&AXAttribute::children()) {
            let child_count = children.len();
            if (index as isize) < child_count {
                if let Some(child_ref) = children.get(index as isize) {
                    current_element = (*child_ref).clone();
                } else {
                    return None;
                }
            } else {
                return None;
            }
        } else {
            return None;
        }
    }

    Some(current_element)
}

/// Check if an element role is writable
fn is_writable_element(role: &str) -> bool {
    matches!(
        role,
        "AXTextField" | "AXTextArea" | "AXComboBox" | "AXSecureTextField"
    )
}

/// Walk the UI tree of a specific application by PID
pub fn walk_app_tree_by_pid(pid: u32) -> Result<UITreeNode, String> {
    // Create AXUIElement for the specific application using PID
    let app_element = AXUIElement::application(pid as i32);

    // Walk the tree starting from this application
    walk_element_tree(&app_element, 0, 100, &[])
}

/// Walk the tree starting from a specific element
fn walk_element_tree(
    element: &AXUIElement,
    depth: usize,
    max_depth: usize,
    current_path: &[usize],
) -> Result<UITreeNode, String> {
    if depth > max_depth {
        return Ok(UITreeNode {
            role: "MAX_DEPTH_REACHED".to_string(),
            title: None,
            value: None,
            enabled: false,
            children: vec![],
            depth,
            description: None,
            help: None,
            placeholder: None,
            role_description: None,
            subrole: None,
            focused: None,
            selected: None,
            selected_text: None,
            character_count: None,
            element_id: None,
        });
    }

    // Get basic attributes
    let role = element
        .attribute(&AXAttribute::role())
        .ok()
        .map(|r| r.to_string())
        .unwrap_or_else(|| "Unknown".to_string());

    let title = element.attribute(&AXAttribute::title()).ok().and_then(|t| {
        let s = t.to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });

    let value = element.attribute(&AXAttribute::value()).ok().and_then(|v| {
        let debug_str = format!("{:?}", v);
        // Strip quotes and filter out empty, null, or weird formatting
        if debug_str.is_empty() || debug_str == "null" || debug_str.contains("{contents = \"\"}") {
            None
        } else {
            // Remove surrounding quotes if present
            let clean_str =
                if debug_str.starts_with('"') && debug_str.ends_with('"') && debug_str.len() > 1 {
                    debug_str[1..debug_str.len() - 1].to_string()
                } else {
                    debug_str
                };
            Some(clean_str)
        }
    });

    let enabled = element
        .attribute(&AXAttribute::enabled())
        .ok()
        .and_then(|e| e.try_into().ok())
        .unwrap_or(false);

    // Extract additional attributes for richer information
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

    let help = element.attribute(&AXAttribute::help()).ok().and_then(|h| {
        let s = h.to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });

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

    let role_description = element
        .attribute(&AXAttribute::role_description())
        .ok()
        .and_then(|rd| {
            let s = rd.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let subrole = element
        .attribute(&AXAttribute::subrole())
        .ok()
        .and_then(|sr| {
            let s = sr.to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        });

    let focused = element
        .attribute(&AXAttribute::focused())
        .ok()
        .and_then(|f| f.try_into().ok());

    // Note: Some attributes may not be available in all versions of the accessibility crate
    let selected = None; // AXAttribute::selected() not available
    let selected_text = None; // AXAttribute::selected_text() not available
    let character_count = None; // AXAttribute::number_of_characters() not available

    // Generate element ID from path for write operations
    let element_id = if is_writable_element(&role) {
        Some(
            current_path
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join("-"),
        )
    } else {
        None
    };

    // Get children
    let mut children = Vec::new();
    if let Ok(child_elements) = element.attribute(&AXAttribute::children()) {
        let child_count = child_elements.len();

        for i in 0..child_count.min(50) {
            if let Some(child_ref) = child_elements.get(i) {
                let mut child_path = current_path.to_vec();
                child_path.push(i as usize);

                if let Ok(child_node) =
                    walk_element_tree(&(*child_ref), depth + 1, max_depth, &child_path)
                {
                    children.push(child_node);
                }
            }
        }
    }

    Ok(UITreeNode {
        role,
        title,
        value,
        enabled,
        children,
        depth,
        description,
        help,
        placeholder,
        role_description,
        subrole,
        focused,
        selected,
        selected_text,
        character_count,
        element_id,
    })
}
