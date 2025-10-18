use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    panic::{self},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

// Cache for bundle ID lookups (PID -> Bundle ID)
static BUNDLE_ID_CACHE: Mutex<Option<HashMap<u32, Option<String>>>> = Mutex::new(None);

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

    /// Check if window is fullscreen (covers entire screen with no chrome)
    ///
    /// NOTE: We currently filter out ALL fullscreen windows to avoid issues with
    /// screenshot/recording UIs. This might need to change in the future if we want
    /// to support overlays on fullscreen apps (games, videos, etc).
    #[cfg(target_os = "macos")]
    fn is_fullscreen(&self) -> bool {
        let (screen_width, screen_height) = get_main_screen_dimensions();

        // Check if window covers the entire screen exactly
        self.x == 0
            && self.y == 0
            && (self.w as f64) == screen_width
            && (self.h as f64) == screen_height
    }

    #[cfg(not(target_os = "macos"))]
    fn is_fullscreen(&self) -> bool {
        false
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

/// List of bundle identifiers to filter out from window list
const FILTERED_BUNDLE_IDS: &[&str] = &[
    "com.apple.screencaptureui", // Screenshot UI
    "com.apple.screenshot.launcher",
    "com.apple.ScreenContinuity", // Screen recording UI
    "com.apple.QuickTimePlayerX", // QuickTime recording (optional - user might want this)
];

/// Get bundle ID for a PID, with caching
#[cfg(target_os = "macos")]
fn get_bundle_id(pid: u32) -> Option<String> {
    use std::process::Command;

    // Check cache first
    {
        let cache = BUNDLE_ID_CACHE.lock().unwrap();
        if let Some(ref map) = *cache {
            if let Some(cached) = map.get(&pid) {
                return cached.clone();
            }
        }
    }

    // Not in cache, query it
    let output = Command::new("lsappinfo")
        .args(&["info", "-only", "bundleid", &format!("{}", pid)])
        .output();

    let bundle_id = if let Ok(output) = output {
        if let Ok(info) = String::from_utf8(output.stdout) {
            // Output format: 'bundleid="com.apple.screencaptureui"' or '"CFBundleIdentifier"="com.apple.screencaptureui"'
            // Find the last "=" to handle both formats
            if let Some(eq_pos) = info.rfind('=') {
                let after_eq = &info[eq_pos + 1..];
                // Now extract the quoted value after the =
                if let Some(start) = after_eq.find('"') {
                    if let Some(end) = after_eq[start + 1..].find('"') {
                        let id = after_eq[start + 1..start + 1 + end].to_string();
                        println!("📋 PID {} has bundle ID: {}", pid, id);
                        Some(id)
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Store in cache
    {
        let mut cache = BUNDLE_ID_CACHE.lock().unwrap();
        if cache.is_none() {
            *cache = Some(HashMap::new());
        }
        cache.as_mut().unwrap().insert(pid, bundle_id.clone());
    }

    bundle_id
}

/// Check if a process should be filtered by its bundle identifier
#[cfg(target_os = "macos")]
fn should_filter_process(pid: u32) -> bool {
    if let Some(bundle_id) = get_bundle_id(pid) {
        // Check if this bundle ID is in our filter list
        for filtered_id in FILTERED_BUNDLE_IDS {
            if bundle_id == *filtered_id {
                return true;
            }
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
fn should_filter_process(_pid: u32) -> bool {
    false
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

    // Convert all windows, excluding our overlay, filtered apps, and fullscreen windows
    all_windows
        .iter()
        .filter(|w| w.info.process_id != current_pid && !should_filter_process(w.info.process_id))
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            WindowInfo::from_x_win(w, focused).with_offset(overlay_offset.0, overlay_offset.1)
        })
        .filter(|w| !w.is_fullscreen()) // Also filter out fullscreen windows
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
