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
#[derive(Debug, Clone)]
pub struct Config {
  /// Capacity of the event broadcast channel.
  /// Default: 1000 events.
  pub event_channel_capacity: usize,

  /// Default polling interval in milliseconds.
  /// Used when `PollingOptions::interval_ms` is not set.
  /// Default: 8ms (~120Hz).
  pub polling_interval_ms: u64,

  /// WebSocket server port.
  /// Default: 3030.
  #[cfg(feature = "ws")]
  pub ws_port: u16,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      event_channel_capacity: 1000,
      polling_interval_ms: 8,
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
