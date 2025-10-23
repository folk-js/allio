/**
 * Private macOS Accessibility API bindings
 *
 * These are undocumented APIs that provide additional functionality.
 * Use with caution - may break in future macOS versions.
 */
use accessibility_sys::{AXError, AXUIElementRef};

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    /// Get the window ID (CGWindowID) for an AXUIElement representing a window
    ///
    /// This is a private API - not officially documented by Apple.
    /// Returns kAXErrorSuccess (0) if successful, error code otherwise.
    ///
    /// # Safety
    /// The element must be a valid AXUIElementRef representing a window.
    pub fn _AXUIElementGetWindow(element: AXUIElementRef, out_window_id: *mut u32) -> AXError;
}

/// Safe wrapper for getting window ID from an AXUIElement
pub fn get_window_id_from_element(element: AXUIElementRef) -> Option<u32> {
    let mut window_id: u32 = 0;
    let result = unsafe { _AXUIElementGetWindow(element, &mut window_id) };

    println!(
        "    🔍 _AXUIElementGetWindow: result={}, window_id={}",
        result, window_id
    );

    if result == 0 {
        // kAXErrorSuccess
        if window_id == 0 {
            println!("    ⚠️  API succeeded but returned window_id=0 (invalid)");
            None
        } else {
            Some(window_id)
        }
    } else {
        println!(
            "    ❌ _AXUIElementGetWindow failed with error code: {}",
            result
        );
        None
    }
}
