use serde::{Deserialize, Serialize};
use std::{
    panic::{self},
    thread,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

use crate::axio::{AXNode, AXRole, Bounds, Position, Size};
use crate::websocket::WebSocketState;

// Constants - optimized polling rate
const POLLING_INTERVAL_MS: u64 = 8; // ~120 FPS - fast enough for smooth tracking

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: String,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub focused: bool,
    pub process_id: u32,
}

// Convert x-win WindowInfo to our WindowInfo struct
impl WindowInfo {
    fn from_x_win(window: &x_win::WindowInfo, focused: bool) -> Self {
        WindowInfo {
            id: window.id.to_string(),
            name: window.title.clone(),
            x: window.position.x,
            y: window.position.y,
            w: window.position.width,
            h: window.position.height,
            focused,
            process_id: window.info.process_id,
        }
    }
}

impl WindowInfo {
    fn with_offset(mut self, offset_x: i32, offset_y: i32) -> Self {
        self.x -= offset_x;
        self.y -= offset_y;
        self
    }

    /// Convert WindowInfo to AXNode
    /// Windows are just root-level accessibility nodes with no children loaded
    /// Returns None if we can't get the actual children count from the accessibility API
    pub fn to_ax_node(&self) -> Option<AXNode> {
        use accessibility::*;

        // Get the actual app element to query children count
        let app_element = AXUIElement::application(self.process_id as i32);

        // Get actual children count from accessibility API
        let children_count = app_element
            .attribute(&AXAttribute::children())
            .ok()
            .map(|children_array| children_array.len() as usize)
            .unwrap_or(0);

        Some(AXNode {
            pid: self.process_id,
            path: vec![], // Windows are root nodes (empty path)
            id: self.id.clone(),
            role: AXRole::Window,
            subrole: None,
            title: if !self.name.is_empty() {
                Some(self.name.clone())
            } else {
                None
            },
            value: None,
            description: None,
            placeholder: None,
            focused: self.focused,
            enabled: true, // Windows are always enabled
            selected: None,
            bounds: Some(Bounds {
                position: Position {
                    x: self.x as f64,
                    y: self.y as f64,
                },
                size: Size {
                    width: self.w as f64,
                    height: self.h as f64,
                },
            }),
            children_count,   // Actual count from accessibility API
            children: vec![], // No children loaded initially
        })
    }
}

// Event payload structures
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowUpdatePayload {
    pub windows: Vec<AXNode>,
}

#[cfg(target_os = "macos")]
pub fn get_main_screen_dimensions() -> (f64, f64) {
    unsafe {
        let display_id: CGDirectDisplayID = CGMainDisplayID();
        let width = CGDisplayPixelsWide(display_id) as f64;
        let height = CGDisplayPixelsHigh(display_id) as f64;
        (width, height)
    }
}

// Combined function to get all windows with focused state in single call
pub fn get_all_windows_with_focus() -> Vec<WindowInfo> {
    let current_pid = std::process::id();

    // Get all windows and active window in parallel
    let all_windows_result = panic::catch_unwind(|| x_win::get_open_windows());
    let active_window_result = panic::catch_unwind(|| x_win::get_active_window());

    let (all_windows, active_window_id) = match (all_windows_result, active_window_result) {
        (Ok(Ok(windows)), Ok(Ok(active))) => (windows, Some(active.id)),
        (Ok(Ok(windows)), _) => (windows, None),
        _ => return Vec::new(),
    };

    // Find overlay offset
    let overlay_offset = all_windows
        .iter()
        .find(|w| w.info.process_id == current_pid)
        .map(|w| (w.position.x, w.position.y))
        .unwrap_or((0, 0));

    // Convert all windows, excluding our overlay, and mark focused
    all_windows
        .iter()
        .filter(|w| w.info.process_id != current_pid)
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            WindowInfo::from_x_win(w, focused).with_offset(overlay_offset.0, overlay_offset.1)
        })
        .collect()
}

// WebSocket-only polling loop
pub fn window_polling_loop(ws_state: WebSocketState) {
    let mut last_windows: Option<Vec<WindowInfo>> = None;

    loop {
        let loop_start = Instant::now();

        // Get fresh data from system
        let current_windows = get_all_windows_with_focus();

        // No longer auto-push tree on focus change - let overlays request what they need

        // Broadcast window updates if something changed
        if last_windows.as_ref() != Some(&current_windows) {
            // Convert windows to AXNodes (filter out any that fail to convert)
            let window_nodes: Vec<AXNode> = current_windows
                .iter()
                .filter_map(|w| w.to_ax_node())
                .collect();

            // Update WebSocket state and broadcast
            let ws_state_clone = ws_state.clone();
            let windows_clone = current_windows.clone();

            tokio::spawn(async move {
                ws_state_clone.update_windows(&windows_clone).await;
            });

            ws_state.broadcast(&WindowUpdatePayload {
                windows: window_nodes,
            });

            last_windows = Some(current_windows);
        }

        // Precise interval handling
        let elapsed = loop_start.elapsed();
        let target_interval = Duration::from_millis(POLLING_INTERVAL_MS);
        if elapsed < target_interval {
            thread::sleep(target_interval - elapsed);
        }
    }
}
