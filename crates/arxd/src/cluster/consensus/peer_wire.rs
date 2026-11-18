//! Bounded syntax only. Routes are untrusted claims, not identity or dispatch authority.
use super::*;
use crate::cluster::enrollment::{ClusterId, codec};
use codec::Reader;
use openraft::raft::{AppendEntriesRequest, AppendError, AppendResult};

pub(super) const HEADER_LEN: usize = 44;
const MAX_ENTRY: usize = 4123;
const MAX_REQUEST: usize = 264068;
const MAX_REPLY: usize = 26;
const MAX_FRAME: usize = HEADER_LEN + MAX_REQUEST;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WireError {
    Malformed,
    Unsupported,
    Limit,
}
impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "malformed peer frame",
            Self::Unsupported => "unsupported peer frame",
            Self::Limit => "peer frame limit",
        })
    }
}
impl std::error::Error for WireError {}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Route {
    pub(super) cluster: ClusterId,
    pub(super) source: u64,
    pub(super) target: u64,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    AppendRequest,
    AppendReply,
}
impl Kind {
    fn tag(self) -> u8 {
        match self {
            Self::AppendRequest => 1,
            Self::AppendReply => 2,
        }
    }
    fn limit(self) -> usize {
        match self {
            Self::AppendRequest => MAX_REQUEST,
            Self::AppendReply => MAX_REPLY,
        }
    }
}
pub(super) struct Header {
    pub(super) route: Route,
    pub(super) kind: Kind,
    pub(super) payload_len: usize,
}
fn parsed<T>(r: Result<T, StoreError>) -> Result<T, WireError> {
    r.map_err(|_| WireError::Malformed)
}

/// Inspect exactly the fixed header before a future reader allocates its body.
pub(super) fn inspect_header(bytes: &[u8]) -> Result<Header, WireError> {
    if bytes.len() != HEADER_LEN {
        return Err(WireError::Malformed);
    }
    let mut r = Reader(bytes);
    if parsed(r.bytes(4))? != b"ARXP" || parsed(r.bytes(1))?[0] != 1 {
        return Err(WireError::Unsupported);
    }
    let kind = match parsed(r.bytes(1))?[0] {
        1 => Kind::AppendRequest,
        2 => Kind::AppendReply,
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
    if route.source == 0 || route.target == 0 {
        return Err(WireError::Malformed);
    }
    if payload.len() > kind.limit() {
        return Err(WireError::Limit);
    }
    let len = HEADER_LEN
        .checked_add(payload.len())
        .ok_or(WireError::Limit)?;
    let mut out = Vec::with_capacity(len);
    out.extend(b"ARXP");
    out.extend([1, kind.tag(), 0, 0]);
    out.extend(
        u32::try_from(payload.len())
            .map_err(|_| WireError::Limit)?
            .to_be_bytes(),
    );
    out.extend(route.cluster);
    out.extend(route.source.to_be_bytes());
    out.extend(route.target.to_be_bytes());
    out.extend(payload);
    Ok(out)
}
fn body(bytes: &[u8], kind: Kind) -> Result<(Route, &[u8]), WireError> {
    if bytes.len() > MAX_FRAME || bytes.len() < HEADER_LEN {
        return Err(WireError::Limit);
    }
    let h = inspect_header(&bytes[..HEADER_LEN])?;
    if h.kind != kind {
        return Err(WireError::Unsupported);
    }
    let end = HEADER_LEN
        .checked_add(h.payload_len)
        .ok_or(WireError::Limit)?;
    if bytes.len() != end {
        return Err(WireError::Malformed);
    }
    Ok((
        h.route,
        bytes.get(HEADER_LEN..end).ok_or(WireError::Malformed)?,
    ))
}
fn put_id(out: &mut Vec<u8>, value: Id) -> Result<(), WireError> {
    if value.index > i64::MAX as u64 {
        return Err(WireError::Limit);
    }
    out.extend(value.leader_id.term.to_be_bytes());
    out.extend(value.leader_id.node_id.to_be_bytes());
    out.extend(value.index.to_be_bytes());
    Ok(())
}
fn get_id(r: &mut Reader<'_>) -> Result<Id, WireError> {
    let p = LogPos {
        term: parsed(r.u64())?,
        leader: parsed(r.u64())?,
        index: parsed(r.u64())?,
    };
    if p.index > i64::MAX as u64 {
        return Err(WireError::Limit);
    }
    Ok(id(p))
}
fn put_optional(out: &mut Vec<u8>, value: Option<Id>) -> Result<(), WireError> {
    out.push(u8::from(value.is_some()));
    if let Some(value) = value {
        put_id(out, value)?;
    }
    Ok(())
}
fn get_optional(r: &mut Reader<'_>) -> Result<Option<Id>, WireError> {
    if parsed(r.flag())? {
        Ok(Some(get_id(r)?))
    } else {
        Ok(None)
    }
}
fn put_vote(out: &mut Vec<u8>, value: Vote<LeaderId>) {
    out.extend(value.leader_id.term.to_be_bytes());
    out.extend(value.leader_id.node_id.to_be_bytes());
    out.push(u8::from(value.committed));
}
fn get_vote(r: &mut Reader<'_>) -> Result<Vote<LeaderId>, WireError> {
    Ok(vote(StoredVote {
        term: parsed(r.u64())?,
        node: parsed(r.u64())?,
        committed: parsed(r.flag())?,
    }))
}
fn entry_bytes(entry: &openraft::type_config::alias::EntryOf<Arx>) -> Result<Vec<u8>, WireError> {
    let mut out = Vec::new();
    put_id(&mut out, entry.log_id)?;
    match &entry.payload {
        EntryPayload::Blank => out.push(0),
        EntryPayload::Normal(command) => codec::encode_command(command, &mut out),
        EntryPayload::Membership(members) => {
            // Parent conversion bounds native sets, nodes and addresses BEFORE cloning.
            let blob = encode(members).map_err(|_| WireError::Limit)?;
            out.push(3);
            out.extend(
                u16::try_from(blob.bytes().len())
                    .map_err(|_| WireError::Limit)?
                    .to_be_bytes(),
            );
            out.extend(blob.bytes());
        }
    }
    if out.len() > MAX_ENTRY {
        return Err(WireError::Limit);
    }
    Ok(out)
}
fn get_entry(bytes: &[u8]) -> Result<openraft::type_config::alias::EntryOf<Arx>, WireError> {
    if bytes.len() > MAX_ENTRY {
        return Err(WireError::Limit);
    }
    let mut r = Reader(bytes);
    let position = get_id(&mut r)?;
    let payload = match parsed(r.bytes(1))?[0] {
        0 => StoredEntry::Blank,
        tag @ (1 | 4 | 5 | 6) => {
            StoredEntry::Enrollment(parsed(codec::decode_command(tag, &mut r))?)
        }
        3 => {
            let len = parsed(r.u16())? as usize;
            if len > 4096 {
                return Err(WireError::Limit);
            }
            let encoded = parsed(r.bytes(len))?;
            // Canonical syntax is checked before copying the bounded blob.
            WireMembership::decode(encoded).map_err(|_| WireError::Malformed)?;
            StoredEntry::Membership(
                MembershipBlob::new(encoded.to_vec()).map_err(|_| WireError::Malformed)?,
            )
        }
        _ => return Err(WireError::Unsupported),
    };
    if !r.0.is_empty() {
        return Err(WireError::Malformed);
    }
    let entry = from_stored((pos(position), payload)).map_err(|_| WireError::Malformed)?;
    if entry_bytes(&entry)? != bytes {
        return Err(WireError::Malformed);
    }
    Ok(entry)
}
pub(super) fn encode_request(
    route: Route,
    request: &AppendEntriesRequest<Arx>,
) -> Result<Vec<u8>, WireError> {
    if request.entries.len() > crate::cluster::log_store::MAX_APPEND {
        return Err(WireError::Limit);
    }
    let mut payload = Vec::new();
    put_vote(&mut payload, request.vote);
    put_optional(&mut payload, request.prev_log_id)?;
    put_optional(&mut payload, request.leader_commit)?;
    payload.push(u8::try_from(request.entries.len()).map_err(|_| WireError::Limit)?);
    for entry in &request.entries {
        let bytes = entry_bytes(entry)?;
        let next = payload
            .len()
            .checked_add(2)
            .and_then(|n| n.checked_add(bytes.len()))
            .ok_or(WireError::Limit)?;
        if next > MAX_REQUEST {
            return Err(WireError::Limit);
        }
        payload.extend(
            u16::try_from(bytes.len())
                .map_err(|_| WireError::Limit)?
                .to_be_bytes(),
        );
        payload.extend(bytes);
    }
    frame(route, Kind::AppendRequest, &payload)
}
pub(super) fn decode_request(
    bytes: &[u8],
) -> Result<(Route, AppendEntriesRequest<Arx>), WireError> {
    let (route, bytes) = body(bytes, Kind::AppendRequest)?;
    let mut r = Reader(bytes);
    let vote = get_vote(&mut r)?;
    let prev_log_id = get_optional(&mut r)?;
    let leader_commit = get_optional(&mut r)?;
    let count = parsed(r.bytes(1))?[0] as usize;
    if count > crate::cluster::log_store::MAX_APPEND {
        return Err(WireError::Limit);
    }
    // The count and whole frame were bounded before any entry collection allocation.
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let len = parsed(r.u16())? as usize;
        if len > MAX_ENTRY {
            return Err(WireError::Limit);
        }
        entries.push(get_entry(parsed(r.bytes(len))?)?);
    }
    if !r.0.is_empty() {
        return Err(WireError::Malformed);
    }
    Ok((
        route,
        AppendEntriesRequest {
            vote,
            prev_log_id,
            entries,
            leader_commit,
        },
    ))
}
pub(super) fn encode_reply(route: Route, reply: &AppendResult<Arx>) -> Result<Vec<u8>, WireError> {
    let mut payload = Vec::new();
    match reply {
        Ok(accepted) => {
            payload.push(0);
            put_optional(&mut payload, *accepted)?;
        }
        Err(AppendError::Conflict { log_id, hint: _ }) => {
            payload.push(1);
            put_id(&mut payload, *log_id)?;
        }
        Err(AppendError::HigherVote(vote)) => {
            payload.push(2);
            put_vote(&mut payload, *vote);
        }
    }
    frame(route, Kind::AppendReply, &payload)
}
pub(super) fn decode_reply(bytes: &[u8]) -> Result<(Route, AppendResult<Arx>), WireError> {
    let (route, bytes) = body(bytes, Kind::AppendReply)?;
    let mut r = Reader(bytes);
    let reply = match parsed(r.bytes(1))?[0] {
        0 => Ok(get_optional(&mut r)?),
        1 => Err(AppendError::Conflict {
            log_id: get_id(&mut r)?,
            hint: None,
        }),
        2 => Err(AppendError::HigherVote(get_vote(&mut r)?)),
        _ => return Err(WireError::Unsupported),
    };
    if !r.0.is_empty() {
        return Err(WireError::Malformed);
    }
    Ok((route, reply))
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
    fn position(index: u64) -> Id {
        id(LogPos {
            term: 11,
            leader: 7,
            index,
        })
    }
    fn ballot() -> Vote<LeaderId> {
        vote(StoredVote {
            term: 11,
            node: 7,
            committed: true,
        })
    }
    fn heartbeat() -> AppendEntriesRequest<Arx> {
        AppendEntriesRequest {
            vote: ballot(),
            prev_log_id: None,
            leader_commit: None,
            entries: vec![],
        }
    }
    // Independent field layout; never calls a production peer encoder.
    fn expected_frame(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut b = b"ARXP\x01".to_vec();
        b.extend([kind, 0, 0]);
        b.extend((payload.len() as u32).to_be_bytes());
        b.extend([3; 16]);
        b.extend(7u64.to_be_bytes());
        b.extend(9u64.to_be_bytes());
        b.extend(payload);
        b
    }
    fn expected_position(index: u64) -> Vec<u8> {
        [11u64.to_be_bytes(), 7u64.to_be_bytes(), index.to_be_bytes()].concat()
    }
    fn prefix() -> Vec<u8> {
        let mut b = [11u64.to_be_bytes(), 7u64.to_be_bytes()].concat();
        b.extend([1, 0, 0]);
        b
    }
    fn payload_cases() -> Vec<Vec<u8>> {
        let mut found = vec![4];
        found.extend([3; 16]);
        found.extend(17u64.to_be_bytes());
        found.push(1);
        found.extend(2u64.to_be_bytes());
        found.extend([8; 32]);
        let mut mint = vec![1];
        for n in [3, 4, 5, 6] {
            mint.extend([n; 16]);
        }
        mint.extend([8; 32]);
        mint.extend(17u64.to_be_bytes());
        let mut redeem = vec![6];
        for n in [3, 5, 6] {
            redeem.extend([n; 16]);
        }
        redeem.extend([8; 32]);
        redeem.extend([9; 32]);
        redeem.extend(17u64.to_be_bytes());
        redeem.extend([10; 32]);
        let mut remove = vec![5];
        remove.extend([3; 16]);
        remove.extend(2u64.to_be_bytes());
        remove.extend(17u64.to_be_bytes());
        let mut m = vec![1, 1, 1];
        m.extend(1u64.to_be_bytes());
        m.push(2);
        for (n, a) in [(1u64, b'a'), (2, b'b')] {
            m.extend(n.to_be_bytes());
            m.extend([1, a]);
        }
        let mut membership = vec![3];
        membership.extend((m.len() as u16).to_be_bytes());
        membership.extend(m);
        vec![vec![0], found, mint, redeem, remove, membership]
    }
    fn native_payload(bytes: &[u8]) -> EntryPayload<Command, Members> {
        let mut r = Reader(&bytes[1..]);
        match bytes[0] {
            0 => EntryPayload::Blank,
            3 => {
                let n = r.u16().unwrap() as usize;
                let blob = MembershipBlob::new(r.bytes(n).unwrap().to_vec()).unwrap();
                EntryPayload::Membership(decode(&blob).unwrap())
            }
            tag => EntryPayload::Normal(codec::decode_command(tag, &mut r).unwrap()),
        }
    }
    fn mixed() -> (AppendEntriesRequest<Arx>, Vec<u8>) {
        let cases = payload_cases();
        let mut payload = prefix();
        payload.push(cases.len() as u8);
        let mut request = heartbeat();
        for (i, bytes) in cases.iter().enumerate() {
            let mut row = expected_position(i as u64);
            row.extend(bytes);
            payload.extend((row.len() as u16).to_be_bytes());
            payload.extend(row);
            request.entries.push(Entry {
                log_id: position(i as u64),
                payload: native_payload(bytes),
            });
        }
        (request, expected_frame(1, &payload))
    }
    fn same(a: &AppendEntriesRequest<Arx>, b: &AppendEntriesRequest<Arx>) -> bool {
        a.vote == b.vote
            && a.prev_log_id == b.prev_log_id
            && a.leader_commit == b.leader_commit
            && a.entries.len() == b.entries.len()
            && a.entries
                .iter()
                .zip(&b.entries)
                .all(|(a, b)| to_stored(a.clone()).unwrap() == to_stored(b.clone()).unwrap())
    }
    #[test]
    fn independent_heartbeat_and_mixed_entry_bytes() {
        let mut payload = prefix();
        payload.push(0);
        let expected = expected_frame(1, &payload);
        assert!(encode_request(route(), &heartbeat()).unwrap() == expected);
        let (r, decoded) = decode_request(&expected).unwrap();
        assert!(r == route() && same(&decoded, &heartbeat()));
        let (request, expected) = mixed();
        assert!(encode_request(route(), &request).unwrap() == expected);
        let (r, decoded) = decode_request(&expected).unwrap();
        assert!(r == route() && same(&request, &decoded));
    }
    #[test]
    fn exact_optional_positions_and_field_changes() {
        let (mut request, _) = mixed();
        request.prev_log_id = Some(position(2));
        request.leader_commit = Some(position(3));
        let baseline = encode_request(route(), &request).unwrap();
        assert!(same(&request, &decode_request(&baseline).unwrap().1));
        for change in 0..5 {
            let mut changed = request.clone();
            match change {
                0 => changed.vote.leader_id.term += 1,
                1 => changed.vote.leader_id.node_id += 1,
                2 => changed.prev_log_id = Some(position(1)),
                3 => changed.leader_commit = None,
                4 => changed.entries[0].log_id = position(8),
                _ => unreachable!(),
            };
            let bytes = encode_request(route(), &changed).unwrap();
            assert!(bytes != baseline && same(&changed, &decode_request(&bytes).unwrap().1));
        }
        let mut changed = request;
        changed.entries[0].payload = EntryPayload::Blank;
        changed.entries[1].payload = EntryPayload::Blank;
        assert!(encode_request(route(), &changed).unwrap() != baseline);
    }
    #[test]
    fn request_count_bound_and_header_allocation_caps() {
        let mut request = heartbeat();
        request.entries = (0..64)
            .map(|n| Entry {
                log_id: position(n),
                payload: EntryPayload::Blank,
            })
            .collect();
        assert!(same(
            &request,
            &decode_request(&encode_request(route(), &request).unwrap())
                .unwrap()
                .1
        ));
        request.entries.push(Entry {
            log_id: position(64),
            payload: EntryPayload::Blank,
        });
        assert!(encode_request(route(), &request).is_err());
        let mut payload = prefix();
        payload.push(65);
        for n in 0..65 {
            payload.extend(25u16.to_be_bytes());
            payload.extend(expected_position(n));
            payload.push(0);
        }
        assert!(decode_request(&expected_frame(1, &payload)).is_err());
        for (kind, max) in [(1, MAX_REQUEST), (2, MAX_REPLY)] {
            let mut header = expected_frame(kind, &[]);
            header[8..12].copy_from_slice(&(max as u32).to_be_bytes());
            assert!(inspect_header(&header).is_ok());
            header[8..12].copy_from_slice(&((max + 1) as u32).to_be_bytes());
            assert!(inspect_header(&header).is_err());
            header[8..12].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(inspect_header(&header).is_err());
        }
    }
    #[test]
    fn malformed_headers_lengths_options_and_truncations() {
        let (_, baseline) = mixed();
        for end in 0..baseline.len() {
            assert!(decode_request(&baseline[..end]).is_err());
        }
        let mut extra = baseline.clone();
        extra.push(0);
        assert!(decode_request(&extra).is_err());
        for (offset, value) in [
            (0, b'Z'),
            (4, 0),
            (4, 2),
            (5, 0),
            (5, 3),
            (5, 255),
            (6, 1),
            (7, 1),
        ] {
            let mut altered = baseline.clone();
            altered[offset] = value;
            assert!(decode_request(&altered).is_err());
        }
        for span in [28..36, 36..44] {
            let mut altered = baseline.clone();
            altered[span].fill(0);
            assert!(decode_request(&altered).is_err());
        }
        let mut payload = prefix();
        payload.push(0);
        let mut inner_trailing = payload.clone();
        inner_trailing.push(0);
        assert!(decode_request(&expected_frame(1, &inner_trailing)).is_err());
        for offset in [16, 17, 18] {
            let mut altered = payload.clone();
            altered[offset] = 2;
            assert!(decode_request(&expected_frame(1, &altered)).is_err());
        }
        let mut altered = baseline;
        altered[64..66].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(decode_request(&altered).is_err());
        assert!(decode_reply(&encode_request(route(), &heartbeat()).unwrap()).is_err());
        assert!(decode_request(&encode_reply(route(), &Ok(None)).unwrap()).is_err());
    }
    #[test]
    fn entry_payload_canonical_bounds_and_legacy_tag_refusal() {
        let (mut request, _) = mixed();
        let oversized = Members::new::<BTreeMap<_, _>>(
            vec![[1].into()],
            [(1, BasicNode::new("x".repeat(201)))].into(),
        )
        .unwrap();
        request.entries[0].payload = EntryPayload::Membership(oversized);
        assert!(encode_request(route(), &request).is_err());
        for payload in [vec![2], vec![255], vec![0, 0], vec![3, 0x10, 0x01]] {
            let mut row = expected_position(0);
            row.extend(payload);
            let mut bytes = prefix();
            bytes.push(1);
            bytes.extend((row.len() as u16).to_be_bytes());
            bytes.extend(row);
            assert!(decode_request(&expected_frame(1, &bytes)).is_err());
        }
        let mut row = expected_position(0);
        row.extend([3, 0, 1, 0]);
        let mut bytes = prefix();
        bytes.push(1);
        bytes.extend((row.len() as u16).to_be_bytes());
        bytes.extend(row);
        assert!(decode_request(&expected_frame(1, &bytes)).is_err());
    }
    #[test]
    fn native_reply_exact_ack_conflict_and_higher_vote() {
        let cases: Vec<(AppendResult<Arx>, Vec<u8>)> = vec![
            (Ok(None), vec![0, 0]),
            (
                Ok(Some(position(2))),
                [vec![0, 1], expected_position(2)].concat(),
            ),
            (
                Err(AppendError::Conflict {
                    log_id: position(1),
                    hint: None,
                }),
                [vec![1], expected_position(1)].concat(),
            ),
            (
                Err(AppendError::HigherVote(ballot())),
                [vec![2], prefix()[..17].to_vec()].concat(),
            ),
        ];
        for (native, payload) in cases {
            let expected = expected_frame(2, &payload);
            assert!(encode_reply(route(), &native).unwrap() == expected);
            let (r, decoded) = decode_reply(&expected).unwrap();
            assert!(r == route() && native == decoded);
            for end in 0..expected.len() {
                assert!(decode_reply(&expected[..end]).is_err());
            }
            let mut trailing = expected;
            trailing.push(0);
            assert!(decode_reply(&trailing).is_err());
        }
        // Reply decoding needs no request/predecessor and never invents a full acknowledgement.
        let bytes = encode_reply(route(), &Ok(Some(position(2)))).unwrap();
        assert!(decode_reply(&bytes).unwrap().1 == Ok(Some(position(2))));
        for payload in [vec![], vec![0, 2], vec![3], vec![0, 0, 0]] {
            assert!(decode_reply(&expected_frame(2, &payload)).is_err());
        }
    }
    #[test]
    fn durable_index_limits_and_zero_native_fields() {
        let mut request = heartbeat();
        request.vote = vote(StoredVote {
            term: 0,
            node: 0,
            committed: false,
        });
        request.prev_log_id = Some(id(LogPos {
            term: 0,
            leader: 0,
            index: i64::MAX as u64,
        }));
        assert!(same(
            &request,
            &decode_request(&encode_request(route(), &request).unwrap())
                .unwrap()
                .1
        ));
        let invalid = position(i64::MAX as u64 + 1);
        for field in 0..3 {
            let mut altered = heartbeat();
            match field {
                0 => altered.prev_log_id = Some(invalid),
                1 => altered.leader_commit = Some(invalid),
                2 => altered.entries.push(Entry {
                    log_id: invalid,
                    payload: EntryPayload::Blank,
                }),
                _ => unreachable!(),
            };
            assert!(encode_request(route(), &altered).is_err());
        }
        assert!(encode_reply(route(), &Ok(Some(invalid))).is_err());
        assert!(
            encode_reply(
                route(),
                &Err(AppendError::Conflict {
                    log_id: invalid,
                    hint: None
                })
            )
            .is_err()
        );
        let mut payload = vec![0, 1];
        payload.extend(expected_position(i64::MAX as u64 + 1));
        assert!(decode_reply(&expected_frame(2, &payload)).is_err());
        for source in [0, 7] {
            let r = Route {
                source,
                target: 0,
                ..route()
            };
            assert!(encode_request(r, &heartbeat()).is_err());
        }
    }
    #[test]
    fn errors_are_static_without_payload_debug() {
        for (e, text) in [
            (WireError::Malformed, "malformed peer frame"),
            (WireError::Unsupported, "unsupported peer frame"),
            (WireError::Limit, "peer frame limit"),
        ] {
            assert!(e.to_string() == text && e.to_string().is_ascii());
        }
    }
}
