//! Canonical borrowed voter observation; not a pending-code authorization query.
use super::*;

pub(in crate::cluster) struct BoundVoters(Vec<(NodeId, KeyDigest)>);
impl BoundVoters {
    /// Both arguments must come from the same locked store image.
    pub(in crate::cluster) fn observe(
        state: &EnrollmentState,
        membership: &Membership,
    ) -> Result<Self, Reject> {
        let [voters] = membership.configs() else {
            return Err(Reject::AnchorChanged);
        };
        if !(1..=16).contains(&voters.len()) {
            return Err(Reject::AnchorChanged);
        }
        let mut pairs = Vec::with_capacity(voters.len());
        for voter in voters {
            let node = NodeId::new(*voter).ok_or(Reject::AnchorChanged)?;
            let key = *state.members.get(&node).ok_or(Reject::AnchorChanged)?;
            if pairs.iter().any(|(_, k)| *k == key) {
                return Err(Reject::AnchorChanged);
            }
            pairs.push((node, key));
        }
        Ok(Self(pairs))
    }
    pub(in crate::cluster) fn decode(bytes: &[u8]) -> Result<Self, Reject> {
        let count = usize::from(*bytes.first().ok_or(Reject::AnchorChanged)?);
        if !(1..=16).contains(&count) || bytes.len() != 1 + 40 * count {
            return Err(Reject::AnchorChanged);
        }
        let mut pairs = Vec::with_capacity(count);
        let mut previous = 0;
        for pair in bytes[1..].chunks_exact(40) {
            let id = u64::from_be_bytes(pair[..8].try_into().map_err(|_| Reject::AnchorChanged)?);
            let node = NodeId::new(id).ok_or(Reject::AnchorChanged)?;
            let key = KeyDigest(pair[8..].try_into().map_err(|_| Reject::AnchorChanged)?);
            if id <= previous || pairs.iter().any(|(_, k)| *k == key) {
                return Err(Reject::AnchorChanged);
            }
            previous = id;
            pairs.push((node, key));
        }
        Ok(Self(pairs))
    }
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "internal probe has no production query caller")
    )]
    pub(in crate::cluster) fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(1 + 40 * self.0.len());
        bytes.push(self.0.len() as u8);
        for (node, key) in &self.0 {
            bytes.extend(node.get().to_be_bytes());
            bytes.extend(key.as_bytes());
        }
        bytes
    }
    pub(in crate::cluster) fn anchor(&self, cluster: ClusterId) -> JoinAnchor {
        let mut hash = Sha256::new()
            .chain_update(b"arxcovo/join/anchor/v1")
            .chain_update(cluster)
            .chain_update([self.0.len() as u8]);
        for (node, key) in &self.0 {
            hash = hash
                .chain_update(node.get().to_be_bytes())
                .chain_update(key.as_bytes());
        }
        JoinAnchor(hash.finalize().into())
    }
    pub(in crate::cluster) fn contains(&self, node: NodeId, key: KeyDigest) -> bool {
        self.0.contains(&(node, key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wire(n: u8) -> Vec<u8> {
        let mut b = vec![n];
        for i in 1..=n {
            b.extend(u64::from(i).to_be_bytes());
            b.extend([i; 32]);
        }
        b
    }
    #[test]
    fn canonical_bounds_and_order() {
        for n in 1..=16 {
            let b = wire(n);
            assert_eq!(BoundVoters::decode(&b).unwrap().encode(), b);
            for end in 0..b.len() {
                assert!(BoundVoters::decode(&b[..end]).is_err());
            }
            let mut suffix = b;
            suffix.push(0);
            assert!(BoundVoters::decode(&suffix).is_err());
        }
        for b in [wire(0), wire(17), vec![0; 642]] {
            assert!(BoundVoters::decode(&b).is_err());
        }
        for id in [0u64, 1] {
            let mut b = wire(2);
            b[41..49].copy_from_slice(&id.to_be_bytes());
            assert!(BoundVoters::decode(&b).is_err());
        }
        let mut b = wire(2);
        b[49..81].fill(1);
        assert!(BoundVoters::decode(&b).is_err());
        let mut b = wire(2);
        b[1..9].copy_from_slice(&3u64.to_be_bytes());
        assert!(BoundVoters::decode(&b).is_err());
    }
    #[test]
    fn observation_excludes_addresses_and_learners() {
        let mut state = EnrollmentState::new([1; 16], NodeId::new(3).unwrap());
        state
            .members
            .insert(NodeId::new(1).unwrap(), KeyDigest([1; 32]));
        let membership =
            |configs| Membership::new(configs, [(1, "a".into()), (2, "b".into())].into()).unwrap();
        let m = membership(vec![[1].into()]);
        let a = BoundVoters::observe(&state, &m).unwrap().encode();
        let changed = Membership::new(vec![[1].into()], [(1, "changed".into())].into()).unwrap();
        assert_eq!(BoundVoters::observe(&state, &changed).unwrap().encode(), a);
        assert!(BoundVoters::observe(&state, &membership(vec![[1].into(), [1].into()])).is_err());
        assert!(BoundVoters::observe(&state, &membership(vec![[1, 2].into()])).is_err());
        let hash = BoundVoters::decode(&a).unwrap().anchor([1; 16]);
        state
            .members
            .insert(NodeId::new(1).unwrap(), KeyDigest([2; 32]));
        assert_ne!(
            BoundVoters::observe(&state, &m).unwrap().anchor([1; 16]),
            hash
        );
        assert_ne!(BoundVoters::decode(&a).unwrap().anchor([2; 16]), hash);
    }
}
