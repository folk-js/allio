//! Window Enumeration and Polling
//!
//! Provides window discovery and tracking for the accessibility system.
//! Uses x-win for cross-platform window enumeration.

use crate::types::AXWindow;
use crate::window_manager::WindowManager;
use crate::WindowId;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

// ============================================================================
// Configuration
// ============================================================================

/// Default polling interval (~120 FPS)
pub const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

/// Bundle IDs to filter out (screenshot UIs, etc.)
const FILTERED_BUNDLE_IDS: &[&str] = &[
    "com.apple.screencaptureui",
    "com.apple.screenshot.launcher",
    "com.apple.ScreenContinuity",
];

// ============================================================================
// Bundle ID Cache
// ============================================================================

static BUNDLE_ID_CACHE: Lazy<Mutex<HashMap<u32, Option<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[cfg(target_os = "macos")]
fn parse_bundle_id(info: &str) -> Option<String> {
    let eq_pos = info.rfind('=')?;
    let after_eq = &info[eq_pos + 1..];
    let start = after_eq.find('"')?;
    let end = after_eq[start + 1..].find('"')?;
    Some(after_eq[start + 1..start + 1 + end].to_string())
}

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
        .args(["info", "-only", "bundleid", &format!("{}", pid)])
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

#[cfg(target_os = "macos")]
fn should_filter_process(pid: u32) -> bool {
    if let Some(bundle_id) = get_bundle_id(pid) {
        FILTERED_BUNDLE_IDS.iter().any(|&id| bundle_id == id)
    } else {
        false
    }
}

#[cfg(not(target_os = "macos"))]
fn should_filter_process(_pid: u32) -> bool {
    false
}

// ============================================================================
// Screen Dimensions
// ============================================================================

#[cfg(target_os = "macos")]
pub fn get_main_screen_dimensions() -> (f64, f64) {
    unsafe {
        let display_id: CGDirectDisplayID = CGMainDisplayID();
        let width = CGDisplayPixelsWide(display_id) as f64;
        let height = CGDisplayPixelsHigh(display_id) as f64;
        (width, height)
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_main_screen_dimensions() -> (f64, f64) {
    (1920.0, 1080.0) // Default fallback
}

// ============================================================================
// Window Info Conversion
// ============================================================================

fn window_from_x_win(window: &x_win::WindowInfo, focused: bool) -> AXWindow {
    AXWindow {
        id: window.id.to_string(),
        title: window.title.clone(),
        app_name: window.info.name.clone(),
        x: window.position.x,
        y: window.position.y,
        w: window.position.width,
        h: window.position.height,
        focused,
        process_id: window.info.process_id,
        root: None,
    }
}

// ============================================================================
// Window Enumeration
// ============================================================================

/// Options for window enumeration
#[derive(Clone, Default)]
pub struct WindowEnumOptions {
    /// PID to exclude (typically the overlay's own process)
    /// If set, this window's position will be used as the coordinate offset
    pub exclude_pid: Option<u32>,
    /// Whether to filter out fullscreen windows
    pub filter_fullscreen: bool,
    /// Whether to filter out windows beyond screen bounds
    pub filter_offscreen: bool,
}

/// Get all windows with the focused state
/// Returns None if the excluded PID's window isn't found (overlay not visible)
///
/// When `exclude_pid` is set:
/// - Returns None if that process's window isn't found (overlay not visible)
/// - Uses that window's position as coordinate offset (for overlay alignment)
/// - Excludes that window from results
pub fn get_windows(options: &WindowEnumOptions) -> Option<Vec<AXWindow>> {
    use std::panic;

    // Get all windows and active window
    let all_windows_result = panic::catch_unwind(|| x_win::get_open_windows());
    let active_window_result = panic::catch_unwind(|| x_win::get_active_window());

    let (all_windows, active_window_id) = match (all_windows_result, active_window_result) {
        (Ok(Ok(windows)), Ok(Ok(active))) => (windows, Some(active.id)),
        (Ok(Ok(windows)), _) => (windows, None),
        _ => return Some(Vec::new()),
    };

    // Find the excluded window (overlay) to get its position for offset calculation
    let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
        match all_windows
            .iter()
            .find(|w| w.info.process_id == exclude_pid)
        {
            Some(overlay_window) => (overlay_window.position.x, overlay_window.position.y),
            None => return None, // Overlay not found = not visible
        }
    } else {
        (0, 0)
    };

    let (screen_width, screen_height) = get_main_screen_dimensions();

    let windows = all_windows
        .iter()
        .filter(|w| {
            // Filter by PID
            if let Some(exclude_pid) = options.exclude_pid {
                if w.info.process_id == exclude_pid {
                    return false;
                }
            }
            // Filter by bundle ID
            if should_filter_process(w.info.process_id) {
                return false;
            }
            true
        })
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            let mut info = window_from_x_win(w, focused);
            // Apply offset from overlay window position
            info.x -= offset_x;
            info.y -= offset_y;
            info
        })
        .filter(|w| {
            // Filter fullscreen (after offset applied)
            if options.filter_fullscreen {
                let is_fullscreen = w.x == 0
                    && w.y == 0
                    && (w.w as f64) == screen_width
                    && (w.h as f64) == screen_height;
                if is_fullscreen {
                    return false;
                }
            }
            // Filter offscreen
            if options.filter_offscreen && w.x > (screen_width as i32 + 1) {
                return false;
            }
            true
        })
        .collect();

    Some(windows)
}

// ============================================================================
// Window Polling
// ============================================================================

/// Configuration for the window polling loop
///
/// Events are emitted via the `EventSink` trait (set with `axio::set_event_sink`).
/// This keeps the polling loop simple and avoids duplicate notification mechanisms.
#[derive(Clone)]
pub struct PollingConfig {
    /// Window enumeration options
    pub enum_options: WindowEnumOptions,
    /// Polling interval in milliseconds
    pub interval_ms: u64,
}

impl Default for PollingConfig {
    fn default() -> Self {
        Self {
            enum_options: WindowEnumOptions::default(),
            interval_ms: DEFAULT_POLLING_INTERVAL_MS,
        }
    }
}

/// Start the window polling loop
///
/// This runs in a background thread and emits events via the `EventSink`:
/// - `on_window_update` when the window list changes
/// - `on_window_root` with the focused window's accessibility tree root
///
/// The loop runs until the process exits.
pub fn start_polling(config: PollingConfig) {
    thread::spawn(move || {
        let mut last_windows: Option<Vec<AXWindow>> = None;

        loop {
            let loop_start = Instant::now();

            if let Some(current_windows) = get_windows(&config.enum_options) {
                // Check if windows changed
                if last_windows.as_ref() != Some(&current_windows) {
                    // Update window manager (handles element cleanup for closed windows)
                    let _ = WindowManager::update_windows(current_windows.clone());

                    // Emit window update via event system
                    crate::events::emit_window_update(&current_windows);

                    // Get focused window root and emit
                    if let Some(focused) = current_windows.iter().find(|w| w.focused) {
                        let window_id = WindowId::new(focused.id.clone());
                        if let Ok(root) =
                            crate::platform::get_ax_tree_by_window_id(&window_id, 1, 0, false)
                        {
                            crate::events::emit_window_root(&focused.id, &root);
                        }
                    }

                    last_windows = Some(current_windows);
                }
            }

            // Maintain polling interval
            let elapsed = loop_start.elapsed();
            let target = Duration::from_millis(config.interval_ms);
            if elapsed < target {
                thread::sleep(target - elapsed);
            }
        }
    });
}
