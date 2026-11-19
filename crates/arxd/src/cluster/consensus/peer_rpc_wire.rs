//! Closed ARXP/v2 syntax. Authentication and dispatch belong to the owner.
pub(super) use super::peer_wire::{HEADER_LEN, Route, WireError};
use super::peer_wire::{get_id, get_optional, get_vote, parsed, put_id, put_optional, put_vote};
use super::*;
use crate::cluster::enrollment::{ClusterId, codec::Reader};
use openraft::raft::{
    AppendEntriesRequest, AppendResult, SnapshotResponse, VoteRequest, VoteResponse,
};
use openraft::{Snapshot, StoredMembership};
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Kind {
    AppendRequest = 1,
    AppendReply,
    VoteRequest,
    VoteReply,
    PreVoteRequest,
    PreVoteReply,
    SnapshotRequest,
    SnapshotReply,
}
impl Kind {
    fn limit(self) -> usize {
        match self {
            Self::AppendRequest => 264068,
            Self::AppendReply => 26,
            Self::VoteRequest | Self::PreVoteRequest => 60,
            Self::VoteReply | Self::PreVoteReply => 43,
            Self::SnapshotRequest => 20551,
            Self::SnapshotReply => 17,
        }
    }
    pub(super) fn reply(self) -> Result<Self, WireError> {
        Ok(match self {
            Self::AppendRequest => Self::AppendReply,
            Self::VoteRequest => Self::VoteReply,
            Self::PreVoteRequest => Self::PreVoteReply,
            Self::SnapshotRequest => Self::SnapshotReply,
            _ => return Err(WireError::Unsupported),
        })
    }
}
pub(super) struct Header {
    pub route: Route,
    pub kind: Kind,
    pub payload_len: usize,
}
pub(super) fn inspect_header(bytes: &[u8]) -> Result<Header, WireError> {
    if bytes.len() != HEADER_LEN {
        return Err(WireError::Malformed);
    }
    let mut r = Reader(bytes);
    if parsed(r.bytes(4))? != b"ARXP" || parsed(r.bytes(1))?[0] != 2 {
        return Err(WireError::Unsupported);
    }
    let kind = match parsed(r.bytes(1))?[0] {
        1 => Kind::AppendRequest,
        2 => Kind::AppendReply,
        3 => Kind::VoteRequest,
        4 => Kind::VoteReply,
        5 => Kind::PreVoteRequest,
        6 => Kind::PreVoteReply,
        7 => Kind::SnapshotRequest,
        8 => Kind::SnapshotReply,
        _ => return Err(WireError::Unsupported),
    };
    if parsed(r.u16())? != 0 {
        return Err(WireError::Malformed);
    }
    let payload_len = u32::from_be_bytes(parsed(r.array())?) as usize;
    if payload_len > kind.limit() {
        return Err(WireError::Limit);
    }
    let route = Route {
        cluster: parsed(r.array())?,
        source: parsed(r.u64())?,
        target: parsed(r.u64())?,
    };
    if route.source == 0 || route.target == 0 {
        return Err(WireError::Malformed);
    }
    Ok(Header {
        route,
        kind,
        payload_len,
    })
}
fn frame(route: Route, kind: Kind, payload: &[u8]) -> Result<Vec<u8>, WireError> {
    if payload.len() > kind.limit() {
        return Err(WireError::Limit);
    }
    if route.source == 0 || route.target == 0 {
        return Err(WireError::Malformed);
    }
    let mut b = Vec::with_capacity(HEADER_LEN + payload.len());
    b.extend(b"ARXP");
    b.extend([2, kind as u8, 0, 0]);
    b.extend((payload.len() as u32).to_be_bytes());
    b.extend(route.cluster);
    b.extend(route.source.to_be_bytes());
    b.extend(route.target.to_be_bytes());
    b.extend(payload);
    Ok(b)
}
fn body(bytes: &[u8], kind: Kind) -> Result<(Route, &[u8]), WireError> {
    if bytes.len() < HEADER_LEN || bytes.len() > 264112 {
        return Err(WireError::Limit);
    }
    let h = inspect_header(&bytes[..HEADER_LEN])?;
    if h.kind != kind {
        return Err(WireError::Unsupported);
    }
    if bytes.len() != HEADER_LEN + h.payload_len {
        return Err(WireError::Malformed);
    }
    Ok((h.route, &bytes[HEADER_LEN..]))
}
fn end(r: &Reader<'_>) -> Result<(), WireError> {
    if r.0.is_empty() {
        Ok(())
    } else {
        Err(WireError::Malformed)
    }
}
pub(super) fn encode_request(
    route: Route,
    r: &AppendEntriesRequest<Arx>,
) -> Result<Vec<u8>, WireError> {
    frame(
        route,
        Kind::AppendRequest,
        &peer_wire::encode_append_body(r)?,
    )
}
pub(super) fn decode_request(b: &[u8]) -> Result<(Route, AppendEntriesRequest<Arx>), WireError> {
    let (r, b) = body(b, Kind::AppendRequest)?;
    Ok((r, peer_wire::decode_append_body(b)?))
}
pub(super) fn encode_reply(route: Route, r: &AppendResult<Arx>) -> Result<Vec<u8>, WireError> {
    frame(
        route,
        Kind::AppendReply,
        &peer_wire::encode_append_reply_body(r)?,
    )
}
pub(super) fn decode_reply(b: &[u8]) -> Result<(Route, AppendResult<Arx>), WireError> {
    let (r, b) = body(b, Kind::AppendReply)?;
    Ok((r, peer_wire::decode_append_reply_body(b)?))
}
pub(super) fn encode_vote(
    route: Route,
    kind: Kind,
    r: &VoteRequest<Arx>,
) -> Result<Vec<u8>, WireError> {
    if !matches!(kind, Kind::VoteRequest | Kind::PreVoteRequest) {
        return Err(WireError::Unsupported);
    }
    let mut b = Vec::new();
    put_vote(&mut b, r.vote);
    put_optional(&mut b, r.last_log_id)?;
    b.push(u8::from(r.leadership_transfer_from.is_some()));
    if let Some(v) = r.leadership_transfer_from {
        put_vote(&mut b, v)
    }
    frame(route, kind, &b)
}
pub(super) fn decode_vote(b: &[u8], kind: Kind) -> Result<(Route, VoteRequest<Arx>), WireError> {
    if !matches!(kind, Kind::VoteRequest | Kind::PreVoteRequest) {
        return Err(WireError::Unsupported);
    }
    let (route, b) = body(b, kind)?;
    let mut r = Reader(b);
    let vote = get_vote(&mut r)?;
    let last_log_id = get_optional(&mut r)?;
    let leadership_transfer_from = if parsed(r.flag())? {
        Some(get_vote(&mut r)?)
    } else {
        None
    };
    end(&r)?;
    Ok((
        route,
        VoteRequest {
            vote,
            last_log_id,
            leadership_transfer_from,
        },
    ))
}
pub(super) fn encode_vote_reply(
    route: Route,
    kind: Kind,
    r: &VoteResponse<Arx>,
) -> Result<Vec<u8>, WireError> {
    if !matches!(kind, Kind::VoteReply | Kind::PreVoteReply) {
        return Err(WireError::Unsupported);
    }
    let mut b = Vec::new();
    put_vote(&mut b, r.vote);
    b.push(u8::from(r.vote_granted));
    put_optional(&mut b, r.last_log_id)?;
    frame(route, kind, &b)
}
pub(super) fn decode_vote_reply(
    b: &[u8],
    kind: Kind,
) -> Result<(Route, VoteResponse<Arx>), WireError> {
    if !matches!(kind, Kind::VoteReply | Kind::PreVoteReply) {
        return Err(WireError::Unsupported);
    }
    let (route, b) = body(b, kind)?;
    let mut r = Reader(b);
    let vote = get_vote(&mut r)?;
    let vote_granted = parsed(r.flag())?;
    let last_log_id = get_optional(&mut r)?;
    end(&r)?;
    Ok((
        route,
        VoteResponse {
            vote,
            vote_granted,
            last_log_id,
        },
    ))
}
fn put_membership(
    b: &mut Vec<u8>,
    m: &StoredMembership<LeaderId, Members>,
) -> Result<(), WireError> {
    b.push(u8::from(m.log_id().is_some()));
    if let Some(p) = m.log_id() {
        put_id(b, *p)?;
        let v = encode(m.membership()).map_err(|_| WireError::Limit)?;
        b.extend((v.bytes().len() as u16).to_be_bytes());
        b.extend(v.bytes());
    } else if m != &StoredMembership::default() {
        return Err(WireError::Malformed);
    }
    Ok(())
}
fn get_membership(r: &mut Reader<'_>) -> Result<StoredMembership<LeaderId, Members>, WireError> {
    if !parsed(r.flag())? {
        return Ok(StoredMembership::default());
    }
    let p = get_id(r)?;
    let n = parsed(r.u16())? as usize;
    if !(1..=4096).contains(&n) {
        return Err(WireError::Limit);
    }
    let b = parsed(r.bytes(n))?;
    WireMembership::decode(b).map_err(|_| WireError::Malformed)?;
    let b = MembershipBlob::new(b.to_vec()).map_err(|_| WireError::Malformed)?;
    Ok(StoredMembership::new(
        Some(p),
        decode(&b).map_err(|_| WireError::Malformed)?,
    ))
}
pub(super) fn inspect_snapshot(
    cluster: ClusterId,
    v: Vote<LeaderId>,
    s: &Snapshot<super::LeaderId, super::Members, Vec<u8>>,
) -> Result<(), WireError> {
    if s.snapshot.len() > 16384 {
        return Err(WireError::Limit);
    }
    let (p, m) =
        StateStore::inspect_snapshot(&s.snapshot, cluster).map_err(|_| WireError::Malformed)?;
    let p = p.ok_or(WireError::Malformed)?;
    if Some(id(p)) != s.meta.last_log_id || id(p).leader_id > v.leader_id {
        return Err(WireError::Malformed);
    }
    let expected = match m {
        Some((p, m)) => {
            StoredMembership::new(Some(id(p)), decode(&m).map_err(|_| WireError::Malformed)?)
        }
        None => StoredMembership::default(),
    };
    if expected != s.meta.last_membership {
        return Err(WireError::Malformed);
    }
    Ok(())
}
pub(super) fn encode_snapshot(
    route: Route,
    v: Vote<LeaderId>,
    s: &Snapshot<super::LeaderId, super::Members, Vec<u8>>,
) -> Result<Vec<u8>, WireError> {
    inspect_snapshot(route.cluster, v, s)?;
    let mut b = Vec::new();
    put_vote(&mut b, v);
    put_optional(&mut b, s.meta.last_log_id)?;
    put_membership(&mut b, &s.meta.last_membership)?;
    b.extend((s.snapshot.len() as u16).to_be_bytes());
    b.extend(&s.snapshot);
    frame(route, Kind::SnapshotRequest, &b)
}
type DecodedSnapshot = (Route, Vote<LeaderId>, Snapshot<LeaderId, Members, Vec<u8>>);
pub(super) fn decode_snapshot(b: &[u8]) -> Result<DecodedSnapshot, WireError> {
    let (route, b) = body(b, Kind::SnapshotRequest)?;
    let mut r = Reader(b);
    let v = get_vote(&mut r)?;
    let last_log_id = get_optional(&mut r)?;
    let last_membership = get_membership(&mut r)?;
    let n = parsed(r.u16())? as usize;
    if n > 16384 {
        return Err(WireError::Limit);
    }
    let blob = parsed(r.bytes(n))?.to_vec();
    end(&r)?;
    let s = Snapshot {
        meta: openraft::SnapshotMeta {
            last_log_id,
            last_membership,
        },
        snapshot: blob,
    };
    inspect_snapshot(route.cluster, v, &s)?;
    Ok((route, v, s))
}
pub(super) fn encode_snapshot_reply(
    route: Route,
    r: &SnapshotResponse<Arx>,
) -> Result<Vec<u8>, WireError> {
    let mut b = Vec::new();
    put_vote(&mut b, r.vote);
    frame(route, Kind::SnapshotReply, &b)
}
pub(super) fn decode_snapshot_reply(b: &[u8]) -> Result<(Route, SnapshotResponse<Arx>), WireError> {
    let (route, b) = body(b, Kind::SnapshotReply)?;
    let mut r = Reader(b);
    let vote = get_vote(&mut r)?;
    end(&r)?;
    Ok((route, SnapshotResponse { vote }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn route() -> Route {
        Route {
            cluster: [3; 16],
            source: 7,
            target: 9,
        }
    }
    fn literal(k: u8, b: &[u8]) -> Vec<u8> {
        let mut v = b"ARXP\x02".to_vec();
        v.extend([k, 0, 0]);
        v.extend((b.len() as u32).to_be_bytes());
        v.extend([3; 16]);
        v.extend(7u64.to_be_bytes());
        v.extend(9u64.to_be_bytes());
        v.extend(b);
        v
    }
    fn ballot() -> Vote<LeaderId> {
        vote(StoredVote {
            term: 11,
            node: 7,
            committed: false,
        })
    }
    #[test]
    fn literal_vote_and_prevote_full_fields_and_reply_goldens() {
        let p = id(LogPos {
            term: 8,
            leader: 2,
            index: 0,
        });
        let old = vote(StoredVote {
            term: 9,
            node: 4,
            committed: true,
        });
        let r = VoteRequest {
            vote: ballot(),
            last_log_id: Some(p),
            leadership_transfer_from: Some(old),
        };
        let mut b = [11u64.to_be_bytes(), 7u64.to_be_bytes()].concat();
        b.extend([0, 1]);
        for n in [8u64, 2, 0] {
            b.extend(n.to_be_bytes())
        }
        b.push(1);
        for n in [9u64, 4] {
            b.extend(n.to_be_bytes())
        }
        b.push(1);
        for (k, t) in [(Kind::VoteRequest, 3), (Kind::PreVoteRequest, 5)] {
            let expected = literal(t, &b);
            assert_eq!(encode_vote(route(), k, &r).unwrap(), expected);
            let (_, q) = decode_vote(&expected, k).unwrap();
            assert_eq!(q.vote, r.vote);
            assert_eq!(q.last_log_id, r.last_log_id);
            assert_eq!(q.leadership_transfer_from, r.leadership_transfer_from);
            assert!(
                decode_vote(
                    &expected,
                    if t == 3 {
                        Kind::PreVoteRequest
                    } else {
                        Kind::VoteRequest
                    }
                )
                .is_err()
            );
        }
        for grant in [false, true] {
            let r = VoteResponse {
                vote: old,
                last_log_id: Some(p),
                vote_granted: grant,
            };
            let mut b = [9u64.to_be_bytes(), 4u64.to_be_bytes()].concat();
            b.extend([1, u8::from(grant), 1]);
            for n in [8u64, 2, 0] {
                b.extend(n.to_be_bytes())
            }
            for (k, t) in [(Kind::VoteReply, 4), (Kind::PreVoteReply, 6)] {
                let expected = literal(t, &b);
                assert_eq!(encode_vote_reply(route(), k, &r).unwrap(), expected);
                let (_, q) = decode_vote_reply(&expected, k).unwrap();
                assert_eq!(q, r)
            }
        }
    }
    #[test]
    fn literal_default_options_snapshot_reply_and_strict_refusals() {
        let r = VoteRequest {
            vote: ballot(),
            last_log_id: None,
            leadership_transfer_from: None,
        };
        let mut b = [11u64.to_be_bytes(), 7u64.to_be_bytes()].concat();
        b.extend([0, 0, 0]);
        let expected = literal(5, &b);
        assert_eq!(
            encode_vote(route(), Kind::PreVoteRequest, &r).unwrap(),
            expected
        );
        for i in 0..expected.len() {
            assert!(decode_vote(&expected[..i], Kind::PreVoteRequest).is_err());
        }
        for (offset, value) in [(4, 1), (4, 3), (5, 9), (6, 1), (60, 2), (61, 2), (62, 2)] {
            let mut x = expected.clone();
            x[offset] = value;
            assert!(decode_vote(&x, Kind::PreVoteRequest).is_err());
        }
        let mut x = expected.clone();
        x.push(0);
        assert!(decode_vote(&x, Kind::PreVoteRequest).is_err());
        for (k, max) in [
            (1, 264068u32),
            (2, 26),
            (3, 60),
            (4, 43),
            (5, 60),
            (6, 43),
            (7, 20551),
            (8, 17),
        ] {
            let mut h = literal(k, &[]);
            h[8..12].copy_from_slice(&(max + 1).to_be_bytes());
            assert!(matches!(inspect_header(&h), Err(WireError::Limit)));
        }
        let response = SnapshotResponse { vote: ballot() };
        let b = [
            11u64.to_be_bytes().as_slice(),
            7u64.to_be_bytes().as_slice(),
            &[0],
        ]
        .concat();
        assert_eq!(
            encode_snapshot_reply(route(), &response).unwrap(),
            literal(8, &b)
        );
        assert_eq!(
            decode_snapshot_reply(&literal(8, &b)).unwrap().1.vote,
            ballot()
        );
    }
}
#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use crate::service::application::tests::private_temp;
    #[test]
    fn snapshot_literal_layout_joint_membership_native_limits_and_binding() {
        let dir = private_temp();
        let c = [3; 16];
        let mut sm = StateStore::open(
            dir.path(),
            Init {
                cluster: c,
                first_free: crate::cluster::enrollment::NodeId::new(1).unwrap(),
            },
        )
        .unwrap();
        let p = LogPos {
            term: 1,
            leader: 7,
            index: 0,
        };
        sm.apply(p, &StoredEntry::Blank).unwrap();
        let members = Members::new(
            vec![BTreeSet::from([1]), BTreeSet::from([2])],
            BTreeMap::from([
                (1, BasicNode::new("a:1")),
                (2, BasicNode::new("b:2")),
                (3, BasicNode::new("c:3")),
            ]),
        )
        .unwrap();
        let mut canonical = vec![1, 2, 1];
        canonical.extend(1u64.to_be_bytes());
        canonical.push(1);
        canonical.extend(2u64.to_be_bytes());
        canonical.push(3);
        for (n, a) in [(1u64, b"a:1"), (2, b"b:2"), (3, b"c:3")] {
            canonical.extend(n.to_be_bytes());
            canonical.push(3);
            canonical.extend(a)
        }
        let membership = MembershipBlob::new(canonical.clone()).unwrap();
        let mp = LogPos { index: 1, ..p };
        sm.apply(mp, &StoredEntry::Membership(membership)).unwrap();
        let v = vote(StoredVote {
            term: 9,
            node: 7,
            committed: true,
        });
        let blob = sm.snapshot();
        let native = Snapshot {
            meta: openraft::SnapshotMeta {
                last_log_id: Some(id(mp)),
                last_membership: StoredMembership::new(Some(id(mp)), members),
            },
            snapshot: blob.clone(),
        };
        let mut payload = [9u64.to_be_bytes(), 7u64.to_be_bytes()].concat();
        payload.extend([1, 1]);
        for n in [1u64, 7, 1] {
            payload.extend(n.to_be_bytes())
        }
        payload.push(1);
        for n in [1u64, 7, 1] {
            payload.extend(n.to_be_bytes())
        }
        payload.extend((canonical.len() as u16).to_be_bytes());
        payload.extend(canonical);
        payload.extend((blob.len() as u16).to_be_bytes());
        payload.extend(&blob);
        let rt = Route {
            cluster: c,
            source: 7,
            target: 9,
        };
        let mut literal = b"ARXP\x02\x07\0\0".to_vec();
        literal.extend((payload.len() as u32).to_be_bytes());
        literal.extend(c);
        literal.extend(7u64.to_be_bytes());
        literal.extend(9u64.to_be_bytes());
        literal.extend(payload);
        assert_eq!(encode_snapshot(rt, v, &native).unwrap(), literal);
        let (_, rv, s) = decode_snapshot(&literal).unwrap();
        assert_eq!(rv, v);
        assert_eq!(s.meta, native.meta);
        assert_eq!(s.snapshot, blob);
        let mut bad = native.clone();
        bad.meta.last_log_id = None;
        assert!(encode_snapshot(rt, v, &bad).is_err());
        bad = native.clone();
        bad.meta.last_membership = StoredMembership::default();
        assert!(encode_snapshot(rt, v, &bad).is_err());
        let mut wrong = rt;
        wrong.cluster = [4; 16];
        assert!(encode_snapshot(wrong, v, &native).is_err());
        assert!(
            inspect_snapshot(
                c,
                vote(StoredVote {
                    term: 0,
                    node: 7,
                    committed: true
                }),
                &native
            )
            .is_err()
        );
        for n in [16384, 16385] {
            let mut bad = native.clone();
            bad.snapshot = vec![0; n];
            assert!(encode_snapshot(rt, v, &bad).is_err());
        }
        let mut invalid_members = Vec::new();
        assert!(
            put_membership(
                &mut invalid_members,
                &StoredMembership::new(None, native.meta.last_membership.membership().clone())
            )
            .is_err()
        );
        let too_long = Members::new(
            vec![BTreeSet::from([1])],
            BTreeMap::from([(1, BasicNode::new("a".repeat(201)))]),
        )
        .unwrap();
        let mut b = Vec::new();
        assert!(put_membership(&mut b, &StoredMembership::new(Some(id(mp)), too_long)).is_err());
    }
}
