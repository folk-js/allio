// FFI bindings for AXValue to properly extract CGPoint and CGSize from accessibility attributes
//
// This module provides safe wrappers around the macOS Accessibility API's AXValue functions
// which are not exposed by the `accessibility` crate.

use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use serde::{Deserialize, Serialize};
use std::os::raw::c_void;

/// Represents a properly typed accessibility value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "value")]
pub enum AXValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

// CGPoint and CGSize structs matching macOS CoreGraphics definitions
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}

// AXValueType enum values
#[allow(non_upper_case_globals)]
const kAXValueTypeCGPoint: i32 = 1;
#[allow(non_upper_case_globals)]
const kAXValueTypeCGSize: i32 = 2;

// AXValue type (it's actually just a CFTypeRef under the hood)
type AXValueRef = CFTypeRef;

// External declarations for AXValue functions
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXValueGetType(value: AXValueRef) -> i32;
    fn AXValueGetValue(value: AXValueRef, value_type: i32, value_ptr: *mut c_void) -> bool;
}

/// Safely extract a CGPoint from a CFType that should be an AXValue
pub fn extract_position(cf_value: &impl TCFType) -> Option<(f64, f64)> {
    unsafe {
        let ax_value: AXValueRef = cf_value.as_CFTypeRef();

        // Check if this is a CGPoint type
        let value_type = AXValueGetType(ax_value);
        if value_type != kAXValueTypeCGPoint {
            return None;
        }

        // Extract the CGPoint
        let mut point = CGPoint { x: 0.0, y: 0.0 };
        let success = AXValueGetValue(
            ax_value,
            kAXValueTypeCGPoint,
            &mut point as *mut CGPoint as *mut c_void,
        );

        if success {
            Some((point.x, point.y))
        } else {
            None
        }
    }
}

/// Safely extract a CGSize from a CFType that should be an AXValue
pub fn extract_size(cf_value: &impl TCFType) -> Option<(f64, f64)> {
    unsafe {
        let ax_value: AXValueRef = cf_value.as_CFTypeRef();

        // Check if this is a CGSize type
        let value_type = AXValueGetType(ax_value);
        if value_type != kAXValueTypeCGSize {
            return None;
        }

        // Extract the CGSize
        let mut size = CGSize {
            width: 0.0,
            height: 0.0,
        };
        let success = AXValueGetValue(
            ax_value,
            kAXValueTypeCGSize,
            &mut size as *mut CGSize as *mut c_void,
        );

        if success {
            Some((size.width, size.height))
        } else {
            None
        }
    }
}

/// Properly extract a typed value from a CFType
/// Handles CFString, CFNumber, CFBoolean, and returns the appropriate typed value
///
/// For certain roles (toggles, checkboxes, radio buttons), 0/1 integers are converted to booleans
pub fn extract_value(cf_value: &impl TCFType, role: Option<&str>) -> Option<AXValue> {
    unsafe {
        let type_ref = cf_value.as_CFTypeRef();
        let cf_type = CFType::wrap_under_get_rule(type_ref);
        let type_id = cf_type.type_of();

        // Try CFString first (most common for values)
        if type_id == CFString::type_id() {
            let cf_string = CFString::wrap_under_get_rule(type_ref as *const _);
            let s = cf_string.to_string();
            // Filter out empty strings
            return if s.is_empty() {
                None
            } else {
                Some(AXValue::String(s))
            };
        }

        // Try CFNumber
        if type_id == CFNumber::type_id() {
            let cf_number = CFNumber::wrap_under_get_rule(type_ref as *const _);

            // For toggle-like elements, convert 0/1 integers to booleans
            if let Some(r) = role {
                if r == "AXToggle"
                    || r == "AXCheckBox"
                    || r == "AXRadioButton"
                    || r.contains("Toggle")
                    || r.contains("CheckBox")
                    || r.contains("RadioButton")
                {
                    if let Some(int_val) = cf_number.to_i64() {
                        return Some(AXValue::Boolean(int_val != 0));
                    }
                }
            }

            // Try to get as i64 first, then f64 if that fails
            if let Some(int_val) = cf_number.to_i64() {
                return Some(AXValue::Integer(int_val));
            } else if let Some(float_val) = cf_number.to_f64() {
                return Some(AXValue::Float(float_val));
            }
        }

        // Try CFBoolean
        if type_id == CFBoolean::type_id() {
            let cf_bool = CFBoolean::wrap_under_get_rule(type_ref as *const _);
            // CFBoolean can be converted to bool via Into trait
            let bool_val: bool = cf_bool.into();
            return Some(AXValue::Boolean(bool_val));
        }

        // For other types, we can't reliably extract them
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests would require actual AXValue instances from the system
    // They're here as documentation of the expected behavior

    #[test]
    fn test_cgpoint_size() {
        // Verify that CGPoint has the expected layout
        assert_eq!(std::mem::size_of::<CGPoint>(), 16); // 2 * f64
    }

    #[test]
    fn test_cgsize_size() {
        // Verify that CGSize has the expected layout
        assert_eq!(std::mem::size_of::<CGSize>(), 16); // 2 * f64
    }
}
