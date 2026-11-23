//! Internal publication primitives. Admission and durable effect tracking are wired in B2.
#![allow(dead_code)] // B1 is intentionally unreachable from production entry points.

pub(super) mod cloudflare;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(super) mod connector;
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum AgentPublishing {
    #[default]
    Disabled,
    Enabled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Refusal {
    Disabled,
    CredentialMissing,
    StaticVmUnhealthy,
    HostnameMissing,
}

pub(super) fn admit(
    owner_policy: AgentPublishing,
    credential_present: bool,
    static_vm_healthy: bool,
    hostname: Option<&Hostname>,
) -> Result<(), Refusal> {
    if owner_policy == AgentPublishing::Disabled {
        return Err(Refusal::Disabled);
    }
    if !credential_present {
        return Err(Refusal::CredentialMissing);
    }
    if !static_vm_healthy {
        return Err(Refusal::StaticVmUnhealthy);
    }
    hostname.ok_or(Refusal::HostnameMissing).map(|_| ())
}

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
