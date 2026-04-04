//! GS-232B serial protocol driver.
//!
//! Supported commands:
//! * `C2`         – query current azimuth and elevation; response: `AZ=aaa EL=eee`
//! * `Maaa`       – rotate to azimuth (0-450 degrees); no response
//! * `Waaa eee`   – rotate to azimuth and elevation (elevation 0-180 degrees); no response
//! * `S`          – stop all movement; no response

use std::{
    io::{BufRead, BufReader, Write},
    sync::{Arc, Mutex},
    time::Duration,
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
    #[error("unexpected response from rotator: {0:?}")]
    UnexpectedResponse(String),
    #[error("value out of range: {0}")]
    OutOfRange(String),
}

/// Thread-safe handle to the rotator serial port.
#[derive(Clone)]
pub struct Rotator {
    port: Arc<Mutex<Box<dyn SerialPort>>>,
}

impl Rotator {
    /// Open the serial port at `device` with the given `baud_rate`.
    pub fn open(device: &str, baud_rate: u32) -> Result<Self, RotatorError> {
        let port = serialport::new(device, baud_rate)
            .timeout(Duration::from_secs(5))
            .open()?;
        Ok(Self {
            port: Arc::new(Mutex::new(port)),
        })
    }

    /// Send a command and read back a single response line.
    ///
    /// Use for query commands (`C2`) that return a response.
    #[instrument(skip(self))]
    fn send(&self, cmd: &str) -> Result<String, RotatorError> {
        let mut port = self.port.lock().expect("rotator mutex poisoned");
        let command = format!("{}\r", cmd);
        debug!(command = %command.trim(), "sending GS-232B command");
        port.write_all(command.as_bytes())?;
        port.flush()?;
        let mut reader = BufReader::new(&mut **port);
        let mut response = String::new();
        reader.read_line(&mut response)?;
        debug!(response = %response.trim(), "received GS-232B response");
        Ok(response.trim().to_string())
    }

    /// Send a command without reading a response.
    ///
    /// Use for action commands (`M`, `W`, `S`) that produce no response.
    #[instrument(skip(self))]
    fn execute(&self, cmd: &str) -> Result<(), RotatorError> {
        let mut port = self.port.lock().expect("rotator mutex poisoned");
        let command = format!("{}\r", cmd);
        debug!(command = %command.trim(), "sending GS-232B command");
        port.write_all(command.as_bytes())?;
        port.flush()?;
        Ok(())
    }

    /// Query the current azimuth and elevation (`C2` command).
    ///
    /// Response format: `AZ=aaa EL=eee`
    pub fn get_position(&self) -> Result<(f32, f32), RotatorError> {
        let raw = self.send("C2")?;
        let mut az: Option<f32> = None;
        let mut el: Option<f32> = None;
        for part in raw.split_whitespace() {
            if let Some(val) = part.strip_prefix("AZ=") {
                az = val.parse().ok();
            } else if let Some(val) = part.strip_prefix("EL=") {
                el = val.parse().ok();
            }
        }
        match (az, el) {
            (Some(az), Some(el)) => Ok((az, el)),
            _ => Err(RotatorError::UnexpectedResponse(raw)),
        }
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
        self.execute(&format!("M{:03}", azimuth))
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
        self.execute(&format!("W{:03} {:03}", azimuth, elevation))
    }

    /// Stop all movement (`S` command).
    pub fn stop(&self) -> Result<(), RotatorError> {
        self.execute("S")
    }
}
