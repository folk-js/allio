/*!
Configuration for AXIO.

All values have sensible defaults. Create a custom config to override:

```ignore
use axio::{Config, start_polling};

let config = Config {
    event_channel_capacity: 2000,
    ..Default::default()
};

// Pass to start_polling via PollingOptions, etc.
```
*/

/// AXIO configuration.
#[derive(Debug, Clone, Copy)]
pub struct Config {
  /// Capacity of the event broadcast channel.
  /// Default: 1000 events.
  pub event_channel_capacity: usize,

  /// WebSocket server port.
  /// Default: 3030.
  #[cfg(feature = "ws")]
  pub ws_port: u16,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      event_channel_capacity: 1000,
      #[cfg(feature = "ws")]
      ws_port: 3030,
    }
  }
}

impl Config {
  /// Create a new config with default values.
  pub fn new() -> Self {
    Self::default()
  }
}
