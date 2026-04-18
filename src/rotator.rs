//! GS-232B serial/RFC-2217 protocol driver.
//!
//! Supported commands:
//! * `C2`         – query current azimuth and elevation; response: `AZ=aaa EL=eee`
//! * `Maaa`       – rotate to azimuth (0-450 degrees); no response
//! * `Waaa eee`   – rotate to azimuth and elevation (elevation 0-180 degrees); no response
//! * `S`          – stop all movement; no response
//!
//! The device URL can be:
//! * A local serial port path, e.g. `/dev/ttyUSB0`
//! * An RFC 2217 remote serial server, e.g. `rfc2217://192.168.1.1:2217`

use std::{io, sync::Arc, time::Duration};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_serial::SerialPortBuilderExt;
use thiserror::Error;
use tracing::{debug, info, instrument, warn};

// ── Telnet / RFC 2217 constants ───────────────────────────────────────────────
const IAC: u8 = 0xFF;
const DONT: u8 = 0xFE;
const DO: u8 = 0xFD;
const WONT: u8 = 0xFC;
const WILL: u8 = 0xFB;
const SB: u8 = 0xFA;
const SE: u8 = 0xF0;
const COM_PORT_OPTION: u8 = 0x2C;

/// Maximum delay between reconnection attempts.
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

// ── Error type ────────────────────────────────────────────────────────────────
#[derive(Debug, Error)]
pub enum RotatorError {
    #[error("serial port error: {0}")]
    Serial(#[from] tokio_serial::Error),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("unexpected response from rotator: {0:?}")]
    UnexpectedResponse(String),
    #[error("value out of range: {0}")]
    OutOfRange(String),
}

// ── RFC 2217 low-level helpers ────────────────────────────────────────────────

/// Read a single raw byte from `r`.
async fn read_byte(r: &mut (impl AsyncReadExt + Unpin)) -> io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b).await?;
    Ok(b[0])
}

/// Read one *data* byte, transparently processing any Telnet IAC sequences
/// and sending the required replies on `w`.
async fn read_data_byte(
    r: &mut (impl AsyncReadExt + Unpin),
    w: &mut (impl AsyncWriteExt + Unpin),
) -> io::Result<u8> {
    loop {
        let b = read_byte(r).await?;
        if b != IAC {
            return Ok(b);
        }

        let cmd = read_byte(r).await?;
        match cmd {
            // Escaped 0xFF — pass through as a data byte
            IAC => return Ok(IAC),

            // Three-byte option negotiation
            WILL | WONT | DO | DONT => {
                let opt = read_byte(r).await?;
                match (cmd, opt) {
                    // Server asks us to use COM-PORT-OPTION: agree
                    (DO, COM_PORT_OPTION) => {
                        w.write_all(&[IAC, WILL, COM_PORT_OPTION]).await?;
                        w.flush().await?;
                    }
                    // Server offers an option we did not request: refuse
                    (WILL, _) => {
                        w.write_all(&[IAC, DONT, opt]).await?;
                        w.flush().await?;
                    }
                    // Server asks us to enable an unknown option: refuse
                    (DO, _) => {
                        w.write_all(&[IAC, WONT, opt]).await?;
                        w.flush().await?;
                    }
                    // WONT / DONT: acknowledged, no reply required
                    _ => {}
                }
            }

            // Sub-negotiation block — skip until IAC SE
            SB => loop {
                let b = read_byte(r).await?;
                if b == IAC && read_byte(r).await? == SE {
                    break;
                }
            },

            // Any other IAC command — ignore and keep reading
            _ => {}
        }
    }
}

/// Read a `\n`-terminated response line, transparently handling inline Telnet
/// IAC sequences.
async fn read_line_rfc2217(
    r: &mut (impl AsyncReadExt + Unpin),
    w: &mut (impl AsyncWriteExt + Unpin),
) -> io::Result<String> {
    let mut buf = Vec::new();
    loop {
        let b = read_data_byte(r, w).await?;
        buf.push(b);
        if b == b'\n' {
            break;
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

// ── RFC 2217 connection ───────────────────────────────────────────────────────

struct Rfc2217Inner {
    reader: ReadHalf<TcpStream>,
    writer: WriteHalf<TcpStream>,
}

struct Rfc2217Connection {
    addr: String,
    inner: Option<Rfc2217Inner>,
}

impl Rfc2217Connection {
    fn new(addr: String) -> Self {
        Self { addr, inner: None }
    }

    /// Attempt one TCP connection and perform the initial COM-PORT-OPTION
    /// negotiation handshake.
    async fn connect(&mut self) -> io::Result<()> {
        info!(addr = %self.addr, "connecting to RFC 2217 server");
        let stream = TcpStream::connect(&self.addr).await?;
        let (reader, mut writer) = tokio::io::split(stream);

        // Announce COM-PORT-OPTION support and request it from the server.
        // Any negotiation replies from the server will be handled inline the
        // first time read_line is called.
        writer
            .write_all(&[IAC, WILL, COM_PORT_OPTION, IAC, DO, COM_PORT_OPTION])
            .await?;
        writer.flush().await?;

        self.inner = Some(Rfc2217Inner { reader, writer });
        info!(addr = %self.addr, "RFC 2217 connection established");
        Ok(())
    }

    /// Keep trying to (re-)connect forever, with exponential back-off (1 s → 30 s).
    async fn reconnect(&mut self) {
        self.inner = None;
        let mut delay = Duration::from_secs(1);
        loop {
            match self.connect().await {
                Ok(()) => return,
                Err(e) => {
                    warn!(
                        addr = %self.addr,
                        error = %e,
                        delay_secs = delay.as_secs(),
                        "RFC 2217 reconnect failed, retrying"
                    );
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(MAX_RECONNECT_DELAY);
                }
            }
        }
    }

    async fn write_command(&mut self, data: &[u8]) -> io::Result<()> {
        let inner = self
            .inner
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "not connected"))?;

        // Escape 0xFF bytes per RFC 2217 / Telnet
        let mut escaped = Vec::with_capacity(data.len());
        for &b in data {
            if b == IAC {
                escaped.push(IAC);
                escaped.push(IAC);
            } else {
                escaped.push(b);
            }
        }
        inner.writer.write_all(&escaped).await?;
        inner.writer.flush().await
    }

    async fn read_line(&mut self) -> io::Result<String> {
        let inner = self
            .inner
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "not connected"))?;
        read_line_rfc2217(&mut inner.reader, &mut inner.writer).await
    }
}

// ── Connection enum ───────────────────────────────────────────────────────────

enum Connection {
    /// Local serial port wrapped in a `BufReader` for efficient line reading.
    /// `tokio::io::BufReader` passes writes through to the inner stream, so
    /// the same handle is used for both directions.
    Local(BufReader<tokio_serial::SerialStream>),
    Remote(Rfc2217Connection),
}

impl Connection {
    async fn write_command(&mut self, data: &[u8]) -> io::Result<()> {
        match self {
            Connection::Local(s) => {
                s.write_all(data).await?;
                s.flush().await
            }
            Connection::Remote(r) => r.write_command(data).await,
        }
    }

    async fn read_response_line(&mut self) -> io::Result<String> {
        match self {
            Connection::Local(s) => {
                let mut line = String::new();
                s.read_line(&mut line).await?;
                Ok(line)
            }
            Connection::Remote(r) => r.read_line().await,
        }
    }

    fn is_remote(&self) -> bool {
        matches!(self, Connection::Remote(_))
    }

    /// Reconnect the underlying transport. No-op for local serial ports.
    async fn reconnect(&mut self) {
        if let Connection::Remote(r) = self {
            r.reconnect().await;
        }
    }
}

// ── Rotator ───────────────────────────────────────────────────────────────────

/// Thread-safe handle to the rotator.
#[derive(Clone)]
pub struct Rotator {
    inner: Arc<Mutex<Connection>>,
}

impl Rotator {
    /// Open the rotator connection.
    ///
    /// `device` may be:
    /// * A local serial port path such as `/dev/ttyUSB0`
    /// * An RFC 2217 URL such as `rfc2217://192.168.1.1:2217`
    ///
    /// For RFC 2217 URLs the initial connection is retried with exponential
    /// back-off until it succeeds, so this call may block for an extended
    /// period if the server is unreachable at startup.
    pub async fn open(device: &str, baud_rate: u32) -> Result<Self, RotatorError> {
        let conn = if let Some(addr) = device.strip_prefix("rfc2217://") {
            let mut rfc = Rfc2217Connection::new(addr.to_string());
            rfc.reconnect().await; // retries forever until connected
            Connection::Remote(rfc)
        } else {
            let stream = tokio_serial::new(device, baud_rate).open_native_async()?;
            Connection::Local(BufReader::new(stream))
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    /// Send a command and read back a single response line.
    ///
    /// For RFC 2217 connections, automatically reconnects and retries on I/O
    /// errors.
    #[instrument(skip(self))]
    async fn send(&self, cmd: &str) -> Result<String, RotatorError> {
        let command = format!("{}\r", cmd);
        debug!(command = %cmd, "sending GS-232B command");
        let mut conn = self.inner.lock().await;
        loop {
            let result = async {
                conn.write_command(command.as_bytes()).await?;
                conn.read_response_line().await
            }
            .await;

            match result {
                Ok(line) => {
                    debug!(response = %line.trim(), "received GS-232B response");
                    return Ok(line.trim().to_string());
                }
                Err(e) if conn.is_remote() => {
                    warn!(error = %e, "rotator connection lost, reconnecting");
                    conn.reconnect().await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Send a command without reading a response.
    ///
    /// For RFC 2217 connections, automatically reconnects and retries on I/O
    /// errors.
    #[instrument(skip(self))]
    async fn execute(&self, cmd: &str) -> Result<(), RotatorError> {
        let command = format!("{}\r", cmd);
        debug!(command = %cmd, "sending GS-232B command");
        let mut conn = self.inner.lock().await;
        loop {
            match conn.write_command(command.as_bytes()).await {
                Ok(()) => return Ok(()),
                Err(e) if conn.is_remote() => {
                    warn!(error = %e, "rotator connection lost, reconnecting");
                    conn.reconnect().await;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Query the current azimuth and elevation (`C2` command).
    ///
    /// Response format: `AZ=aaa EL=eee`
    pub async fn get_position(&self) -> Result<(f32, f32), RotatorError> {
        let raw = self.send("C2").await?;
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
    pub async fn set_azimuth(&self, azimuth: u16) -> Result<(), RotatorError> {
        if azimuth > 450 {
            return Err(RotatorError::OutOfRange(format!(
                "azimuth {azimuth} is out of range [0, 450]"
            )));
        }
        self.execute(&format!("M{:03}", azimuth)).await
    }

    /// Rotate to the given azimuth **and** elevation (`Waaa eee` command).
    ///
    /// `azimuth` must be in [0, 450], `elevation` in [0, 180].
    pub async fn set_position(&self, azimuth: u16, elevation: u16) -> Result<(), RotatorError> {
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
        self.execute(&format!("W{:03} {:03}", azimuth, elevation)).await
    }

    /// Stop all movement (`S` command).
    pub async fn stop(&self) -> Result<(), RotatorError> {
        self.execute("S").await
    }
}
