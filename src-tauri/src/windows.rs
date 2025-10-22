use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::c_void,
    panic::{self},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use accessibility::AXUIElement;
use accessibility_sys::{
    kAXUIElementDestroyedNotification, AXObserverAddNotification, AXObserverCreate,
    AXObserverGetRunLoopSource, AXObserverRef, AXUIElementRef,
};
use core_foundation::base::TCFType;
use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop, CFRunLoopSource};
use core_foundation::string::{CFString, CFStringRef};

// Private CoreGraphics function to get window ID from AXUIElement
extern "C" {
    fn _AXUIElementGetWindow(element: AXUIElementRef, window_id: *mut u32) -> i32;
}

// Cache for bundle ID lookups (PID -> Bundle ID)
static BUNDLE_ID_CACHE: Mutex<Option<HashMap<u32, Option<String>>>> = Mutex::new(None);

#[cfg(target_os = "macos")]
use core_graphics::display::{
    CGDirectDisplayID, CGDisplayPixelsHigh, CGDisplayPixelsWide, CGMainDisplayID,
};

use crate::axio::{AXNode, AXRole, Bounds, Position, Size};
use crate::websocket::WebSocketState;

// ============================================================================
// WindowTracker - Maintains windows with AXUIElements and close event observers
// ============================================================================

/// Tracked window with its accessibility element
#[derive(Clone)]
struct TrackedWindow {
    info: WindowInfo,
    #[allow(dead_code)] // Stored for observer reference - must be kept alive
    ax_element: AXUIElement,
}

/// Context for window close callbacks
struct WindowCloseContext {
    window_id: String,
    tracker: Arc<WindowTracker>,
}

/// Manages window tracking with accessibility observers
pub struct WindowTracker {
    tracked_windows: Arc<Mutex<HashMap<String, TrackedWindow>>>,
    observers: Arc<Mutex<HashMap<u32, AXObserverRef>>>, // PID -> Observer
    close_contexts: Arc<Mutex<HashMap<String, *mut c_void>>>, // window_id -> context pointer
}

unsafe impl Send for WindowTracker {}
unsafe impl Sync for WindowTracker {}

impl WindowTracker {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            tracked_windows: Arc::new(Mutex::new(HashMap::new())),
            observers: Arc::new(Mutex::new(HashMap::new())),
            close_contexts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Update tracked windows from polling results
    /// Adds new windows and subscribes to close events
    /// Keeps windows that are no longer onscreen until close event fires
    pub fn update_from_poll(self: &Arc<Self>, current_windows: &[WindowInfo]) {
        let mut tracked = self.tracked_windows.lock().unwrap();

        // Add new windows and subscribe to close events
        for window in current_windows {
            if !tracked.contains_key(&window.id) {
                // New window detected
                println!(
                    "🆕 New window detected: {} (PID: {})",
                    window.id, window.process_id
                );

                // Get AXUIElement for the window
                if let Some(ax_element) = self.get_window_element(window.process_id, window) {
                    // Subscribe to close event
                    if let Err(e) =
                        self.subscribe_to_close_event(window.process_id, &window.id, &ax_element)
                    {
                        println!(
                            "⚠️  Failed to subscribe to close event for window {}: {}",
                            window.id, e
                        );
                    } else {
                        println!("✅ Subscribed to close event for window {}", window.id);
                    }

                    // Add to tracked windows
                    tracked.insert(
                        window.id.clone(),
                        TrackedWindow {
                            info: window.clone(),
                            ax_element,
                        },
                    );
                } else {
                    println!("⚠️  Could not get AXUIElement for window {}", window.id);
                }
            } else {
                // Update existing window info (position might have changed)
                if let Some(tracked_window) = tracked.get_mut(&window.id) {
                    tracked_window.info = window.clone();
                }
            }
        }
    }

    /// Get all currently tracked windows
    pub fn get_tracked_windows(&self) -> Vec<WindowInfo> {
        self.tracked_windows
            .lock()
            .unwrap()
            .values()
            .map(|tw| tw.info.clone())
            .collect()
    }

    /// Remove a window (called by close event callback)
    fn remove_window(&self, window_id: &str) {
        let mut tracked = self.tracked_windows.lock().unwrap();
        if let Some(tracked_window) = tracked.remove(window_id) {
            println!("🗑️  Removed window from tracking: {}", window_id);

            // Remove observer if this was the last window for this PID
            let pid = tracked_window.info.process_id;
            let has_other_windows = tracked.values().any(|w| w.info.process_id == pid);

            if !has_other_windows {
                let mut observers = self.observers.lock().unwrap();
                if let Some(_observer) = observers.remove(&pid) {
                    println!("🧹 Removed observer for PID {} (no more windows)", pid);
                }
            }

            // Clean up context
            let mut contexts = self.close_contexts.lock().unwrap();
            if let Some(context_ptr) = contexts.remove(window_id) {
                unsafe {
                    let _ = Box::from_raw(context_ptr as *mut WindowCloseContext);
                }
            }
        }
    }

    /// Get the AXUIElement for a specific window by matching window ID using private API
    fn get_window_element(&self, pid: u32, window_info: &WindowInfo) -> Option<AXUIElement> {
        use accessibility::AXAttribute;

        let app_element = AXUIElement::application(pid as i32);

        // Parse window ID from string
        let target_window_id: u32 = window_info.id.parse().ok()?;

        // Get the windows array from the application
        let windows_attr = app_element.attribute(&AXAttribute::windows()).ok()?;

        // Iterate through all windows to find the one with matching window ID
        for i in 0..windows_attr.len() {
            if let Some(window_element_ref) = windows_attr.get(i) {
                // Clone to get owned AXUIElement
                let window_element = window_element_ref.clone();

                // Use private API to get the actual window ID
                let element_ref = window_element.as_concrete_TypeRef() as AXUIElementRef;
                let mut ax_window_id: u32 = 0;

                let result = unsafe { _AXUIElementGetWindow(element_ref, &mut ax_window_id) };

                if result == 0 && ax_window_id == target_window_id {
                    println!(
                        "  ✅ Matched window element by ID for window {}",
                        window_info.id
                    );
                    return Some(window_element);
                }
            }
        }

        println!(
            "  ⚠️  Could not match window element for window {}",
            window_info.id
        );
        None
    }

    /// Subscribe to window close events
    fn subscribe_to_close_event(
        self: &Arc<Self>,
        pid: u32,
        window_id: &str,
        element: &AXUIElement,
    ) -> Result<(), String> {
        // Get or create observer for this PID
        let observer = {
            let mut observers = self.observers.lock().unwrap();
            if !observers.contains_key(&pid) {
                // Create new observer
                let mut observer_ref: AXObserverRef = std::ptr::null_mut();

                let result = unsafe {
                    AXObserverCreate(
                        pid as i32,
                        window_close_callback as _,
                        &mut observer_ref as *mut _,
                    )
                };

                if result != 0 {
                    return Err(format!("Failed to create observer: error code {}", result));
                }

                println!("✅ Created AXObserver for PID {}", pid);

                // Add observer to the main run loop
                unsafe {
                    let run_loop_source_ref = AXObserverGetRunLoopSource(observer_ref);
                    if run_loop_source_ref.is_null() {
                        return Err("Failed to get run loop source from observer".to_string());
                    }

                    let run_loop_source =
                        CFRunLoopSource::wrap_under_get_rule(run_loop_source_ref as *mut _);
                    let main_run_loop = CFRunLoop::get_main();
                    main_run_loop.add_source(&run_loop_source, kCFRunLoopDefaultMode);

                    println!("✅ Added observer to main run loop");
                }

                observers.insert(pid, observer_ref);
                observer_ref
            } else {
                *observers.get(&pid).unwrap()
            }
        };

        // Create context for this window
        let context = Box::new(WindowCloseContext {
            window_id: window_id.to_string(),
            tracker: self.clone(),
        });
        let context_ptr = Box::into_raw(context) as *mut c_void;

        // Store context pointer
        self.close_contexts
            .lock()
            .unwrap()
            .insert(window_id.to_string(), context_ptr);

        // Register for destroyed notification
        let element_ref = element.as_concrete_TypeRef() as AXUIElementRef;
        let notif_cfstring = CFString::new(kAXUIElementDestroyedNotification);

        let result = unsafe {
            AXObserverAddNotification(
                observer,
                element_ref,
                notif_cfstring.as_concrete_TypeRef() as _,
                context_ptr,
            )
        };

        if result != 0 {
            return Err(format!("Failed to add notification: error code {}", result));
        }

        Ok(())
    }
}

/// C callback for window close notifications
unsafe extern "C" fn window_close_callback(
    _observer: AXObserverRef,
    _element: AXUIElementRef,
    notification: CFStringRef,
    refcon: *mut c_void,
) {
    if refcon.is_null() {
        return;
    }

    let context = &*(refcon as *const WindowCloseContext);
    let notif_cfstring = CFString::wrap_under_get_rule(notification);
    let notification_name = notif_cfstring.to_string();

    println!(
        "🔔 Window notification: {} for window {}",
        notification_name, context.window_id
    );

    if notification_name == "AXUIElementDestroyed" {
        println!("🪟 Window closed: {}", context.window_id);
        context.tracker.remove_window(&context.window_id);
    }
}

// Constants - optimized polling rate
const POLLING_INTERVAL_MS: u64 = 8; // ~120 FPS - fast enough for smooth tracking

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: String,
    pub app_name: String,
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub focused: bool,
    pub process_id: u32,
    pub fullscreen: bool,
}

// Convert x-win WindowInfo to our WindowInfo struct
impl WindowInfo {
    fn from_x_win(window: &x_win::WindowInfo, focused: bool) -> Self {
        WindowInfo {
            id: window.id.to_string(),
            app_name: window.info.name.clone(),
            title: window.title.clone(),
            x: window.position.x,
            y: window.position.y,
            w: window.position.width,
            h: window.position.height,
            focused,
            process_id: window.info.process_id,
            fullscreen: window.position.is_full_screen,
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

        // Build description with app name and fullscreen status
        let description = {
            let mut desc_parts = vec![];
            if !self.app_name.is_empty() {
                desc_parts.push(format!("app={}", self.app_name));
            }
            if self.fullscreen {
                desc_parts.push("fullscreen=true".to_string());
            }
            if desc_parts.is_empty() {
                None
            } else {
                Some(desc_parts.join(";"))
            }
        };

        Some(AXNode {
            pid: self.process_id,
            path: vec![], // Windows are root nodes (empty path)
            id: self.id.clone(),
            role: AXRole::Window,
            subrole: None,
            title: if !self.title.is_empty() {
                Some(self.title.clone())
            } else {
                None
            },
            value: None,
            description,
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
                        // println!( "📋 PID {} has bundle ID: {}", pid, id);
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

    // Convert all windows, excluding our overlay and filtered apps
    // NOTE: We NO LONGER filter out fullscreen windows - pass that info to frontend
    // NOTE: Coordinates are relative to the overlay window position
    all_windows
        .iter()
        .filter(|w| w.info.process_id != current_pid && !should_filter_process(w.info.process_id))
        .map(|w| {
            let focused = active_window_id.map_or(false, |id| id == w.id);
            WindowInfo::from_x_win(w, focused).with_offset(overlay_offset.0, overlay_offset.1)
        })
        .collect()
}

// WebSocket-only polling loop
pub fn window_polling_loop(ws_state: WebSocketState) {
    let mut last_windows: Option<Vec<WindowInfo>> = None;
    let window_tracker = WindowTracker::new();

    loop {
        let loop_start = Instant::now();

        // Poll for currently onscreen windows
        let onscreen_windows = get_all_windows_with_focus();

        // Update tracker with onscreen windows
        // This adds new windows and subscribes to close events
        window_tracker.update_from_poll(&onscreen_windows);

        // Get all tracked windows (onscreen + offscreen-but-not-yet-closed)
        let tracked_windows = window_tracker.get_tracked_windows();

        // Broadcast window updates if something changed
        if last_windows.as_ref() != Some(&tracked_windows) {
            // Convert windows to AXNodes (filter out any that fail to convert)
            let window_nodes: Vec<AXNode> = tracked_windows
                .iter()
                .filter_map(|w| w.to_ax_node())
                .collect();

            // Update WebSocket state and broadcast
            let ws_state_clone = ws_state.clone();
            let windows_clone = tracked_windows.clone();

            tokio::spawn(async move {
                ws_state_clone.update_windows(&windows_clone).await;
            });

            ws_state.broadcast(&WindowUpdatePayload {
                windows: window_nodes,
            });

            last_windows = Some(tracked_windows);
        }

        // Precise interval handling
        let elapsed = loop_start.elapsed();
        let target_interval = Duration::from_millis(POLLING_INTERVAL_MS);
        if elapsed < target_interval {
            thread::sleep(target_interval - elapsed);
        }
    }
}
