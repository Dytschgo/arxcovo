//! Installation-authenticated process-exit history. No record is an admission
//! exception, a clean-close token or authorization to serve.
#![allow(dead_code)] // The owner-local runtime consumes this lane in step 13d.

use super::{
    consensus::activation::{CheckedClock, RecoveryComplete},
    enrollment::{ClusterId, KeyDigest, NodeId},
    join_receipt::{ExpectedIdentity, Origin, PublicIdentity},
    pki::custody::{
        Error, FounderCustody, HistoryAuth, HistoryCustody, HistoryRoots, LearnerCustody,
    },
    startup_plan::{Mode, Owner, Refusal},
    state_store::LogPos,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    ffi::CStr,
    fs::File,
    io::{Read, Write},
    net::SocketAddr,
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
};

type Result<T> = std::result::Result<T, Refusal>;
type Object = [u64; 2];
type Digest32 = [u8; 32];
type Position = [u64; 3];

fn custody<T>(value: std::result::Result<T, Error>) -> Result<T> {
    value.map_err(|_| Refusal::Identity)
}

fn object(file: &File) -> Result<Object> {
    let m = file.metadata().map_err(|_| Refusal::Layout)?;
    Ok([m.dev(), m.ino()])
}

fn digest(bytes: &[u8]) -> Digest32 {
    Sha256::digest(bytes).into()
}

fn position(
    LogPos {
        term,
        leader,
        index,
    }: LogPos,
) -> Position {
    [term, leader, index]
}

fn full_position([term, leader, index]: Position) -> LogPos {
    LogPos {
        term,
        leader,
        index,
    }
}

pub(super) fn covers(actual: LogPos, earlier: LogPos) -> bool {
    if actual.index == earlier.index {
        actual == earlier
    } else {
        // LogPos orders (term, leader, index), like the configured
        // OpenRaft advanced LogId.
        actual.index > earlier.index && actual > earlier
    }
}

struct Record {
    kind: u8,
    temp: &'static CStr,
    final_name: &'static CStr,
    limit: usize,
}

const ISSUANCE: Record = Record {
    kind: 1,
    temp: c".issuance.tmp",
    final_name: c"issuance",
    limit: 8192,
};
const STRICT: Record = Record {
    kind: 2,
    temp: c".strict.tmp",
    final_name: c"strict",
    limit: 2048,
};
const VOTER: Record = Record {
    kind: 3,
    temp: c".voter.tmp",
    final_name: c"voter",
    limit: 4096,
};

const MAGIC: &[u8; 6] = b"ARXHST";
const HEADER: usize = 12;
const TAG: usize = 32;

// Only this module constructs MAC requests. Readers cannot submit arbitrary
// bytes to attest, and the record domain is fixed by the typed operation.
pub(super) struct Attest<'a>(&'a Record, &'a [u8]);
pub(super) struct Verify<'a>(&'a Record, &'a [u8], &'a [u8]);

impl<'a> Attest<'a> {
    pub(super) fn parts(self) -> (u8, &'a [u8]) {
        (self.0.kind, self.1)
    }
}

impl<'a> Verify<'a> {
    pub(super) fn parts(self) -> (u8, &'a [u8], &'a [u8]) {
        (self.0.kind, self.1, self.2)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    object: Object,
    payload: T,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Issuance {
    installation: [u8; 16],
    cluster: ClusterId,
    node: u64,
    key: Digest32,
    root: Digest32,
    leaf: Digest32,
    origin: Origin,
    peer: String,
    enrollment: String,
    configuration: Digest32,
    directory: Object,
    owner_objects: Vec<Object>,
    custody_objects: Vec<Object>,
    redeemer: Option<(u64, Digest32)>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Strict {
    issuance: Digest32,
    applied: Position,
    stores: [Object; 4],
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VoterOffer {
    cluster: ClusterId,
    root: Digest32,
    sender: (u64, Digest32),
    receiver: (u64, Digest32),
    sender_address: SocketAddr,
    receiver_address: SocketAddr,
    epoch: [u64; 2],
    membership: Position,
}

impl VoterOffer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn from_guarded_offer(
        cluster: ClusterId,
        root: Digest32,
        sender: (NodeId, Digest32),
        receiver: (NodeId, Digest32),
        sender_address: SocketAddr,
        receiver_address: SocketAddr,
        epoch: (u64, u64),
        membership: LogPos,
    ) -> Self {
        Self {
            cluster,
            root,
            sender: (sender.0.get(), sender.1),
            receiver: (receiver.0.get(), receiver.1),
            sender_address,
            receiver_address,
            epoch: [epoch.0, epoch.1],
            membership: position(membership),
        }
    }

    pub(super) fn cluster(&self) -> ClusterId {
        self.cluster
    }
    pub(super) fn root(&self) -> Digest32 {
        self.root
    }

    pub(super) fn membership(&self) -> LogPos {
        full_position(self.membership)
    }

    fn matches(&self, issuance: &Issuance) -> bool {
        let original = issuance.origin.position();
        let membership = self.membership();
        self.cluster == issuance.cluster
            && self.root == issuance.root
            && self.receiver == (issuance.node, issuance.key)
            && self.receiver_address.to_string() == issuance.peer
            && self.sender.0 != self.receiver.0
            && self.sender.0 != 0
            && self.epoch[1] == self.sender.0
            && self.epoch[0] >= membership.term
            && membership.index > original.index
            && membership.term >= original.term
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Voter {
    issuance: Digest32,
    strict: Digest32,
    offer: VoterOffer,
    objects: [Object; 3],
    ciphertext: Digest32,
}

struct Confirmed<T> {
    record: T,
    digest: Digest32,
    pin: File,
}

pub(super) struct Slot(PathBuf);

impl Slot {
    pub(super) fn configured(path: PathBuf) -> Self {
        Self(path)
    }

    pub(super) fn check_fresh(self, owner: &Owner, wrap: &Path, store: &Path) -> Result<Fresh> {
        if owner.plan.mode.recovery() {
            return Err(Refusal::Provenance);
        }
        owner.recheck()?;
        let roots = custody(HistoryRoots::check(owner, &self.0, wrap, store))?;
        inventory(&roots, &[])?;
        custody(roots.voter_absent())?;
        Ok(Fresh {
            roots,
            owner_objects: owner.history_base_objects()?,
            configuration: owner.history_config_digest()?,
            originals: None,
            spent: false,
            #[cfg(test)]
            fault: None,
        })
    }
}

pub(super) struct Fresh {
    roots: HistoryRoots,
    owner_objects: [Object; 5],
    configuration: Digest32,
    originals: Option<Vec<Object>>,
    spent: bool,
    #[cfg(test)]
    fault: Option<TestFault>,
}

#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum TestFault {
    FileSync,
    DirectorySync,
    VoterSubstitution,
}

impl Fresh {
    #[cfg(test)]
    pub(super) fn fault(&mut self, fault: TestFault) {
        self.fault = Some(fault);
    }

    pub(super) fn bind(&mut self, auth: HistoryAuth<'_>) -> Result<()> {
        if self.originals.is_some() {
            return Err(Refusal::Provenance);
        }
        inventory(&self.roots, &[])?;
        self.originals = Some(custody(auth.objects(&self.roots))?);
        Ok(())
    }

    fn complete(
        &mut self,
        owner: &Owner,
        auth: HistoryAuth<'_>,
        expected: &ExpectedIdentity,
        leaf: &[u8],
        redeemer: Option<(NodeId, KeyDigest)>,
    ) -> Result<Live> {
        if std::mem::replace(&mut self.spent, true)
            || expected.origin.founder() != owner.plan.mode.founder()
        {
            return Err(Refusal::Provenance);
        }
        if self.owner_objects != owner.history_base_objects()?
            || self.configuration != owner.history_config_digest()?
        {
            return Err(Refusal::Ownership);
        }
        let custody_objects = custody(auth.objects(&self.roots))?;
        if Some(&custody_objects) != self.originals.as_ref() {
            return Err(Refusal::Ownership);
        }
        let founder = expected.origin.founder();
        let root = owner.read("ca.der", 4096)?;
        if digest(&root) != expected.ca {
            return Err(Refusal::Identity);
        }
        let record = Issuance {
            installation: auth.installation(),
            cluster: expected.cluster,
            node: expected.node.get(),
            key: *expected.key.as_bytes(),
            root: expected.ca,
            leaf: digest(leaf),
            origin: expected.origin.clone(),
            peer: owner.plan.peer.clone(),
            enrollment: owner.plan.enrollment.clone(),
            configuration: self.configuration,
            directory: object(self.roots.directory())?,
            owner_objects: owner.history_objects(founder)?,
            custody_objects,
            redeemer: redeemer.map(|(node, key)| (node.get(), *key.as_bytes())),
        };
        let issuance = write_record(
            &self.roots,
            &auth,
            &ISSUANCE,
            &[],
            record,
            #[cfg(test)]
            self.fault,
        )?;
        Ok(Live {
            issuance,
            strict: None,
            strict_spent: false,
        })
    }

    pub(super) fn founder(
        &mut self,
        owner: &Owner,
        custody: &FounderCustody,
        proof: &<FounderCustody as FounderProof>::Publication,
    ) -> Result<Live> {
        self.complete(
            owner,
            custody.history_auth(),
            proof.expected(),
            proof.leaf(),
            None,
        )
    }

    pub(super) fn learner(
        &mut self,
        owner: &Owner,
        custody: &LearnerCustody,
        proof: &super::enrollment::durable_join::JoinPublication<'_>,
    ) -> Result<Live> {
        self.complete(
            owner,
            custody.history_auth(),
            proof.expected(),
            proof.leaf(),
            Some(proof.peer()),
        )
    }

    pub(super) fn persist_strict(
        &self,
        owner: &Owner,
        custody: &LearnerCustody,
        live: &mut Live,
        witness: <LearnerCustody as StrictProof>::Observation,
    ) -> Result<()> {
        self.persist_strict_position(owner, custody, live, witness.position())
    }

    fn persist_strict_position(
        &self,
        owner: &Owner,
        custody: &LearnerCustody,
        live: &mut Live,
        applied: LogPos,
    ) -> Result<()> {
        if std::mem::replace(&mut live.strict_spent, true) {
            return Err(Refusal::Provenance);
        }
        let auth = custody.history_auth();
        self.check_issuance(owner, &auth, live)?;
        let issuance = &live.issuance;
        if !covers(applied, issuance.record.origin.position()) {
            return Err(Refusal::Provenance);
        }
        let strict = Strict {
            issuance: issuance.digest,
            applied: position(applied),
            stores: store_objects(owner)?,
        };
        live.strict = Some(write_record(
            &self.roots,
            &auth,
            &STRICT,
            &["issuance"],
            strict,
            #[cfg(test)]
            self.fault,
        )?);
        Ok(())
    }

    fn check_issuance(&self, owner: &Owner, auth: &HistoryAuth<'_>, live: &Live) -> Result<()> {
        validate_issuance(&self.roots, owner, auth, &live.issuance)?;
        let current = read_record::<Issuance>(&self.roots, auth, &ISSUANCE)?;
        if current.digest != live.issuance.digest
            || object(&current.pin)? != object(&live.issuance.pin)?
        {
            return Err(Refusal::Ownership);
        }
        Ok(())
    }

    pub(super) fn confirm_strict(
        &self,
        owner: &Owner,
        auth: &HistoryAuth<'_>,
        live: &Live,
    ) -> Result<()> {
        self.check_issuance(owner, auth, live)?;
        let strict = live.strict.as_ref().ok_or(Refusal::Provenance)?;
        let current = read_record::<Strict>(&self.roots, auth, &STRICT)?;
        if current.digest != strict.digest
            || object(&current.pin)? != object(&strict.pin)?
            || store_objects(owner)? != strict.record.stores
        {
            return Err(Refusal::Ownership);
        }
        Ok(())
    }

    pub(super) fn commit_voter(
        &self,
        owner: &Owner,
        auth: HistoryAuth<'_>,
        live: &Live,
        candidate: &super::pki::custody::voter_transfer::HistoryCandidate,
    ) -> Result<DurableVoter> {
        self.confirm_strict(owner, &auth, live)?;
        let strict = live.strict.as_ref().ok_or(Refusal::Provenance)?;
        #[cfg(test)]
        if self.fault == Some(TestFault::VoterSubstitution) {
            let path = owner.plan.config.parent().ok_or(Refusal::Layout)?;
            let envelope = path.join("voter-custody/ca-key");
            let displaced = path.join("displaced-voter-envelope");
            std::fs::rename(&envelope, &displaced).unwrap();
            std::fs::copy(&displaced, &envelope).unwrap();
        }
        let binding = custody(candidate.recheck())?;
        if binding.links != live.links()? || !binding.offer.matches(&live.issuance.record) {
            return Err(Refusal::Provenance);
        }
        let voter = Voter {
            issuance: live.issuance.digest,
            strict: strict.digest,
            offer: binding.offer.clone(),
            objects: binding.objects,
            ciphertext: binding.ciphertext,
        };
        write_record(
            &self.roots,
            &auth,
            &VOTER,
            &["issuance", "strict"],
            voter,
            #[cfg(test)]
            self.fault,
        )?;
        Ok(DurableVoter {
            objects: binding.objects,
            ciphertext: binding.ciphertext,
        })
    }
}

// The founder and learner keep their proof constructors private to the job.
// These projections pin each writer to the exact role's opaque proof type
// without exposing the enclosing bound-start modules.
pub(super) trait FounderProof {
    type Publication: PublishedFounder;
}

pub(super) trait PublishedFounder {
    fn expected(&self) -> &ExpectedIdentity;
    fn leaf(&self) -> &[u8];
}

pub(super) trait StrictProof {
    type Observation: StrictObservation;
}

pub(super) trait StrictObservation {
    fn position(self) -> LogPos;
}

pub(super) struct Live {
    issuance: Confirmed<Issuance>,
    strict: Option<Confirmed<Strict>>,
    strict_spent: bool,
}

impl Live {
    pub(super) fn links(&self) -> Result<[Digest32; 2]> {
        Ok([
            self.issuance.digest,
            self.strict.as_ref().ok_or(Refusal::Provenance)?.digest,
        ])
    }
}

impl Fresh {
    pub(super) fn confirm_learner_without_ca(&self) -> Result<()> {
        custody(self.roots.recheck())?;
        inventory(&self.roots, &["issuance"])?;
        custody(self.roots.voter_absent())
    }
}

pub(super) struct DurableVoter {
    objects: [Object; 3],
    ciphertext: Digest32,
}

impl DurableVoter {
    pub(super) fn matches(&self, objects: [Object; 3], ciphertext: Digest32) -> bool {
        self.objects == objects && self.ciphertext == ciphertext
    }
}

pub(super) struct VoterReopen<'a>(&'a Voter);

impl VoterReopen<'_> {
    pub(super) fn offer(&self) -> &VoterOffer {
        &self.0.offer
    }

    pub(super) fn matches(&self, objects: [Object; 3], ciphertext: Digest32) -> bool {
        self.0.objects == objects && self.0.ciphertext == ciphertext
    }
}

pub(super) struct Recovering {
    roots: HistoryRoots,
    custody: Option<HistoryCustody>,
    verified: Option<Verified>,
}

impl Recovering {
    pub(super) fn open(owner: &Owner, slot: Slot, wrap: &Path, store: &Path) -> Result<Self> {
        if !owner.plan.mode.recovery() {
            return Err(Refusal::Provenance);
        }
        owner.inspect_validate()?;
        let roots = custody(HistoryRoots::check(owner, &slot.0, wrap, store))?;
        Ok(Self {
            roots,
            custody: None,
            verified: None,
        })
    }

    pub(super) fn authenticate(&mut self, owner: &Owner) -> Result<()> {
        let founder = owner.plan.mode == Mode::RecoverFounder;
        if self.custody.is_some() {
            return Err(Refusal::Ownership);
        }
        self.custody = Some(custody(HistoryCustody::open(&mut self.roots, founder))?);
        let installation = self.custody.as_mut().unwrap();
        let auth = installation.auth();
        let names = custody(self.roots.entries())?;
        let voter_present = names == ["issuance", "strict", "voter"];
        if !(founder && names == ["issuance"]
            || !founder && (names == ["issuance", "strict"] || voter_present))
        {
            return Err(Refusal::Provenance);
        }
        if !voter_present {
            custody(self.roots.voter_absent())?;
        }
        let issuance = read_record::<Issuance>(&self.roots, &auth, &ISSUANCE)?;
        let expected = validate_issuance(&self.roots, owner, &auth, &issuance)?;
        let strict = if founder {
            None
        } else {
            let strict = read_record::<Strict>(&self.roots, &auth, &STRICT)?;
            if strict.record.issuance != issuance.digest
                || strict.record.stores != store_objects(owner)?
                || !covers(
                    full_position(strict.record.applied),
                    expected.origin.position(),
                )
            {
                return Err(Refusal::Provenance);
            }
            Some(strict)
        };
        let voter = if voter_present {
            let strict = strict.as_ref().ok_or(Refusal::Provenance)?;
            let voter = read_record::<Voter>(&self.roots, &auth, &VOTER)?;
            if voter.record.strict != strict.digest
                || voter.record.issuance != issuance.digest
                || !voter.record.offer.matches(&issuance.record)
            {
                return Err(Refusal::Provenance);
            }
            Some(voter)
        } else {
            None
        };
        custody(installation.reopen_member())?;
        self.verified = Some(Verified {
            expected,
            issuance,
            strict,
            voter,
        });
        Ok(())
    }

    pub(super) fn verified(&self) -> Result<&Verified> {
        self.verified.as_ref().ok_or(Refusal::Provenance)
    }

    pub(super) fn validate_credentials(&mut self, owner: &Owner, now: u64) -> Result<()> {
        let evidence = self.verified.as_ref().ok_or(Refusal::Provenance)?;
        let installation = self.custody.as_mut().ok_or(Refusal::Provenance)?;
        let auth = installation.auth();
        validate_issuance(&self.roots, owner, &auth, &evidence.issuance)?;
        let mut names = vec!["issuance"];
        same_record(&self.roots, &auth, &ISSUANCE, &evidence.issuance)?;
        if let Some(strict) = &evidence.strict {
            names.push("strict");
            same_record(&self.roots, &auth, &STRICT, strict)?;
            if store_objects(owner)? != strict.record.stores {
                return Err(Refusal::Ownership);
            }
        }
        if let Some(voter) = &evidence.voter {
            names.push("voter");
            same_record(&self.roots, &auth, &VOTER, voter)?;
        } else {
            custody(self.roots.voter_absent())?;
        }
        inventory(&self.roots, &names)?;
        let member = custody(installation.member())?;
        super::issuance_publication::check_retained(
            owner,
            Some(&evidence.expected),
            auth.installation(),
            &member,
            Some(now),
        )
        .map_err(|_| Refusal::Identity)?;
        if let HistoryCustody::Learner(custody_owner) = installation {
            let root = owner.read("ca.der", 4096)?;
            let voter = evidence.voter.as_ref().map(|v| VoterReopen(&v.record));
            custody(custody_owner.reopen_voter_history(owner, &root, now, voter))?;
        }
        Ok(())
    }

    pub(super) fn complete(
        self,
        _complete: RecoveryComplete,
        clock: Arc<dyn CheckedClock>,
    ) -> RecoveredHistory {
        RecoveredHistory {
            roots: self.roots,
            custody: self.custody.expect("original custody retained"),
            evidence: self.verified.expect("authenticated evidence retained"),
            clock,
        }
    }
}

/// Move-only original custody certified by the typed recovery reader. A
/// successful transition does not acquire a second custody lock.
pub(super) struct RecoveredHistory {
    roots: HistoryRoots,
    custody: HistoryCustody,
    evidence: Verified,
    clock: Arc<dyn CheckedClock>,
}

impl std::fmt::Debug for RecoveredHistory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveredHistory")
    }
}

impl RecoveredHistory {
    pub(super) fn verified(&self) -> Result<&Verified> {
        Ok(&self.evidence)
    }

    pub(super) fn founder(&self) -> Option<&FounderCustody> {
        match &self.custody {
            HistoryCustody::Founder(custody) => Some(custody),
            HistoryCustody::Learner(_) => None,
        }
    }

    pub(super) fn into_learner(self) -> std::result::Result<(LearnerCustody, Verified), Box<Self>> {
        if !matches!(self.custody, HistoryCustody::Learner(_)) {
            return Err(Box::new(self));
        }
        let HistoryCustody::Learner(custody) = self.custody else {
            unreachable!("learner role checked")
        };
        Ok((*custody, self.evidence))
    }

    pub(super) fn founder_context(
        &self,
        installed: &FounderCustody,
        owner: &Owner,
    ) -> std::result::Result<(ClusterId, Digest32, u64), Error> {
        let original = self.founder().ok_or(Error::Context)?;
        if owner.plan.mode != Mode::RecoverFounder || !std::ptr::eq(original, installed) {
            return Err(Error::Ownership);
        }
        let auth = installed.history_auth();
        let evidence = &self.evidence.issuance;
        same_record(&self.roots, &auth, &ISSUANCE, evidence).map_err(|_| Error::Ownership)?;
        let expected =
            validate_issuance(&self.roots, owner, &auth, evidence).map_err(|_| Error::Ownership)?;
        let now = self.clock.sample_ms().map_err(|_| Error::Clock)?;
        Ok((expected.cluster, expected.ca, now))
    }
}

pub(super) struct Verified {
    pub(super) expected: ExpectedIdentity,
    issuance: Confirmed<Issuance>,
    strict: Option<Confirmed<Strict>>,
    voter: Option<Confirmed<Voter>>,
}

impl Verified {
    pub(super) fn strict_position(&self) -> Option<LogPos> {
        self.strict
            .as_ref()
            .map(|strict| full_position(strict.record.applied))
    }

    pub(super) fn voter_position(&self) -> Option<LogPos> {
        self.voter
            .as_ref()
            .map(|voter| voter.record.offer.membership())
    }
}

fn validate_issuance(
    roots: &HistoryRoots,
    owner: &Owner,
    auth: &HistoryAuth<'_>,
    evidence: &Confirmed<Issuance>,
) -> Result<ExpectedIdentity> {
    let record = &evidence.record;
    let founder = owner.plan.mode.founder();
    if record.installation != auth.installation()
        || record.directory != object(roots.directory())?
        || record.origin.founder() != founder
        || record.peer != owner.plan.peer
        || record.enrollment != owner.plan.enrollment
        || record.configuration != owner.history_config_digest()?
        || record.owner_objects != owner.history_objects(founder)?
        || record.custody_objects != custody(auth.objects(roots))?
    {
        return Err(Refusal::Ownership);
    }
    let root = owner.read("ca.der", 4096)?;
    let public = PublicIdentity::decode(&owner.read("identity.json", 8192)?)
        .map_err(|_| Refusal::Identity)?;
    let leaf = public.leaf();
    let (remaining, parsed) =
        x509_parser::parse_x509_certificate(leaf).map_err(|_| Refusal::Identity)?;
    let key = KeyDigest::of_spki(parsed.public_key().raw).map_err(|_| Refusal::Identity)?;
    let node = NodeId::new(record.node).ok_or(Refusal::Identity)?;
    let original = record.origin.position();
    if !remaining.is_empty()
        || public.artifact() != "member-key"
        || record.root != digest(&root)
        || record.leaf != digest(leaf)
        || record.key != *key.as_bytes()
        || original.index > i64::MAX as u64
        || original.term == 0
        || original.index == 0
    {
        return Err(Refusal::Identity);
    }
    match &record.origin {
        Origin::Found { leader: 1, .. } if record.node == 1 && record.redeemer.is_none() => (),
        Origin::Redeem {
            anchors, leader, ..
        } if record.node >= 2
            && (1..=16).contains(&anchors.len())
            && anchors.windows(2).all(|w| w[0].0 < w[1].0)
            && anchors.iter().all(|(id, _)| *id != 0 && *id != record.node)
            && anchors.iter().any(|(id, _)| id == leader)
            && record.redeemer.is_some_and(|p| anchors.contains(&p)) => {}
        _ => return Err(Refusal::Identity),
    }
    Ok(ExpectedIdentity {
        cluster: record.cluster,
        node,
        key,
        ca: record.root,
        origin: record.origin.clone(),
    })
}

fn store_objects(owner: &Owner) -> Result<[Object; 4]> {
    owner.history_objects(true)?[8..]
        .try_into()
        .map_err(|_| Refusal::Ownership)
}

fn inventory(roots: &HistoryRoots, expected: &[&str]) -> Result<()> {
    if custody(roots.entries())? != expected {
        return Err(Refusal::Provenance);
    }
    Ok(())
}

fn same_record<T: Serialize + DeserializeOwned>(
    roots: &HistoryRoots,
    auth: &HistoryAuth<'_>,
    spec: &Record,
    original: &Confirmed<T>,
) -> Result<()> {
    let current = read_record::<T>(roots, auth, spec)?;
    if current.digest != original.digest || object(&current.pin)? != object(&original.pin)? {
        return Err(Refusal::Ownership);
    }
    Ok(())
}

fn read_record<T: Serialize + DeserializeOwned>(
    roots: &HistoryRoots,
    auth: &HistoryAuth<'_>,
    spec: &Record,
) -> Result<Confirmed<T>> {
    let name = spec.final_name.to_str().map_err(|_| Refusal::Layout)?;
    let pin = custody(roots.file(name, false))?;
    let mut raw = Vec::with_capacity(spec.limit + 1);
    (&pin)
        .take((spec.limit + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|_| Refusal::Unavailable)?;
    if raw.len() < HEADER + TAG
        || raw.len() > spec.limit
        || &raw[..6] != MAGIC
        || raw[6] != 1
        || raw[7] != spec.kind
    {
        return Err(Refusal::Provenance);
    }
    let (framed, tag) = raw.split_at(raw.len() - TAG);
    let size = u32::from_be_bytes(framed[8..12].try_into().unwrap()) as usize;
    if size != framed.len() - HEADER {
        return Err(Refusal::Provenance);
    }
    custody(auth.verify(Verify(spec, framed, tag)))?;
    let body = &framed[HEADER..];
    let envelope: Envelope<T> = serde_json::from_slice(body).map_err(|_| Refusal::Provenance)?;
    if serde_json::to_vec(&envelope).map_err(|_| Refusal::Provenance)? != body
        || envelope.object != object(&pin)?
        || envelope.object != object(&custody(roots.file(name, false))?)?
    {
        return Err(Refusal::Ownership);
    }
    Ok(Confirmed {
        record: envelope.payload,
        digest: digest(&raw),
        pin,
    })
}

fn write_record<T: Serialize + DeserializeOwned>(
    roots: &HistoryRoots,
    auth: &HistoryAuth<'_>,
    spec: &Record,
    existing: &[&str],
    payload: T,
    #[cfg(test)] fault: Option<TestFault>,
) -> Result<Confirmed<T>> {
    inventory(roots, existing)?;
    let file_name = spec.temp.to_str().map_err(|_| Refusal::Layout)?;
    let mut file = custody(roots.file(file_name, true))?;
    let id = object(&file)?;
    let body = serde_json::to_vec(&Envelope {
        object: id,
        payload,
    })
    .map_err(|_| Refusal::Provenance)?;
    if body.len() + HEADER + TAG > spec.limit {
        return Err(Refusal::Provenance);
    }
    let mut raw = Vec::with_capacity(HEADER + body.len() + TAG);
    raw.extend_from_slice(MAGIC);
    raw.extend_from_slice(&[1, spec.kind]);
    raw.extend_from_slice(&(body.len() as u32).to_be_bytes());
    raw.extend_from_slice(&body);
    raw.extend_from_slice(&auth.attest(Attest(spec, &raw)));
    file.write_all(&raw).map_err(|_| Refusal::Unavailable)?;
    #[cfg(test)]
    if fault == Some(TestFault::FileSync) {
        return Err(Refusal::Unavailable);
    }
    file.sync_all().map_err(|_| Refusal::Unavailable)?;
    if object(&custody(roots.file(file_name, false))?)? != id {
        return Err(Refusal::Ownership);
    }
    let dir = roots.directory();
    // SAFETY: both fixed names are NUL-terminated and relative to the retained directory.
    if unsafe {
        libc::renameat2(
            dir.as_raw_fd(),
            spec.temp.as_ptr(),
            dir.as_raw_fd(),
            spec.final_name.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    } != 0
    {
        return Err(Refusal::Unavailable);
    }
    #[cfg(test)]
    if fault == Some(TestFault::DirectorySync) {
        return Err(Refusal::Unavailable);
    }
    dir.sync_all().map_err(|_| Refusal::Unavailable)?;
    let confirmed = read_record(roots, auth, spec)?;
    if object(&confirmed.pin)? != id || confirmed.digest != digest(&raw) {
        return Err(Refusal::Ownership);
    }
    let mut final_inventory = existing.to_vec();
    final_inventory.push(spec.final_name.to_str().map_err(|_| Refusal::Layout)?);
    final_inventory.sort_unstable();
    inventory(roots, &final_inventory)?;
    Ok(confirmed)
}

#[cfg(test)]
pub(super) mod tests;
