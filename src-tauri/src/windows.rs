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
}

// Event payload structures
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WindowUpdatePayload {
    pub windows: Vec<WindowInfo>,
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

        // Only broadcast if something actually changed
        if last_windows.as_ref() != Some(&current_windows) {
            // Update WebSocket state and broadcast
            let ws_state_clone = ws_state.clone();
            let windows_clone = current_windows.clone();

            tokio::spawn(async move {
                ws_state_clone.update_windows(&windows_clone).await;
            });

            ws_state.broadcast(&WindowUpdatePayload {
                windows: current_windows.clone(),
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
