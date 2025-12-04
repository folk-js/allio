//! Window enumeration and polling using x-win.

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

pub const DEFAULT_POLLING_INTERVAL_MS: u64 = 8;

const FILTERED_BUNDLE_IDS: &[&str] = &[
    "com.apple.screencaptureui",
    "com.apple.screenshot.launcher",
    "com.apple.ScreenContinuity",
];

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

    {
        let cache = BUNDLE_ID_CACHE.lock().unwrap();
        if let Some(cached) = cache.get(&pid) {
            return cached.clone();
        }
    }

    // TODO: Use native NSRunningApplication API instead of shelling out
    let bundle_id = Command::new("lsappinfo")
        .args(["info", "-only", "bundleid", &format!("{}", pid)])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|info| parse_bundle_id(&info));

    BUNDLE_ID_CACHE
        .lock()
        .unwrap()
        .insert(pid, bundle_id.clone());

    bundle_id
}

#[cfg(target_os = "macos")]
fn should_filter_process(pid: u32) -> bool {
    get_bundle_id(pid).map_or(false, |id| FILTERED_BUNDLE_IDS.contains(&id.as_str()))
}

#[cfg(not(target_os = "macos"))]
fn should_filter_process(_pid: u32) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub fn get_main_screen_dimensions() -> (f64, f64) {
    unsafe {
        let display_id: CGDirectDisplayID = CGMainDisplayID();
        (
            CGDisplayPixelsWide(display_id) as f64,
            CGDisplayPixelsHigh(display_id) as f64,
        )
    }
}

#[cfg(not(target_os = "macos"))]
pub fn get_main_screen_dimensions() -> (f64, f64) {
    (1920.0, 1080.0)
}

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
        root: None, // Populated client-side from WindowRoot event
    }
}

#[derive(Clone, Default)]
pub struct WindowEnumOptions {
    /// PID to exclude. Its window position is used as coordinate offset.
    pub exclude_pid: Option<u32>,
    pub filter_fullscreen: bool,
    pub filter_offscreen: bool,
}

/// Returns None if exclude_pid is set but that window isn't found.
pub fn get_windows(options: &WindowEnumOptions) -> Option<Vec<AXWindow>> {
    use std::panic;

    let all_windows_result = panic::catch_unwind(|| x_win::get_open_windows());
    let active_window_result = panic::catch_unwind(|| x_win::get_active_window());

    let (all_windows, active_window_id) = match (all_windows_result, active_window_result) {
        (Ok(Ok(windows)), Ok(Ok(active))) => (windows, Some(active.id)),
        (Ok(Ok(windows)), _) => (windows, None),
        _ => return Some(Vec::new()),
    };

    let (offset_x, offset_y) = if let Some(exclude_pid) = options.exclude_pid {
        match all_windows
            .iter()
            .find(|w| w.info.process_id == exclude_pid)
        {
            Some(overlay_window) => (overlay_window.position.x, overlay_window.position.y),
            None => return None,
        }
    } else {
        (0, 0)
    };

    let (screen_width, screen_height) = get_main_screen_dimensions();

    let windows = all_windows
        .iter()
        .filter(|w| {
            if options.exclude_pid == Some(w.info.process_id) {
                return false;
            }
            if should_filter_process(w.info.process_id) {
                return false;
            }
            true
        })
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            let mut info = window_from_x_win(w, focused);
            info.x -= offset_x;
            info.y -= offset_y;
            info
        })
        .filter(|w| {
            if options.filter_fullscreen {
                let is_fullscreen = w.x == 0
                    && w.y == 0
                    && (w.w as f64) == screen_width
                    && (w.h as f64) == screen_height;
                if is_fullscreen {
                    return false;
                }
            }
            if options.filter_offscreen && w.x > (screen_width as i32 + 1) {
                return false;
            }
            true
        })
        .collect();

    Some(windows)
}

#[derive(Clone)]
pub struct PollingConfig {
    pub enum_options: WindowEnumOptions,
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

/// Runs in background thread, emits events via EventSink.
pub fn start_polling(config: PollingConfig) {
    thread::spawn(move || {
        let mut last_windows: Option<Vec<AXWindow>> = None;

        loop {
            let loop_start = Instant::now();

            if let Some(current_windows) = get_windows(&config.enum_options) {
                if last_windows.as_ref() != Some(&current_windows) {
                    let _ = WindowManager::update_windows(current_windows.clone());
                    crate::events::emit_window_update(&current_windows);

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

            let elapsed = loop_start.elapsed();
            let target = Duration::from_millis(config.interval_ms);
            if elapsed < target {
                thread::sleep(target - elapsed);
            }
        }
    });
}
