//! Bounded proof of possession and test-only certificate issuance; no transport.
#![allow(dead_code)]
pub(super) mod ca;
pub(super) mod csr;
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
