use accessibility::*;
use accessibility_sys::{kAXPositionAttribute, kAXSizeAttribute};
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
    // Position and size for UI element positioning
    pub position: Option<(f64, f64)>, // (x, y) screen coordinates
    pub size: Option<(f64, f64)>,     // (width, height) dimensions
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
    // Use default limits for backward compatibility
    walk_app_tree_by_pid_with_limits(pid, 50, 2000)
}

/// Walk the UI tree of a specific application by PID with configurable limits
pub fn walk_app_tree_by_pid_with_limits(
    pid: u32,
    max_depth: usize,
    max_children_per_level: usize,
) -> Result<UITreeNode, String> {
    // Create AXUIElement for the specific application using PID
    let app_element = AXUIElement::application(pid as i32);

    // Walk the tree starting from this application with configurable limits
    walk_element_tree(&app_element, 0, max_depth, max_children_per_level, &[])
}

/// Walk the tree starting from a specific element
fn walk_element_tree(
    element: &AXUIElement,
    depth: usize,
    max_depth: usize,
    max_children_per_level: usize,
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
            position: None,
            size: None,
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

    // Extract position and size using accessibility-sys constants
    let position = {
        // Create CFString from the string constant
        let position_attr = CFString::new(kAXPositionAttribute);
        let ax_position_attr = AXAttribute::new(&position_attr);

        element
            .attribute(&ax_position_attr)
            .ok()
            .and_then(|pos_value| {
                // The position is typically a CGPoint structure
                // Try to extract x,y coordinates from the value
                let value_str = format!("{:?}", pos_value);

                // Look for coordinate patterns in the debug output
                if let Some(x_start) = value_str.find("x:") {
                    if let Some(y_start) = value_str.find("y:") {
                        let x_part = &value_str[x_start + 2..];
                        let y_part = &value_str[y_start + 2..];

                        let x_end = x_part
                            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                            .unwrap_or(x_part.len());
                        let y_end = y_part
                            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                            .unwrap_or(y_part.len());

                        let x = x_part[..x_end].trim().parse::<f64>().ok()?;
                        let y = y_part[..y_end].trim().parse::<f64>().ok()?;

                        Some((x, y))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
    };

    let size = {
        // Create CFString from the string constant
        let size_attr = CFString::new(kAXSizeAttribute);
        let ax_size_attr = AXAttribute::new(&size_attr);

        element
            .attribute(&ax_size_attr)
            .ok()
            .and_then(|size_value| {
                // The size is typically a CGSize structure
                // Try to extract width,height from the value
                let value_str = format!("{:?}", size_value);

                // Look for dimension patterns in the debug output
                if let Some(w_start) = value_str.find("width:").or_else(|| value_str.find("w:")) {
                    if let Some(h_start) =
                        value_str.find("height:").or_else(|| value_str.find("h:"))
                    {
                        let w_offset = if value_str[w_start..].starts_with("width:") {
                            6
                        } else {
                            2
                        };
                        let h_offset = if value_str[h_start..].starts_with("height:") {
                            7
                        } else {
                            2
                        };

                        let w_part = &value_str[w_start + w_offset..];
                        let h_part = &value_str[h_start + h_offset..];

                        let w_end = w_part
                            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                            .unwrap_or(w_part.len());
                        let h_end = h_part
                            .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
                            .unwrap_or(h_part.len());

                        let width = w_part[..w_end].trim().parse::<f64>().ok()?;
                        let height = h_part[..h_end].trim().parse::<f64>().ok()?;

                        Some((width, height))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
    };

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

        // Log when we hit the child limit
        if child_count > max_children_per_level as isize {
            println!(
                "⚠️ Hit child limit: {} children at depth {}, showing first {}",
                child_count, depth, max_children_per_level
            );
        }

        for i in 0..child_count.min(max_children_per_level as isize) {
            if let Some(child_ref) = child_elements.get(i) {
                let mut child_path = current_path.to_vec();
                child_path.push(i as usize);

                if let Ok(child_node) = walk_element_tree(
                    &(*child_ref),
                    depth + 1,
                    max_depth,
                    max_children_per_level,
                    &child_path,
                ) {
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
        position,
        size,
    })
}
