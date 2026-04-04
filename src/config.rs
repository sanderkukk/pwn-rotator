use std::env;

/// Runtime configuration loaded from environment variables with sensible defaults.
#[derive(Debug, Clone)]
pub struct Config {
    /// Serial device path (e.g. `/dev/ttyUSB0`). Override with `ROTATOR_DEVICE`.
    pub device: String,
    /// Serial port baud rate. Override with `ROTATOR_BAUD`.
    pub baud_rate: u32,
    /// TCP address the HTTP server listens on. Override with `LISTEN_ADDR`.
    pub listen_addr: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            device: env::var("ROTATOR_DEVICE").unwrap_or_else(|_| "/dev/ttyUSB0".to_string()),
            baud_rate: env::var("ROTATOR_BAUD")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(9600),
            listen_addr: env::var("LISTEN_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3000".to_string()),
        }
    }
}
