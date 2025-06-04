use accessibility::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UITreeNode {
    pub role: String,
    pub title: Option<String>,
    pub value: Option<String>,
    pub enabled: bool,
    pub children: Vec<UITreeNode>,
    pub depth: usize,
}

/// Walk the UI tree of a specific application by PID
pub fn walk_app_tree_by_pid(pid: u32) -> Result<UITreeNode, String> {
    // Create AXUIElement for the specific application using PID
    let app_element = AXUIElement::application(pid as i32);

    // Walk the tree starting from this application
    walk_element_tree(&app_element, 0, 500) // Much higher depth for debugging
}

/// Walk the UI tree of the currently focused application (fallback method)
pub fn walk_focused_app_tree() -> Result<UITreeNode, String> {
    let system_element = AXUIElement::system_wide();

    // Try to get the focused window
    match system_element.attribute(&AXAttribute::focused_window()) {
        Ok(focused_window) => {
            // Walk the tree starting from the focused window
            walk_element_tree(&focused_window, 0, 500) // Much higher depth for debugging
        }
        Err(e) => Err(format!(
            "Failed to get focused window: {:?}. Try using walk_app_tree_by_pid instead.",
            e
        )),
    }
}

/// Walk the tree starting from a specific element
fn walk_element_tree(
    element: &AXUIElement,
    depth: usize,
    max_depth: usize,
) -> Result<UITreeNode, String> {
    if depth > max_depth {
        return Ok(UITreeNode {
            role: "MAX_DEPTH_REACHED".to_string(),
            title: None,
            value: None,
            enabled: false,
            children: vec![],
            depth,
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
        // Filter out empty, null, or weird debug formatting
        if debug_str.is_empty() || debug_str == "null" || debug_str.contains("{contents = \"\"}") {
            None
        } else {
            Some(debug_str)
        }
    });

    let enabled = element
        .attribute(&AXAttribute::enabled())
        .ok()
        .and_then(|e| e.try_into().ok())
        .unwrap_or(false);

    // Get children
    let mut children = Vec::new();
    if let Ok(child_elements) = element.attribute(&AXAttribute::children()) {
        let child_count = child_elements.len();

        // Show way more children for debugging
        for i in 0..child_count.min(100) {
            if let Some(child) = child_elements.get(i) {
                if let Ok(child_node) = walk_element_tree(&child, depth + 1, max_depth) {
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
    })
}

/// Find all text input elements by walking the tree using PID
// pub fn find_text_elements_by_pid(pid: u32) -> Result<Vec<UITreeNode>, String> {
//     let tree = walk_app_tree_by_pid(pid)?;
//     let mut text_elements = Vec::new();
//     collect_text_elements(&tree, &mut text_elements);
//     Ok(text_elements)
// }

/// Find all text input elements by walking the tree  
pub fn find_text_elements() -> Result<Vec<UITreeNode>, String> {
    // This will try the focused window approach and may fail
    // Better to use find_text_elements_by_pid with actual PID
    let tree = walk_focused_app_tree()?;
    let mut text_elements = Vec::new();
    collect_text_elements(&tree, &mut text_elements);
    Ok(text_elements)
}

/// Recursively collect text input elements from the tree
fn collect_text_elements(node: &UITreeNode, elements: &mut Vec<UITreeNode>) {
    // Check if this is a text input element
    let text_roles = [
        "AXTextField",
        "AXTextArea",
        "AXComboBox",
        "AXSearchField",
        "AXSecureTextField",
    ];
    if text_roles.contains(&node.role.as_str()) {
        elements.push(node.clone());
    }

    // Recursively check children
    for child in &node.children {
        collect_text_elements(child, elements);
    }
}

/// Insert text into the active text field (simplified for now)
pub fn insert_text_into_active_field(text: &str) -> Result<(), String> {
    println!("Text insertion requested: {}", text);
    // For now, just log that we received the request
    // Real implementation would find the focused text field and set its value
    Ok(())
}

/// Placeholder implementations to satisfy the existing API
pub fn find_text_elements_in_app(_app_name: &str) -> Result<Vec<UITreeNode>, String> {
    find_text_elements()
}

pub fn insert_text_into_element(
    _app_name: &str,
    _element_id: &str,
    text: &str,
) -> Result<(), String> {
    insert_text_into_active_field(text)
}
