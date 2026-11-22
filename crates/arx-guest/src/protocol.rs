//! Guest v1 frames carry a big-endian u32 payload length followed by one UTF-8
//! JSON object. Message tags use the `type` field. The serialized payload is
//! limited to 32 KiB, and the decoded script is limited to 4,096 UTF-8 bytes.

use std::fmt;
use std::io::{self, Cursor, Read, Write};
use std::str;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub const PROTOCOL_VERSION: u32 = 1;
pub const MAX_FRAME: usize = 32_768;
pub const MAX_SCRIPT: usize = 4096;
pub const MIN_TIMEOUT_S: u32 = 1;
pub const MAX_TIMEOUT_S: u32 = 120;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Request {
    Hello {
        version: u32,
        challenge: String,
    },
    Ping {
        id: u32,
    },
    RunScript {
        id: u32,
        script: String,
        /// Guest execution budget in seconds.
        timeout_s: u32,
    },
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum Response {
    HelloAck {
        version: u32,
        challenge: String,
        script_execution: ScriptExecution,
    },
    Pong {
        id: u32,
    },
    ScriptResult {
        id: u32,
        status: ScriptStatus,
        #[serde(
            default,
            deserialize_with = "deserialize_exit_code",
            skip_serializing_if = "Option::is_none"
        )]
        exit_code: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptExecution {
    Unavailable,
    Available,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptStatus {
    Exited,
    Unavailable,
    TimedOut,
    LaunchFailed,
    Terminated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    Io(io::ErrorKind),
    InvalidFrameLength,
    TruncatedPrefix,
    TruncatedPayload,
    InvalidUtf8,
    InvalidJson,
    InvalidChallenge,
    InvalidId,
    InvalidScriptLength,
    EmbeddedNul,
    InvalidTimeout,
    InvalidExitCode,
    ExpectedHello,
    UnsupportedVersion,
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "guest protocol I/O error",
            Self::InvalidFrameLength => "invalid guest frame length",
            Self::TruncatedPrefix => "truncated guest frame prefix",
            Self::TruncatedPayload => "truncated guest frame payload",
            Self::InvalidUtf8 => "invalid guest frame UTF-8",
            Self::InvalidJson => "invalid guest frame JSON",
            Self::InvalidChallenge => "invalid guest handshake challenge",
            Self::InvalidId => "invalid guest request ID",
            Self::InvalidScriptLength => "invalid guest script length",
            Self::EmbeddedNul => "guest script contains NUL",
            Self::InvalidTimeout => "invalid guest script timeout",
            Self::InvalidExitCode => "invalid guest script exit code",
            Self::ExpectedHello => "expected guest handshake",
            Self::UnsupportedVersion => "unsupported guest protocol version",
        })
    }
}

impl std::error::Error for ProtocolError {}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error.kind())
    }
}

mod private {
    pub trait Sealed {}
}

/// A request or response supported by the guest frame codec.
pub trait Message: private::Sealed + Serialize + DeserializeOwned {
    fn validate(&self) -> Result<(), ProtocolError>;
}

impl private::Sealed for Request {}

impl Message for Request {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::Hello { version, challenge } => validate_hello(*version, challenge),
            Self::Ping { id } => validate_id(*id),
            Self::RunScript {
                id,
                script,
                timeout_s,
            } => {
                validate_id(*id)?;
                if !(1..=MAX_SCRIPT).contains(&script.len()) {
                    return Err(ProtocolError::InvalidScriptLength);
                }
                if script.as_bytes().contains(&0) {
                    return Err(ProtocolError::EmbeddedNul);
                }
                if !(MIN_TIMEOUT_S..=MAX_TIMEOUT_S).contains(timeout_s) {
                    return Err(ProtocolError::InvalidTimeout);
                }
                Ok(())
            }
        }
    }
}

impl private::Sealed for Response {}

impl Message for Response {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::HelloAck {
                version, challenge, ..
            } => validate_hello(*version, challenge),
            Self::Pong { id } => validate_id(*id),
            Self::ScriptResult {
                id,
                status,
                exit_code,
            } => {
                validate_id(*id)?;
                if exit_code.is_some() != (*status == ScriptStatus::Exited) {
                    return Err(ProtocolError::InvalidExitCode);
                }
                Ok(())
            }
        }
    }
}

fn validate_hello(version: u32, challenge: &str) -> Result<(), ProtocolError> {
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion);
    }
    if challenge.len() == 32
        && challenge
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(ProtocolError::InvalidChallenge)
    }
}

fn deserialize_exit_code<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u32>, D::Error> {
    u32::deserialize(deserializer).map(Some)
}

fn validate_id(id: u32) -> Result<(), ProtocolError> {
    if id == 0 {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(())
    }
}

// Serde's enum deserializers also accept representations used by formats other
// than JSON. Require string tags before deserializing the full typed message.
#[derive(Deserialize)]
struct WireTags {
    #[serde(rename = "type")]
    _message_type: String,
    #[serde(rename = "status")]
    _status: Option<String>,
    #[serde(rename = "script_execution")]
    _script_execution: Option<String>,
}

/// Reads one frame, returning `None` only at EOF before a new prefix.
pub fn read_frame<M: Message>(reader: &mut impl Read) -> Result<Option<M>, ProtocolError> {
    let mut prefix = [0; 4];
    if read_retrying_interrupts(reader, &mut prefix[..1])? == 0 {
        return Ok(None);
    }
    read_remaining(reader, &mut prefix[1..], ProtocolError::TruncatedPrefix)?;

    let len = u32::from_be_bytes(prefix) as usize;
    if !(1..=MAX_FRAME).contains(&len) {
        return Err(ProtocolError::InvalidFrameLength);
    }
    let mut payload = vec![0; len];
    read_remaining(reader, &mut payload, ProtocolError::TruncatedPayload)?;

    let json = str::from_utf8(&payload).map_err(|_| ProtocolError::InvalidUtf8)?;
    if !json
        .trim_start_matches([' ', '\t', '\r', '\n'])
        .starts_with('{')
    {
        return Err(ProtocolError::InvalidJson);
    }
    serde_json::from_str::<WireTags>(json).map_err(|_| ProtocolError::InvalidJson)?;
    let message: M = serde_json::from_str(json).map_err(|_| ProtocolError::InvalidJson)?;
    message.validate()?;
    Ok(Some(message))
}

fn read_retrying_interrupts(reader: &mut impl Read, bytes: &mut [u8]) -> io::Result<usize> {
    loop {
        match reader.read(bytes) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn read_remaining(
    reader: &mut impl Read,
    mut bytes: &mut [u8],
    truncated: ProtocolError,
) -> Result<(), ProtocolError> {
    while !bytes.is_empty() {
        let count = read_retrying_interrupts(reader, bytes)?;
        if count == 0 {
            return Err(truncated);
        }
        bytes = &mut bytes[count..];
    }
    Ok(())
}

/// Writes and flushes one frame after validation and bounded serialization.
pub fn write_frame<M: Message>(writer: &mut impl Write, message: &M) -> Result<(), ProtocolError> {
    message.validate()?;
    // Cap serialization before writing the frame prefix to the transport.
    let mut payload = [0; MAX_FRAME];
    let mut cursor = Cursor::new(payload.as_mut_slice());
    serde_json::to_writer(&mut cursor, message).map_err(|error| {
        if error.is_io() {
            ProtocolError::InvalidFrameLength
        } else {
            ProtocolError::InvalidJson
        }
    })?;
    let len = cursor.position() as usize;
    writer.write_all(&(len as u32).to_be_bytes())?;
    writer.write_all(&payload[..len])?;
    writer.flush()?;
    Ok(())
}
