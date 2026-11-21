//! Bounded, single-message D4 records. No request material implements Debug.
use crate::{
    cluster::enrollment::{ClusterId, NodeId},
    service::executor::Request,
    store::BuildActivity,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const VERSION: u8 = 1;
const MAX_FRAME: usize = 64 * 1024;
pub(crate) type Operation = [u8; 32];
pub(crate) type Secret = [u8; 32];

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Descriptor {
    pub(crate) workspace: String,
    pub(crate) tool: String,
    pub(crate) args: Vec<String>,
    pub(crate) timeout_s: u64,
    pub(crate) output_bytes: usize,
}

// Command input stays local on A. B stores its own descriptors in grants.
#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Offer {
    pub(crate) workspace: String,
    command_sha256: [u8; 32],
    profile: Profile,
    pub(crate) timeout_s: u64,
    output_bytes: usize,
}

#[derive(Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Profile {
    StandardRun,
}

impl Descriptor {
    pub(crate) fn offer(&self) -> Option<Offer> {
        self.request()?;
        let mut hash = Sha256::new();
        hash.update(b"arxcovo/d4/standard-run/v1\0");
        hash.update(serde_json::to_vec(self).ok()?);
        Some(Offer {
            workspace: self.workspace.clone(),
            command_sha256: hash.finalize().into(),
            profile: Profile::StandardRun,
            timeout_s: self.timeout_s,
            output_bytes: self.output_bytes,
        })
    }

    pub(crate) fn request(&self) -> Option<Request> {
        let request = Request {
            tool: Some(self.tool.clone()),
            executable: None,
            args: self.args.clone(),
            cwd: String::new(),
            timeout_s: self.timeout_s,
            output_bytes: self.output_bytes,
        };
        (selector(&self.workspace)
            && request.validate()
            && self.output_bytes <= 4096
            && self.args.iter().map(String::len).sum::<usize>() <= 4096)
            .then_some(request)
    }
}

pub(crate) fn selector(value: &str) -> bool {
    (1..=48).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Query {
    Grant(Offer),
    Spend {
        operation: Operation,
        lease: Secret,
        descriptor: Offer,
    },
    Status {
        operation: Operation,
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Envelope {
    pub(crate) version: u8,
    pub(crate) cluster: ClusterId,
    pub(crate) source: NodeId,
    pub(crate) target: NodeId,
    pub(crate) agent: String,
    pub(crate) query: Query,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Receipt {
    pub(crate) version: u8,
    pub(crate) cluster: ClusterId,
    pub(crate) source: NodeId,
    pub(crate) target: NodeId,
    pub(crate) result: Response,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) enum Response {
    Granted {
        operation: Operation,
        lease: Secret,
    },
    Refused,
    Unknown {
        operation: Operation,
    },
    Finished {
        operation: Operation,
        activity: BuildActivity,
        output: Option<serde_json::Value>,
    },
}

fn invalid() -> io::Error {
    io::ErrorKind::InvalidData.into()
}

pub(crate) async fn send(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &impl Serialize,
) -> io::Result<()> {
    let payload = serde_json::to_vec(value).map_err(|_| invalid())?;
    if payload.is_empty() || payload.len() > MAX_FRAME {
        return Err(invalid());
    }
    stream.write_u32(payload.len() as u32).await?;
    stream.write_all(&payload).await?;
    // A TLS close-notify terminates this direction. The receiver requires it
    // to distinguish one complete frame from a padded or second request.
    stream.shutdown().await
}

pub(crate) async fn receive<T: DeserializeOwned>(
    stream: &mut (impl AsyncRead + Unpin),
) -> io::Result<T> {
    let size = stream.read_u32().await? as usize;
    if !(1..=MAX_FRAME).contains(&size) {
        return Err(invalid());
    }
    let mut payload = vec![0; size];
    stream.read_exact(&mut payload).await?;
    if stream.read(&mut [0]).await? != 0 {
        return Err(invalid());
    }
    serde_json::from_slice(&payload).map_err(|_| invalid())
}
