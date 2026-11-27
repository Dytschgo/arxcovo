//! The fixed bootstrap channel for a disposable Compose qualification boot.
use std::io::{Read, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::compose::{
    COMPOSE_CAPABILITIES, COMPOSE_PROTOCOL_VERSION, ComposeRequest, ComposeResponse,
};
use crate::protocol::{Message, ProtocolError, private, read_frame, write_frame};

pub const FW_CFG: &str = "opt/arxcovo/compose-trial";
pub const DETACHED: &str = "detached-v1";
pub const ENDPOINT: &str = "endpoint-v2";
pub const MAX_ORIGIN_CERT: usize = 4096;
pub const ORIGIN_HEALTH_PATH: &str = "/arx-health";
pub const ORIGIN_PROFILE: &[u8] = b"arx-docker-trial-origin-v2\n";
pub const PACKAGES: [&str; 4] = [
    "containerd.io",
    "docker-ce",
    "docker-ce-cli",
    "docker-compose-plugin",
];

/// Exact installed Debian versions, including epochs and packaging suffixes.
pub fn package_digest(versions: [&str; 4]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"arx-compose-packages-v1\0");
    for (name, version) in PACKAGES.into_iter().zip(versions) {
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(version.as_bytes());
        hash.update([0]);
    }
    hash.finalize().into()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootMode {
    Detached,
    Endpoint,
}

impl BootMode {
    pub const fn value(self) -> &'static str {
        match self {
            Self::Detached => DETACHED,
            Self::Endpoint => ENDPOINT,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Challenge {
    pub generation: u64,
    pub nonce: [u8; 16],
}

impl Challenge {
    fn validate(self) -> Result<(), ProtocolError> {
        if self.generation == 0 || self.nonce == [0; 16] {
            return Err(ProtocolError::InvalidChallenge);
        }
        Ok(())
    }

    pub fn origin_name(self) -> String {
        format!("arx-{}.invalid", self.compose_nonce())
    }

    pub fn origin_health(self) -> String {
        format!("{}-{}\n", self.generation, self.compose_nonce())
    }

    pub fn compose_nonce(self) -> String {
        self.nonce
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TrialRequest {
    DockerTrialHealth { challenge: Challenge },
    DockerTrialProvision { challenge: Challenge },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum TrialResponse {
    DockerTrialHealthy {
        challenge: Challenge,
        packages: [u8; 32],
    },
    DockerTrialProvisioned {
        challenge: Challenge,
        certificate_der: Vec<u8>,
    },
}

impl private::Sealed for TrialRequest {}
impl private::Sealed for TrialResponse {}

impl Message for TrialRequest {
    fn validate(&self) -> Result<(), ProtocolError> {
        let (Self::DockerTrialHealth { challenge } | Self::DockerTrialProvision { challenge }) =
            self;
        challenge.validate()
    }
}

impl Message for TrialResponse {
    fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            Self::DockerTrialHealthy {
                challenge,
                packages,
            } if *packages != [0; 32] => challenge.validate(),
            Self::DockerTrialProvisioned {
                challenge,
                certificate_der,
            } if (1..=MAX_ORIGIN_CERT).contains(&certificate_der.len()) => challenge.validate(),
            Self::DockerTrialProvisioned { .. } => Err(ProtocolError::InvalidComposeField),
            Self::DockerTrialHealthy { .. } => Err(ProtocolError::InvalidComposeField),
        }
    }
}

/// Pre-service observations from the finalized offline recipe.
pub struct Health {
    pub recipe_layout: bool,
    pub media_and_residue_absent: bool,
    pub units_masked: bool,
    pub units_inactive: bool,
    pub swap_disabled: bool,
    pub secret_tmpfs: bool,
}

impl Health {
    pub fn ready(&self) -> bool {
        self.recipe_layout
            && self.media_and_residue_absent
            && self.units_masked
            && self.units_inactive
            && self.swap_disabled
            && self.secret_tmpfs
    }
}

/// Prove the path is the root of a dedicated tmpfs and carries the reviewed
/// mount restrictions. The parent /run can itself be tmpfs.
pub fn secret_mount_matches(mountinfo: &str) -> bool {
    let mut matched = false;
    for line in mountinfo.lines() {
        let Some((mount, filesystem)) = line.split_once(" - ") else {
            continue;
        };
        let mut fields = mount.split_whitespace().skip(3);
        let root = fields.next();
        if fields.next() != Some(crate::compose::SECRET_ROOT) {
            continue;
        }
        let Some(options) = fields.next() else {
            return false;
        };
        if matched
            || root != Some("/")
            || filesystem.split_whitespace().next() != Some("tmpfs")
            || !["rw", "nosuid", "nodev", "noexec"]
                .into_iter()
                .all(|required| options.split(',').any(|option| option == required))
        {
            return false;
        }
        matched = true;
    }
    matched
}

/// Local fixed actions. The wire cannot supply a command, image, path or secret.
pub trait Bootstrap {
    fn health(&mut self) -> Option<[u8; 32]>;
    fn provision(&mut self, challenge: Challenge) -> Option<Vec<u8>>;
}

/// The detached boot accepts health only. On the separate endpoint copy,
/// health precedes the one provisioning action and the exact Compose hello.
/// This session installs no production broker or script executor.
pub fn serve(
    mode: BootMode,
    mut reader: impl Read,
    mut writer: impl Write,
    guest: &mut impl Bootstrap,
) -> Result<(), ProtocolError> {
    let Some(TrialRequest::DockerTrialHealth { challenge }) = read_frame(&mut reader)? else {
        return Err(ProtocolError::UnexpectedRequest);
    };
    let packages = guest.health().ok_or(ProtocolError::UnexpectedRequest)?;
    write_frame(
        &mut writer,
        &TrialResponse::DockerTrialHealthy {
            challenge,
            packages,
        },
    )?;
    if mode == BootMode::Detached {
        return Ok(());
    }
    if read_frame(&mut reader)? != Some(TrialRequest::DockerTrialProvision { challenge }) {
        return Err(ProtocolError::UnexpectedRequest);
    }
    let certificate_der = guest
        .provision(challenge)
        .ok_or(ProtocolError::UnexpectedRequest)?;
    write_frame(
        &mut writer,
        &TrialResponse::DockerTrialProvisioned {
            challenge,
            certificate_der,
        },
    )?;
    if read_frame(&mut reader)? != Some(compose_hello(challenge)) {
        return Err(ProtocolError::ExpectedComposeHello);
    }
    write_frame(&mut writer, &compose_ack(challenge))?;
    // Keep the guest service and its nginx child alive for HTTPS. The host
    // holds the verified pipe until shutdown and sends no more trial frames.
    if read_frame::<TrialRequest>(&mut reader)?.is_some() {
        return Err(ProtocolError::UnexpectedRequest);
    }
    Ok(())
}

pub fn compose_hello(challenge: Challenge) -> ComposeRequest {
    ComposeRequest::ComposeHello {
        version: COMPOSE_PROTOCOL_VERSION,
        challenge: challenge.compose_nonce(),
        generation: challenge.generation,
        capabilities: capability_names(),
    }
}

pub fn matches_compose_ack(challenge: Challenge, response: &ComposeResponse) -> bool {
    *response == compose_ack(challenge)
}

fn compose_ack(challenge: Challenge) -> ComposeResponse {
    ComposeResponse::ComposeHelloAck {
        version: COMPOSE_PROTOCOL_VERSION,
        challenge: challenge.compose_nonce(),
        generation: challenge.generation,
        capabilities: capability_names(),
        cleared_staging: 0,
        ambiguous_operations: 0,
    }
}

fn capability_names() -> Vec<String> {
    COMPOSE_CAPABILITIES.map(str::to_owned).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHALLENGE: Challenge = Challenge {
        generation: 4,
        nonce: [42; 16],
    };
    const HEALTH: TrialRequest = TrialRequest::DockerTrialHealth {
        challenge: CHALLENGE,
    };
    const PROVISION: TrialRequest = TrialRequest::DockerTrialProvision {
        challenge: CHALLENGE,
    };

    const CERTIFICATE: &[u8] = b"fixture certificate";

    struct Guest {
        healthy: bool,
        provisions: u8,
    }

    impl Bootstrap for Guest {
        fn health(&mut self) -> Option<[u8; 32]> {
            self.healthy.then_some([11; 32])
        }

        fn provision(&mut self, challenge: Challenge) -> Option<Vec<u8>> {
            assert_eq!(challenge, CHALLENGE);
            self.provisions += 1;
            Some(CERTIFICATE.into())
        }
    }

    #[test]
    fn private_bootstrap_completes_only_the_fixed_exchange() {
        let mut request = Vec::new();
        write_frame(&mut request, &HEALTH).unwrap();
        write_frame(&mut request, &PROVISION).unwrap();
        write_frame(&mut request, &compose_hello(CHALLENGE)).unwrap();
        let mut guest = Guest {
            healthy: true,
            provisions: 0,
        };
        let mut reply = Vec::new();
        serve(
            BootMode::Detached,
            request.as_slice(),
            &mut reply,
            &mut guest,
        )
        .unwrap();
        assert_eq!(guest.provisions, 0);
        let mut reply = reply.as_slice();
        assert_eq!(
            read_frame(&mut reply).unwrap(),
            Some(TrialResponse::DockerTrialHealthy {
                challenge: CHALLENGE,
                packages: [11; 32],
            })
        );
        assert!(reply.is_empty());

        let mut reply = Vec::new();
        serve(
            BootMode::Endpoint,
            request.as_slice(),
            &mut reply,
            &mut guest,
        )
        .unwrap();
        assert_eq!(guest.provisions, 1);
        let mut reply = reply.as_slice();
        assert!(matches!(
            read_frame::<TrialResponse>(&mut reply).unwrap(),
            Some(TrialResponse::DockerTrialHealthy { .. })
        ));
        assert_eq!(
            read_frame(&mut reply).unwrap(),
            Some(TrialResponse::DockerTrialProvisioned {
                challenge: CHALLENGE,
                certificate_der: CERTIFICATE.into(),
            })
        );
        let ack = read_frame(&mut reply).unwrap().unwrap();
        assert!(matches_compose_ack(CHALLENGE, &ack));
        assert!(!matches_compose_ack(
            Challenge {
                generation: 9,
                ..CHALLENGE
            },
            &ack
        ));
        assert!(reply.is_empty());
        let ComposeResponse::ComposeHelloAck {
            version,
            challenge,
            generation,
            mut capabilities,
            ..
        } = ack
        else {
            unreachable!();
        };
        capabilities.reverse();
        let reordered = ComposeResponse::ComposeHelloAck {
            version,
            challenge,
            generation,
            capabilities,
            cleared_staging: 0,
            ambiguous_operations: 0,
        };
        assert!(!matches_compose_ack(CHALLENGE, &reordered));
    }

    #[test]
    fn trial_origin_reply_carries_a_single_bounded_public_certificate() {
        for certificate_der in [Vec::new(), vec![0; MAX_ORIGIN_CERT + 1]] {
            let response = TrialResponse::DockerTrialProvisioned {
                challenge: CHALLENGE,
                certificate_der,
            };
            assert!(write_frame(&mut Vec::new(), &response).is_err());
        }
    }

    #[test]
    fn endpoint_trial_refuses_additional_requests_after_compose_hello() {
        let mut input = Vec::new();
        write_frame(&mut input, &HEALTH).unwrap();
        write_frame(&mut input, &PROVISION).unwrap();
        write_frame(&mut input, &compose_hello(CHALLENGE)).unwrap();
        write_frame(&mut input, &PROVISION).unwrap();
        let mut guest = Guest {
            healthy: true,
            provisions: 0,
        };
        assert!(matches!(
            serve(BootMode::Endpoint, input.as_slice(), Vec::new(), &mut guest),
            Err(ProtocolError::UnexpectedRequest)
        ));
        assert_eq!(guest.provisions, 1);
    }

    #[test]
    fn bootstrap_refuses_scripts_mismatched_provision_and_unhealthy_guest() {
        let script = crate::Request::RunScript {
            id: 1,
            timeout_s: 1,
            script: "true".into(),
        };
        let wrong = TrialRequest::DockerTrialProvision {
            challenge: Challenge {
                nonce: [99; 16],
                ..CHALLENGE
            },
        };
        for (healthy, first, second) in [
            (false, HEALTH, None),
            (true, PROVISION, None),
            (true, HEALTH, Some(wrong)),
        ] {
            let mut input = Vec::new();
            write_frame(&mut input, &first).unwrap();
            if let Some(second) = second {
                write_frame(&mut input, &second).unwrap();
            }
            let mut guest = Guest {
                healthy,
                provisions: 0,
            };
            assert!(serve(BootMode::Endpoint, input.as_slice(), Vec::new(), &mut guest).is_err());
            assert_eq!(guest.provisions, 0);
        }
        let mut input = Vec::new();
        write_frame(&mut input, &script).unwrap();
        let mut guest = Guest {
            healthy: true,
            provisions: 0,
        };
        assert!(serve(BootMode::Endpoint, input.as_slice(), Vec::new(), &mut guest).is_err());
        assert_eq!(guest.provisions, 0);
    }
}

#[cfg(test)]
#[test]
fn detached_health_refuses_a_non_tmpfs_mount() {
    let mut health = Health {
        recipe_layout: true,
        media_and_residue_absent: true,
        units_masked: true,
        units_inactive: true,
        swap_disabled: true,
        secret_tmpfs: true,
    };
    let parent_mount = "24 11 0:1 / /run rw,nosuid,nodev,noexec - tmpfs tmpfs rw";
    let exact_mount =
        "36 24 0:2 / /run/arx-compose/secrets rw,nosuid,nodev,noexec - tmpfs tmpfs rw";
    assert!(secret_mount_matches(&format!(
        "{parent_mount}\n{exact_mount}"
    )));
    assert!(health.ready());
    health.secret_tmpfs = secret_mount_matches(parent_mount);
    assert!(!health.ready());
}
