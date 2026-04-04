//! GS-232A serial protocol driver.
//!
//! Supported commands:
//! * `C`          – query current azimuth
//! * `C2`         – query current azimuth **and** elevation
//! * `M aaa`      – rotate to azimuth (0-359 degrees)
//! * `W aaa eee`  – rotate to azimuth and elevation (elevation 0-180 degrees)
//! * `S`          – stop all movement

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
            .timeout(Duration::from_millis(2000))
            .open()?;
        Ok(Self {
            port: Arc::new(Mutex::new(port)),
        })
    }

    /// Send a raw command and read back a single response line.
    #[instrument(skip(self))]
    fn send(&self, cmd: &str) -> Result<String, RotatorError> {
        let mut port = self.port.lock().expect("rotator mutex poisoned");
        let command = format!("{}\r", cmd);
        debug!(command = %command.trim(), "sending GS-232A command");
        port.write_all(command.as_bytes())?;
        port.flush()?;

        let mut reader = BufReader::new(&mut **port);
        let mut response = String::new();
        reader.read_line(&mut response)?;
        debug!(response = %response.trim(), "received GS-232A response");
        Ok(response.trim().to_string())
    }

    /// Parse a GS-232A position response `+0aaa+0eee` into `(azimuth, elevation)`.
    fn parse_position(raw: &str) -> Result<(f32, f32), RotatorError> {
        // Response format: +0aaa+0eee  (e.g. "+0123+0045")
        if raw.len() < 10 {
            return Err(RotatorError::UnexpectedResponse(raw.to_string()));
        }
        let az_str = &raw[0..5];   // "+0aaa"
        let el_str = &raw[5..10];  // "+0eee"
        let az: f32 = az_str
            .parse()
            .map_err(|_| RotatorError::UnexpectedResponse(raw.to_string()))?;
        let el: f32 = el_str
            .parse()
            .map_err(|_| RotatorError::UnexpectedResponse(raw.to_string()))?;
        Ok((az, el))
    }

    /// Query the current azimuth only (`C` command).
    pub fn get_azimuth(&self) -> Result<f32, RotatorError> {
        let raw = self.send("C")?;
        // Response is "+0aaa" (5 chars)
        let az: f32 = raw
            .parse()
            .map_err(|_| RotatorError::UnexpectedResponse(raw.clone()))?;
        Ok(az)
    }

    /// Query the current azimuth and elevation (`C2` command).
    pub fn get_position(&self) -> Result<(f32, f32), RotatorError> {
        let raw = self.send("C2")?;
        Self::parse_position(&raw)
    }

    /// Rotate to the given azimuth (`M aaa` command).
    ///
    /// `azimuth` must be in [0, 359].
    pub fn set_azimuth(&self, azimuth: u16) -> Result<(), RotatorError> {
        if azimuth > 359 {
            return Err(RotatorError::OutOfRange(format!(
                "azimuth {azimuth} is out of range [0, 359]"
            )));
        }
        self.send(&format!("M {:03}", azimuth))?;
        Ok(())
    }

    /// Rotate to the given azimuth **and** elevation (`W aaa eee` command).
    ///
    /// `azimuth` must be in [0, 359], `elevation` in [0, 180].
    pub fn set_position(&self, azimuth: u16, elevation: u16) -> Result<(), RotatorError> {
        if azimuth > 359 {
            return Err(RotatorError::OutOfRange(format!(
                "azimuth {azimuth} is out of range [0, 359]"
            )));
        }
        if elevation > 180 {
            return Err(RotatorError::OutOfRange(format!(
                "elevation {elevation} is out of range [0, 180]"
            )));
        }
        self.send(&format!("W {:03} {:03}", azimuth, elevation))?;
        Ok(())
    }

    /// Stop all movement (`S` command).
    pub fn stop(&self) -> Result<(), RotatorError> {
        self.send("S")?;
        Ok(())
    }
}
