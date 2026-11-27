//! Fixed provisioning handshake on the attributed static guest port.
use crate::protocol::{Message, ProtocolError, read_frame, write_frame};
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

pub const FW_CFG: &str = "opt/arxcovo/static-origin";
pub const MAX_CERT: usize = 4096;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub enum Request {
    StaticOrigin {
        generation: [u8; 16],
        nonce: [u8; 16],
    },
}

impl crate::protocol::private::Sealed for Request {}

impl Message for Request {
    fn validate(&self) -> Result<(), ProtocolError> {
        let Self::StaticOrigin { generation, nonce } = self;
        if *generation == [0; 16] || *nonce == [0; 16] {
            return Err(ProtocolError::InvalidComposeField);
        }
        Ok(())
    }
}

#[derive(Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "snake_case")]
pub enum Response {
    StaticOriginReady {
        request: Request,
        ca: Vec<u8>,
        leaf: Vec<u8>,
    },
}

impl crate::protocol::private::Sealed for Response {}

impl Message for Response {
    fn validate(&self) -> Result<(), ProtocolError> {
        let Self::StaticOriginReady { request, ca, leaf } = self;
        request.validate()?;
        if [ca, leaf]
            .iter()
            .any(|cert| !(1..=MAX_CERT).contains(&cert.len()))
        {
            return Err(ProtocolError::InvalidComposeField);
        }
        Ok(())
    }
}

pub fn serve(
    mut reader: impl Read,
    mut writer: impl Write,
    expected: [u8; 16],
    activate: impl FnOnce(Request) -> io::Result<[Vec<u8>; 2]>,
) -> io::Result<()> {
    let bad = |_| io::Error::from(io::ErrorKind::InvalidData);
    let request = read_frame(&mut reader).map_err(bad)?
        .filter(|request| matches!(request, Request::StaticOrigin { generation, .. } if *generation == expected))
        .ok_or(io::ErrorKind::InvalidData)?;
    let [ca, leaf] = activate(request)?;
    write_frame(
        &mut writer,
        &Response::StaticOriginReady { request, ca, leaf },
    )
    .map_err(bad)?;
    writer.flush()
}

pub fn server_name(generation: [u8; 16]) -> String {
    use std::fmt::Write as _;
    let mut name = String::from("arx-");
    for byte in generation {
        write!(name, "{byte:02x}").expect("write to string");
    }
    name.push_str(".invalid");
    name
}

pub fn health_body(nonce: [u8; 16]) -> String {
    use std::fmt::Write as _;
    let mut body = String::new();
    for byte in nonce {
        write!(body, "{byte:02x}").expect("write to string");
    }
    body
}

#[cfg(test)]
#[test]
fn wrong_generation_and_zero_nonce_refuse_before_provisioning() {
    let expected = [1; 16];
    for request in [
        Request::StaticOrigin {
            generation: [2; 16],
            nonce: [3; 16],
        },
        Request::StaticOrigin {
            generation: expected,
            nonce: [0; 16],
        },
    ] {
        let mut input = Vec::new();
        // Encode directly so the receiver must enforce the validation.
        let body = serde_json::to_vec(&request).unwrap();
        input.extend_from_slice(&(body.len() as u32).to_be_bytes());
        input.extend_from_slice(&body);
        let mut out = Vec::new();
        assert!(
            serve(&*input, &mut out, expected, |_| panic!(
                "unexpected provision"
            ))
            .is_err()
        );
        assert!(out.is_empty());
    }
    let request = Request::StaticOrigin {
        generation: expected,
        nonce: [3; 16],
    };
    let mut input = Vec::new();
    write_frame(&mut input, &request).unwrap();
    let mut out = Vec::new();
    serve(&*input, &mut out, expected, |_| Ok([vec![1], vec![2]])).unwrap();
    assert_eq!(
        read_frame(&mut out.as_slice()).unwrap(),
        Some(Response::StaticOriginReady {
            request,
            ca: vec![1],
            leaf: vec![2]
        }),
    );
}
