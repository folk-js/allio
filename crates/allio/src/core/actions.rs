/*!
Write operations that modify OS state through the platform layer.

These are user-initiated actions that send commands to the accessibility API.
*/

use super::Allio;
use crate::a11y::Action;
use crate::platform::PlatformHandle;
use crate::types::{AllioError, AllioResult, ElementId};

impl Allio {
  /// Set a typed value on an element.
  pub fn set_value(&self, element_id: ElementId, value: &crate::a11y::Value) -> AllioResult<()> {
    // Step 1: Extract what we need (quick read)
    let (handle, role) = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AllioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.role))
    })?;

    // Step 2: Validate role is writable (no lock)
    if !role.is_writable() {
      return Err(AllioError::NotSupported(format!(
        "Element with role '{role:?}' is not writable"
      )));
    }

    // Step 3: Validate value type matches role's expected type
    let expected = role.value_type();
    let got = value.value_type();
    if expected != got {
      return Err(AllioError::TypeMismatch { expected, got });
    }

    // Step 4: Platform call (NO LOCK)
    handle.set_value(value)
  }

  /// Perform an action on an element.
  pub fn perform_action(&self, element_id: ElementId, action: Action) -> AllioResult<()> {
    let handle = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AllioError::ElementNotFound(element_id))?;
      Ok(e.handle.clone())
    })?;

    handle.perform_action(action)
  }
}
