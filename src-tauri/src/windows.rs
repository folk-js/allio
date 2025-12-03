use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    panic::{self},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

// Cache for bundle ID lookups (PID -> Bundle ID)
// Using once_cell::Lazy to avoid Option wrapper and simplify initialization
static BUNDLE_ID_CACHE: Lazy<Mutex<HashMap<u32, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

use crate::websocket::WebSocketState;

// Constants - optimized polling rate
const POLLING_INTERVAL_MS: u64 = 8; // ~120 FPS - fast enough for smooth tracking

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_name: String,
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
            title: window.title.clone(),
            app_name: window.info.name.clone(),
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
    fn with_offset(mut self, offset_x: i32, offset_y: i32) -> Self {
        self.x -= offset_x;
        self.y -= offset_y;
        self
    }

    #[cfg(not(target_os = "macos"))]
    fn is_fullscreen(&self) -> bool {
        false
    }
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

/// Parse bundle ID from lsappinfo output
/// Handles formats: 'bundleid="com.example"' or '"CFBundleIdentifier"="com.example"'
#[cfg(target_os = "macos")]
fn parse_bundle_id(info: &str) -> Option<String> {
    let eq_pos = info.rfind('=')?;
    let after_eq = &info[eq_pos + 1..];
    let start = after_eq.find('"')?;
    let end = after_eq[start + 1..].find('"')?;
    Some(after_eq[start + 1..start + 1 + end].to_string())
}

/// Get bundle ID for a PID, with caching
#[cfg(target_os = "macos")]
fn get_bundle_id(pid: u32) -> Option<String> {
    use std::process::Command;

    // Check cache first
    {
        let cache = BUNDLE_ID_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&pid) {
            return cached.clone();
        }
    }

    // Query bundle ID from lsappinfo
    let bundle_id = Command::new("lsappinfo")
        .args(&["info", "-only", "bundleid", &format!("{}", pid)])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|info| parse_bundle_id(&info));

    // Store in cache
    BUNDLE_ID_CACHE
        .lock()
        .unwrap()
        .insert(pid, bundle_id.clone());

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
// Returns None if overlay window is not present (indicating we should keep previous window list)
pub fn get_all_windows_with_focus() -> Option<Vec<WindowInfo>> {
    let current_pid = std::process::id();

    // Get all windows and active window in parallel
    let all_windows_result = panic::catch_unwind(|| x_win::get_open_windows());
    let active_window_result = panic::catch_unwind(|| x_win::get_active_window());

    let (all_windows, active_window_id) = match (all_windows_result, active_window_result) {
        (Ok(Ok(windows)), Ok(Ok(active))) => (windows, Some(active.id)),
        (Ok(Ok(windows)), _) => (windows, None),
        _ => return Some(Vec::new()),
    };

    // Check if overlay window is present in results
    // If not, we've switched to a different view and should pause updates
    let overlay_window = all_windows
        .iter()
        .find(|w| w.info.process_id == current_pid);

    if overlay_window.is_none() {
        return None; // Overlay not found, pause updates
    }

    // Find overlay offset
    let overlay_offset = overlay_window
        .map(|w| (w.position.x, w.position.y))
        .unwrap_or((0, 0));

    // Get screen dimensions for filtering
    #[cfg(target_os = "macos")]
    let (screen_width, _) = get_main_screen_dimensions();
    #[cfg(not(target_os = "macos"))]
    let screen_width = f64::MAX;

    // Convert all windows, excluding our overlay, filtered apps, and fullscreen windows
    let windows = all_windows
        .iter()
        .filter(|w| w.info.process_id != current_pid && !should_filter_process(w.info.process_id))
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            WindowInfo::from_x_win(w, focused).with_offset(overlay_offset.0, overlay_offset.1)
        })
        .filter(|w| !w.is_fullscreen()) // Also filter out fullscreen windows
        .filter(|w| w.x <= (screen_width as i32 + 1)) // Filter out windows beyond screen width
        .collect();

    Some(windows)
}

// WebSocket-only polling loop
pub fn window_polling_loop(ws_state: WebSocketState) {
    use crate::window_manager::WindowManager;

    let mut last_windows: Option<Vec<WindowInfo>> = None;

    loop {
        let loop_start = Instant::now();

        // Get fresh data from system (lightweight - no AX elements)
        // Returns None if overlay window is not visible (switched to different view)
        let current_windows_opt = get_all_windows_with_focus();

        // No longer auto-push tree on focus change - let overlays request what they need

        // Only update if we got a result (overlay window is visible)
        if let Some(current_windows) = current_windows_opt {
            // Broadcast window updates if something changed
            if last_windows.as_ref() != Some(&current_windows) {
                // Update window manager (fetches AX elements only for new windows)
                // WindowManager handles all lifecycle including cleanup
                let (_managed_windows, _added_ids, _removed_ids) =
                    WindowManager::update_windows(current_windows.clone());

                // Broadcast root node for focused window
                if let Some(focused_window) = current_windows.iter().find(|w| w.focused) {
                    // Get the root AX node for this window
                    if let Ok(root_node) = crate::platform::get_ax_tree_by_window_id(
                        &focused_window.id,
                        1,     // Just the root, no children
                        0,     // No children
                        false, // Don't load full tree
                    ) {
                        ws_state.broadcast_window_root(&focused_window.id, root_node);
                    }
                }

                // Update WebSocket state and broadcast window list
                let ws_state_clone = ws_state.clone();
                let windows_clone = current_windows.clone();

                tokio::spawn(async move {
                    ws_state_clone.update_windows(&windows_clone).await;
                });

                ws_state.broadcast(&current_windows);

                last_windows = Some(current_windows);
            }
        }
        // If current_windows_opt is None, we keep last_windows unchanged (pause updates)

        // Precise interval handling
        let elapsed = loop_start.elapsed();
        let target_interval = Duration::from_millis(POLLING_INTERVAL_MS);
        if elapsed < target_interval {
            thread::sleep(target_interval - elapsed);
        }
    }
}
