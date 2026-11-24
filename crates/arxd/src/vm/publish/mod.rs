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

pub(crate) const MAX_HOSTNAMES: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvalidInput;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct Hostname(String);

impl Hostname {
    pub(crate) fn parse(value: &str) -> Result<Self, InvalidInput> {
        if !arx_guest::compose::routing::is_exact_hostname(value) {
            return Err(InvalidInput);
        }
        Ok(Self(value.to_owned()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// True when `hostnames` is non-empty, bounded by [`MAX_HOSTNAMES`], strictly
/// sorted ascending without duplicates, and belongs to one enrolled zone.
pub(crate) fn validate_hostname_set(
    hostnames: &[Hostname],
    zone_name: Option<&Hostname>,
) -> Result<(), InvalidInput> {
    if hostnames.is_empty() || hostnames.len() > MAX_HOSTNAMES {
        return Err(InvalidInput);
    }
    if !hostnames.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(InvalidInput);
    }
    let names: Vec<_> = hostnames.iter().map(Hostname::as_str).collect();
    if !arx_guest::compose::routing::share_enrolled_zone(&names, zone_name.map(Hostname::as_str)) {
        return Err(InvalidInput);
    }
    Ok(())
}

/// Fixed project/service/route slot binding for one enrolled public hostname.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RouteSlot {
    pub(crate) project_id: String,
    pub(crate) service: String,
    pub(crate) project_slot: u8,
    pub(crate) service_index: u8,
    pub(crate) route_slot: u8,
}

impl RouteSlot {
    pub(crate) fn target(&self, hostname: &Hostname) -> arx_guest::compose::routing::RouteTarget {
        arx_guest::compose::routing::RouteTarget {
            hostname: hostname.as_str().into(),
            project_id: self.project_id.clone(),
            service: self.service.clone(),
            project_slot: self.project_slot,
            service_index: self.service_index,
            route_slot: self.route_slot,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), InvalidInput> {
        if !arx_guest::compose::is_project_id(&self.project_id)
            || !valid_service_name(&self.service)
            || usize::from(self.project_slot) >= arx_guest::compose::MAX_PROJECTS
            || usize::from(self.service_index) >= arx_guest::compose::MAX_SERVICES
            || usize::from(self.route_slot) >= MAX_HOSTNAMES
        {
            return Err(InvalidInput);
        }
        Ok(())
    }

    pub(crate) fn loopback_port(&self) -> u16 {
        arx_guest::compose::loopback_port(self.project_slot, usize::from(self.service_index))
    }
}

fn valid_service_name(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 16
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
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

/// Render the complete closed YAML grammar for 1..=8 sorted unique hostnames.
/// Hostnames and tunnel identity have already passed their typed parsers, and
/// the CA path has been pinned. Each row targets the one leased localhost TLS
/// origin, verifies TLS against the custodied per-generation CA and
/// `originServerName`, and sends its own `httpHostHeader`.
pub(super) fn ingress(
    tunnel: &TunnelId,
    hostnames: &[Hostname],
    port: std::num::NonZeroU16,
    origin: &Hostname,
    ca: &str,
) -> Result<String, InvalidInput> {
    use std::fmt::Write as _;
    validate_hostname_set(hostnames, None)?;
    if ca
        .chars()
        .any(|ch| ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}'))
    {
        return Err(InvalidInput);
    }
    let ca = ca.replace('\'', "''");
    let mut out = format!(
        "tunnel: '{}'\nwarp-routing:\n  enabled: false\ningress:\n",
        tunnel.as_str()
    );
    for host in hostnames {
        let _ = write!(
            out,
            "  - hostname: '{}'\n    service: 'https://127.0.0.1:{}'\n    originRequest:\n      originServerName: '{}'\n      httpHostHeader: '{}'\n      caPool: '{}'\n      noTLSVerify: false\n",
            host.as_str(),
            port,
            origin.as_str(),
            host.as_str(),
            ca,
        );
    }
    out.push_str("  - service: http_status:404\n");
    Ok(out)
}
