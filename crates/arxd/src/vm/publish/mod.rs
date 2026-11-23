//! Typed Cloudflare binding and connector primitives.
#![cfg_attr(
    not(all(windows, target_arch = "x86_64")),
    allow(dead_code, reason = "native connector is Windows x64 only")
)]

pub(super) mod cloudflare;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(super) mod connector;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(super) mod windows;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct InvalidInput;

#[derive(Clone, Eq, PartialEq)]
pub(super) struct Hostname(String);

impl Hostname {
    pub(super) fn parse(value: &str) -> Result<Self, InvalidInput> {
        if value.len() > 253
            || !value.contains('.')
            || !value
                .rsplit('.')
                .next()
                .is_some_and(|label| label.as_bytes().first().is_some_and(u8::is_ascii_lowercase))
            || !value.split('.').all(|label| {
                let bytes = label.as_bytes();
                !bytes.is_empty()
                    && bytes.len() <= 63
                    && bytes[0].is_ascii_alphanumeric()
                    && bytes[bytes.len() - 1].is_ascii_alphanumeric()
                    && bytes
                        .iter()
                        .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(InvalidInput);
        }
        Ok(Self(value.to_owned()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

pub(super) fn hex_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(lower_hex)
}

fn lower_hex(value: u8) -> bool {
    value.is_ascii_digit() || (b'a'..=b'f').contains(&value)
}

#[derive(Clone, Eq, PartialEq)]
pub(super) struct TunnelId(String);

impl TunnelId {
    pub(super) fn parse(value: &str) -> Result<Self, InvalidInput> {
        if value.len() != 36
            || !value.bytes().enumerate().all(|(index, byte)| {
                if matches!(index, 8 | 13 | 18 | 23) {
                    byte == b'-'
                } else {
                    lower_hex(byte)
                }
            })
        {
            return Err(InvalidInput);
        }
        Ok(Self(value.to_owned()))
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Render the complete closed YAML grammar. Hostnames and tunnel identity
/// have already passed their typed parsers, and the CA path has been pinned.
pub(super) fn ingress(
    tunnel: &TunnelId,
    hostnames: &[Hostname; 1],
    port: std::num::NonZeroU16,
    origin: &Hostname,
    ca: &str,
) -> String {
    format!(
        "tunnel: '{}'\nwarp-routing:\n  enabled: false\ningress:\n  - hostname: '{}'\n    service: 'https://127.0.0.1:{}'\n    originRequest:\n      originServerName: '{}'\n      caPool: '{}'\n      noTLSVerify: false\n  - service: http_status:404\n",
        tunnel.as_str(),
        hostnames[0].as_str(),
        port,
        origin.as_str(),
        ca,
    )
}
