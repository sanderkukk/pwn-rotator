//! GS-232B serial protocol driver.
//!
//! Supported commands:
//! * `Maaa`      – rotate to azimuth (0-450 degrees)
//! * `Waaa eee`  – rotate to azimuth and elevation (elevation 0-180 degrees)
//! * `S`          – stop all movement
//!
//! This device uses a write-only protocol: no response is returned for any command.
//! The last commanded position is tracked in memory and returned by `get_position`.

use std::{
    io::Write,
    sync::{Arc, Mutex},
};

use serialport::SerialPort;
use thiserror::Error;
use tracing::{debug, instrument};

#[derive(Debug, Error)]
pub enum RotatorError {
    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("value out of range: {0}")]
    OutOfRange(String),
}

struct RotatorState {
    azimuth: f32,
    elevation: f32,
}

/// Thread-safe handle to the rotator serial port.
#[derive(Clone)]
pub struct Rotator {
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    state: Arc<Mutex<RotatorState>>,
}

impl Rotator {
    /// Open the serial port at `device` with the given `baud_rate`.
    pub fn open(device: &str, baud_rate: u32) -> Result<Self, RotatorError> {
        let port = serialport::new(device, baud_rate).open()?;
        Ok(Self {
            port: Arc::new(Mutex::new(port)),
            state: Arc::new(Mutex::new(RotatorState {
                azimuth: 0.0,
                elevation: 0.0,
            })),
        })
    }

    /// Send a raw command over the serial port. No response is read.
    #[instrument(skip(self))]
    fn execute(&self, cmd: &str) -> Result<(), RotatorError> {
        let mut port = self.port.lock().expect("rotator mutex poisoned");
        let command = format!("{}\r", cmd);
        debug!(command = %command.trim(), "sending GS-232B command");
        port.write_all(command.as_bytes())?;
        port.flush()?;
        Ok(())
    }

    /// Return the last commanded azimuth and elevation.
    ///
    /// The device does not report its position; this reflects the most recent
    /// `set_azimuth` / `set_position` call, defaulting to `(0.0, 0.0)`.
    pub fn get_position(&self) -> Result<(f32, f32), RotatorError> {
        let s = self.state.lock().expect("rotator state mutex poisoned");
        Ok((s.azimuth, s.elevation))
    }

    /// Rotate to the given azimuth (`M aaa` command).
    ///
    /// `azimuth` must be in [0, 450].
    pub fn set_azimuth(&self, azimuth: u16) -> Result<(), RotatorError> {
        if azimuth > 450 {
            return Err(RotatorError::OutOfRange(format!(
                "azimuth {azimuth} is out of range [0, 450]"
            )));
        }
        self.execute(&format!("M{:03}", azimuth))?;
        let mut s = self.state.lock().expect("rotator state mutex poisoned");
        s.azimuth = f32::from(azimuth);
        Ok(())
    }

    /// Rotate to the given azimuth **and** elevation (`Waaa eee` command).
    ///
    /// `azimuth` must be in [0, 450], `elevation` in [0, 180].
    pub fn set_position(&self, azimuth: u16, elevation: u16) -> Result<(), RotatorError> {
        if azimuth > 450 {
            return Err(RotatorError::OutOfRange(format!(
                "azimuth {azimuth} is out of range [0, 450]"
            )));
        }
        if elevation > 180 {
            return Err(RotatorError::OutOfRange(format!(
                "elevation {elevation} is out of range [0, 180]"
            )));
        }
        self.execute(&format!("W{:03} {:03}", azimuth, elevation))?;
        let mut s = self.state.lock().expect("rotator state mutex poisoned");
        s.azimuth = f32::from(azimuth);
        s.elevation = f32::from(elevation);
        Ok(())
    }

    /// Stop all movement (`S` command).
    pub fn stop(&self) -> Result<(), RotatorError> {
        self.execute("S")
    }
}
