/*!
Write operations that modify OS state through the platform layer.

These are user-initiated actions that send commands to the accessibility API.
*/

use super::Axio;
use crate::accessibility::Action;
use crate::platform::PlatformHandle;
use crate::types::{AxioError, AxioResult, ElementId};

impl Axio {
  /// Set a typed value on an element.
  pub fn set_value(
    &self,
    element_id: ElementId,
    value: &crate::accessibility::Value,
  ) -> AxioResult<()> {
    // Step 1: Extract what we need (quick read)
    let (handle, role) = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok((e.handle.clone(), e.role))
    })?;

    // Step 2: Validate role is writable (no lock)
    if !role.is_writable() {
      return Err(AxioError::NotSupported(format!(
        "Element with role '{role:?}' is not writable"
      )));
    }

    // Step 3: Validate value type matches role's expected type
    let expected = role.value_type();
    let got = value.value_type();
    if expected != got {
      return Err(AxioError::TypeMismatch { expected, got });
    }

    // Step 4: Platform call (NO LOCK)
    handle.set_value(value)
  }

  /// Perform an action on an element.
  pub fn perform_action(&self, element_id: ElementId, action: Action) -> AxioResult<()> {
    let handle = self.read(|s| {
      let e = s
        .element(element_id)
        .ok_or(AxioError::ElementNotFound(element_id))?;
      Ok(e.handle.clone())
    })?;

    handle.perform_action(action)
  }
}
