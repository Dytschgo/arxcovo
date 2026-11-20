//! Bounded proof of possession, certificate issuance and private acceptance custody.
#![allow(dead_code)]
pub(super) mod ca;
pub(super) mod csr;
mod custody;
#[cfg(test)]
mod tests;
use super::enrollment::{ClusterId, NodeId};
pub const MAX_CSR: usize = 4096;
pub fn node_uri(cluster: ClusterId, node: NodeId) -> String {
    format!("arxcovo://cluster/{}/node/{}", hex::encode(cluster), node)
}
pub fn parse_node_uri(uri: &str) -> Option<(ClusterId, NodeId)> {
    if uri.len() > 77 {
        return None;
    }
    let tail = uri.strip_prefix("arxcovo://cluster/")?;
    let (cluster, node) = tail.split_once("/node/")?;
    if cluster.len() != 32
        || !cluster
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || node.starts_with('0')
        || !node.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let mut id = [0; 16];
    hex::decode_to_slice(cluster, &mut id).ok()?;
    Some((id, NodeId::new(node.parse().ok()?)?))
}
#[cfg(test)]
mod tls_tests;
pub(super) mod verify;

#[cfg(test)]
pub(super) use tls_tests::*;

pub(super) mod preproof;
#[cfg(test)]
pub(super) mod preproof_tests;

#[cfg(target_os = "linux")]
pub(super) use custody::MemberCopy;
#[cfg(all(test, target_os = "linux"))]
pub(super) use custody::member_copy;
#[cfg(all(test, target_os = "linux"))]
pub(super) use custody::publication_fixture;
