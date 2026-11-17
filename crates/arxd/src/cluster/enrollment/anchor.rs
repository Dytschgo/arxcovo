//! Canonical borrowed voter observation; not a pending-code authorization query.
use super::*;

/// Uniform refusal; eligibility details are deliberately not exposed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) struct AnchorQueryRefused;
impl std::fmt::Display for AnchorQueryRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("anchor query refused")
    }
}
impl EnrollmentState {
    /// Pure local observation, without pruning or a freshness/unspent promise.
    pub(in crate::cluster) fn pending_voters(
        &self,
        cluster: ClusterId,
        code: CodeId,
        now_ms: u64,
        membership: Option<&Membership>,
    ) -> Result<BoundVoters, AnchorQueryRefused> {
        if !self.founded
            || cluster != self.cluster
            || now_ms > MAX_TIME_MS
            || now_ms
                .checked_add(SKEW_MS)
                .is_none_or(|t| t < self.committed_ms)
        {
            return Err(AnchorQueryRefused);
        }
        let usable = now_ms
            .max(self.committed_ms)
            .checked_add(2 * SKEW_MS)
            .ok_or(AnchorQueryRefused)?;
        let record = self.records.get(&code).ok_or(AnchorQueryRefused)?;
        if record.consumed.is_some() || usable >= record.expires_ms {
            return Err(AnchorQueryRefused);
        }
        BoundVoters::observe(self, membership.ok_or(AnchorQueryRefused)?)
            .map_err(|_| AnchorQueryRefused)
    }
}

#[cfg(test)]
mod pending_query_tables {
    use super::*;
    fn fixture() -> (EnrollmentState, Membership, JoinCode) {
        let mut s = EnrollmentState::new([1; 16], NodeId::new(2).unwrap());
        let m = Membership::new(vec![[1].into()], [(1, "a".into())].into()).unwrap();
        s.founded = true;
        s.members
            .insert(NodeId::new(1).unwrap(), KeyDigest([1; 32]));
        let code = JoinCode::generate(
            [1; 16],
            [2; 32],
            BoundVoters::observe(&s, &m).unwrap().anchor([1; 16]),
            |b| {
                b.fill(9);
                Ok::<_, ()>(())
            },
        )
        .unwrap();
        assert!(matches!(
            s.apply(
                &Command::Mint {
                    cluster: [1; 16],
                    actor: [3; 16],
                    request: [4; 16],
                    code: code.code_id(),
                    verifier: code.verifier(),
                    proposed_at_ms: 1_000_000,
                },
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 0
                },
                Some(&m)
            ),
            Outcome::Minted { replay: false, .. }
        ));
        (s, m, code)
    }
    #[test]
    fn pending_query_eligibility_and_immutability() {
        let (s, m, code) = fixture();
        let mut expected = vec![1];
        expected.extend(1u64.to_be_bytes());
        expected.extend([1; 32]);
        for (cluster, id, now, ok) in [
            ([1; 16], code.code_id(), 1_000_000, true),
            ([2; 16], code.code_id(), 1_000_000, false),
            ([1; 16], [0; 16], 1_000_000, false),
            ([1; 16], code.code_id(), 970_000, true),
            ([1; 16], code.code_id(), 969_999, false),
            ([1; 16], code.code_id(), 1_839_999, true),
            ([1; 16], code.code_id(), 1_840_000, false),
            ([1; 16], code.code_id(), MAX_TIME_MS, false),
            ([1; 16], code.code_id(), MAX_TIME_MS + 1, false),
            ([1; 16], code.code_id(), u64::MAX, false),
        ] {
            let before = s.clone();
            let result = s.pending_voters(cluster, id, now, Some(&m));
            assert_eq!(result.is_ok(), ok);
            if let Ok(v) = result {
                assert_eq!(v.encode(), expected);
            }
            assert_eq!(s, before);
        }
        let mut consumed = s.clone();
        assert!(matches!(
            consumed.apply(
                &Command::redeem(
                    [1; 16],
                    [5; 16],
                    code.code_id(),
                    &code.proof(),
                    KeyDigest([2; 32]),
                    code.anchor(),
                    1_000_000
                ),
                LogPos {
                    term: 1,
                    leader: 1,
                    index: 1
                },
                Some(&m)
            ),
            Outcome::Enrolled(_)
        ));
        let before = consumed.clone();
        assert!(
            consumed
                .pending_voters([1; 16], code.code_id(), 1_000_000, Some(&m))
                .is_err()
        );
        assert_eq!(consumed, before);
    }
    #[test]
    fn pending_query_membership_and_foundation() {
        let (mut s, m, c) = fixture();
        for membership in [
            None,
            Some(Membership::new(vec![[1].into(), [1].into()], [(1, "a".into())].into()).unwrap()),
            Some(Membership::new(vec![[2].into()], [(2, "b".into())].into()).unwrap()),
        ] {
            let before = s.clone();
            assert!(
                s.pending_voters([1; 16], c.code_id(), 1_000_000, membership.as_ref())
                    .is_err()
            );
            assert_eq!(s, before);
        }
        s.founded = false;
        let before = s.clone();
        assert!(
            s.pending_voters([1; 16], c.code_id(), 1_000_000, Some(&m))
                .is_err()
        );
        assert_eq!(s, before);
    }
    #[test]
    fn pending_query_canonical_max_and_overflow() {
        let (mut s, _, c) = fixture();
        s.members = (1..=16)
            .map(|i| (NodeId::new(i).unwrap(), KeyDigest([i as u8; 32])))
            .collect();
        let m = Membership::new(
            vec![(1..=16).collect()],
            (1..=16).map(|i| (i, format!("node{i}"))).collect(),
        )
        .unwrap();
        let mut expected = vec![16];
        for i in 1u64..=16 {
            expected.extend(i.to_be_bytes());
            expected.extend([i as u8; 32]);
        }
        assert_eq!(
            s.pending_voters([1; 16], c.code_id(), 1_000_000, Some(&m))
                .unwrap()
                .encode(),
            expected
        );
        let changed = Membership::new(
            vec![[1].into()],
            [(1, "changed".into()), (2, "learner".into())].into(),
        )
        .unwrap();
        assert_eq!(
            s.pending_voters([1; 16], c.code_id(), 1_000_000, Some(&changed))
                .unwrap()
                .encode(),
            expected[..41]
                .iter()
                .enumerate()
                .map(|(i, b)| if i == 0 { 1 } else { *b })
                .collect::<Vec<_>>()
        );
        s.committed_ms = u64::MAX;
        let before = s.clone();
        assert!(
            s.pending_voters([1; 16], c.code_id(), MAX_TIME_MS, Some(&m))
                .is_err()
        );
        assert_eq!(s, before);
    }
}

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
