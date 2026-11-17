use super::{CaFingerprint, ClusterId, CodeId, JoinAnchor, MAX_CODE_TEXT};
use sha2::{Digest, Sha256};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeError {
    Malformed,
    Rng,
}
impl fmt::Display for CodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed join code",
            Self::Rng => "join code randomness failed",
        })
    }
}
impl std::error::Error for CodeError {}
#[derive(Clone, PartialEq, Eq)]
pub struct JoinSecret([u8; 32]);
#[derive(Clone, PartialEq, Eq)]
pub struct RedeemProof([u8; 32]);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeVerifier([u8; 32]);
#[derive(Clone, PartialEq, Eq)]
pub struct JoinCode {
    cluster: ClusterId,
    code: CodeId,
    secret: JoinSecret,
    ca_fp: CaFingerprint,
    anchor: JoinAnchor,
}
impl fmt::Debug for JoinSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
impl fmt::Debug for RedeemProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
impl fmt::Debug for JoinCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}
impl RedeemProof {
    pub(in crate::cluster) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
impl CodeVerifier {
    pub(super) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
    pub fn of_proof(proof: &RedeemProof) -> Self {
        Self(
            Sha256::new()
                .chain_update(b"arxcovo/join/verifier/v1")
                .chain_update(proof.0)
                .finalize()
                .into(),
        )
    }
    pub(super) fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
}
impl JoinCode {
    /// The caller supplies randomness (production adapter: getrandom::fill).
    pub fn generate<E>(
        cluster: ClusterId,
        ca_fp: CaFingerprint,
        anchor: JoinAnchor,
        mut fill: impl FnMut(&mut [u8]) -> Result<(), E>,
    ) -> Result<Self, CodeError> {
        let mut code = [0; 16];
        let mut secret = [0; 32];
        fill(&mut code).map_err(|_| CodeError::Rng)?;
        fill(&mut secret).map_err(|_| CodeError::Rng)?;
        Ok(Self {
            cluster,
            code,
            secret: JoinSecret(secret),
            ca_fp,
            anchor,
        })
    }
    pub fn cluster(&self) -> ClusterId {
        self.cluster
    }
    pub fn code_id(&self) -> CodeId {
        self.code
    }
    pub fn ca_fingerprint(&self) -> CaFingerprint {
        self.ca_fp
    }
    pub fn anchor(&self) -> JoinAnchor {
        self.anchor
    }
    pub fn proof(&self) -> RedeemProof {
        RedeemProof(
            Sha256::new()
                .chain_update(b"arxcovo/join/proof/v2")
                .chain_update(self.cluster)
                .chain_update(self.code)
                .chain_update(self.secret.0)
                .chain_update(self.anchor.0)
                .finalize()
                .into(),
        )
    }
    pub fn verifier(&self) -> CodeVerifier {
        CodeVerifier::of_proof(&self.proof())
    }
    /// Sensitive plaintext for owner transfer only. Never log or return through MCP.
    pub fn expose_for_owner_transfer(&self) -> String {
        let mut raw = [0; 132];
        raw[..16].copy_from_slice(&self.cluster);
        raw[16..32].copy_from_slice(&self.code);
        raw[32..64].copy_from_slice(&self.secret.0);
        raw[64..96].copy_from_slice(&self.ca_fp);
        raw[96..128].copy_from_slice(self.anchor.as_bytes());
        let check = checksum(&raw[..128]);
        raw[128..].copy_from_slice(&check);
        format!("arxj2-{}", hex::encode(raw))
    }
    pub fn parse(text: &str) -> Result<Self, CodeError> {
        if text.len() > MAX_CODE_TEXT {
            return Err(CodeError::Malformed);
        }
        let text = text.trim_matches(|c: char| c.is_ascii_whitespace());
        let payload = text.strip_prefix("arxj2-").ok_or(CodeError::Malformed)?;
        if payload.len() != 264 || !payload.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(CodeError::Malformed);
        }
        let mut raw = [0; 132];
        hex::decode_to_slice(payload, &mut raw).map_err(|_| CodeError::Malformed)?;
        if raw[128..] != checksum(&raw[..128]) {
            return Err(CodeError::Malformed);
        }
        let mut cluster = [0; 16];
        let mut code = [0; 16];
        let mut secret = [0; 32];
        let mut ca_fp = [0; 32];
        cluster.copy_from_slice(&raw[..16]);
        code.copy_from_slice(&raw[16..32]);
        secret.copy_from_slice(&raw[32..64]);
        ca_fp.copy_from_slice(&raw[64..96]);
        let anchor = JoinAnchor(raw[96..128].try_into().map_err(|_| CodeError::Malformed)?);
        Ok(Self {
            cluster,
            code,
            secret: JoinSecret(secret),
            ca_fp,
            anchor,
        })
    }
}
fn checksum(bytes: &[u8]) -> [u8; 4] {
    let hash = Sha256::new()
        .chain_update(b"arxcovo/join/check/v2")
        .chain_update(bytes)
        .finalize();
    [hash[0], hash[1], hash[2], hash[3]]
}
#[cfg(test)]
pub(super) fn privacy_fixture(code: &JoinCode) -> (&JoinSecret, [u8; 32]) {
    (&code.secret, code.proof().0)
}
