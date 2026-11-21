//! Existing-store inspection precedes every create-capable opener. No Engine,
//! credential release, key unseal or runtime handoff is implemented here.
use super::{
    checked_clock::{CheckedClock, HostClock},
    preflight,
};
use crate::cluster::{
    enrollment::{
        ClusterId, NodeId,
        codec::{self, Reader},
    },
    join_receipt::{ExpectedIdentity, PublicIdentity},
    log_store::{MAX_LIVE, VoteLogStore},
    startup_plan::{Mode, Owner, Plan, Refusal},
    state_store::{Entry, Init, LogPos, MembershipBlob, StateStore, StoreError, get_pos, put_pos},
};
use rusqlite::Connection;
#[cfg(test)]
use rusqlite::OpenFlags;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::fs::OpenOptions;
use std::{fs::File, path::Path};
type Result<T> = std::result::Result<T, Refusal>;

pub(super) struct Prepared {
    owner: Owner,
    pub observation: Observation,
    binding: Option<(ClusterId, NodeId)>,
    fresh_cluster: Option<ClusterId>,
    creation: Creation,
}
enum Creation {
    Ready,
    Begun,
    Failed(Refusal),
}
pub(super) struct StartIntent {
    pub(super) mode: Mode,
    pub(super) cluster: ClusterId,
    pub(super) node: NodeId,
    pub(super) peer: std::net::SocketAddr,
    pub(super) enrollment: std::net::SocketAddr,
}
pub(super) struct Retained {
    owner: Owner,
    observation: Observation,
    intent: StartIntent,
}
pub(super) struct StartInputs<'a> {
    pub(super) dir: &'a Path,
    pub(super) node: NodeId,
    pub(super) init: Init,
    pub(super) peer: std::net::SocketAddr,
    pub(super) enrollment: std::net::SocketAddr,
}
impl Prepared {
    // Exact ownership-return contract: refusal returns the same Prepared inline.
    #[allow(clippy::result_large_err)]
    pub(super) fn handoff(
        mut self,
        intent: StartIntent,
    ) -> std::result::Result<Retained, (Refusal, Self)> {
        let refusal = match self.creation {
            Creation::Failed(r) => Some(r),
            Creation::Begun => Some(Refusal::Store),
            Creation::Ready => None,
        };
        if let Some(r) = refusal {
            return Err((r, self));
        }
        if let Err(r) = self.validate_intent(&intent) {
            return Err((r, self));
        }
        if matches!(self.observation, Observation::Fresh) {
            // Retain Prepared outside the caught mutable borrow on ordinary unwind.
            // Abort/process destruction has no retention guarantee.
            self.creation = Creation::Begun;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let result = initialize_fresh(&mut self.owner, intent.cluster);
                if let Err(r) = result {
                    self.creation = Creation::Failed(r);
                }
                #[cfg(test)]
                tests::after_creation_result(&result);
                result
            }));
            let refusal = match outcome {
                Ok(Ok(())) => None,
                Ok(Err(r)) => Some(r),
                Err(_) => Some(match self.creation {
                    Creation::Failed(r) => r,
                    _ => Refusal::Store,
                }),
            };
            if let Some(r) = refusal {
                self.creation = Creation::Failed(r);
                return Err((r, self));
            }
        }
        if matches!(self.creation, Creation::Begun) {
            self.observation = Observation::UntouchedGenesis {
                cluster: intent.cluster,
            };
        }
        Ok(Retained {
            owner: self.owner,
            observation: self.observation,
            intent,
        })
    }
    fn validate_intent(&self, intent: &StartIntent) -> Result<()> {
        self.owner.recheck()?;
        if intent.mode != self.owner.plan.mode
            || Some(intent.peer) != self.owner.plan.peer.parse().ok()
            || Some(intent.enrollment) != self.owner.plan.enrollment.parse().ok()
        {
            return Err(Refusal::Plan);
        }
        if intent.mode.recovery() {
            if !matches!(self.observation, Observation::Recovered { .. })
                || self.binding != Some((intent.cluster, intent.node))
            {
                return Err(Refusal::Identity);
            }
        } else {
            if intent.mode != Mode::CreateFounder || self.fresh_cluster.is_none() {
                return Err(Refusal::Provenance);
            }
            if self.fresh_cluster != Some(intent.cluster) || intent.node.get() != 1 {
                return Err(Refusal::Identity);
            }
            match self.observation {
                Observation::Fresh => self.owner.fresh_baseline()?,
                Observation::UntouchedGenesis { cluster } if cluster == intent.cluster => {}
                _ => return Err(Refusal::Provenance),
            }
        }
        Ok(())
    }
}
impl Retained {
    pub(super) fn recheck(&self) -> Result<()> {
        self.owner.recheck()
    }
    /// Checked paths only, never an actual-open Engine capability.
    pub(super) fn checked_inputs(&self) -> Result<StartInputs<'_>> {
        self.recheck()?;
        Ok(StartInputs {
            dir: &self.owner.plan.state,
            node: self.intent.node,
            init: Init {
                cluster: self.intent.cluster,
                first_free: NodeId::new(2).unwrap(),
            },
            peer: self.intent.peer,
            enrollment: self.intent.enrollment,
        })
    }
}
fn initialize_fresh(owner: &mut Owner, cluster: ClusterId) -> Result<()> {
    owner.fresh_baseline()?;
    let mut log = VoteLogStore::open(&owner.plan.state, cluster).map_err(|_| Refusal::Store)?;
    #[cfg(test)]
    tests::creation_phase("first-open", owner)?;
    let sm = StateStore::open(
        &owner.plan.state,
        Init {
            cluster,
            first_free: NodeId::new(2).unwrap(),
        },
    )
    .map_err(|_| Refusal::Store)?;
    let untouched =
        crate::cluster::enrollment::EnrollmentState::new(cluster, NodeId::new(2).unwrap());
    let state = log.log_state().map_err(|_| Refusal::Store)?;
    if log.vote().map_err(|_| Refusal::Store)?.is_some()
        || state.last.is_some()
        || state.purged.is_some()
        || log.live().map_err(|_| Refusal::Store)? != 0
        || sm.applied().is_some()
        || sm.membership().is_some()
        || *sm.state() != untouched
    {
        return Err(Refusal::Provenance);
    }
    preflight(&mut log, &sm).map_err(|_| Refusal::Store)?;
    #[cfg(test)]
    tests::creation_phase("before-capture", owner)?;
    owner.capture_created_stores()?;
    #[cfg(test)]
    tests::creation_phase("captured", owner)?;
    drop(sm);
    drop(log);
    #[cfg(test)]
    tests::creation_phase("dropped", owner)?;
    let (inspected, log) = inspect_pair(owner, cluster)?;
    validate_preflight(inspected.applied, &log).map_err(|_| Refusal::Store)?;
    if inspected.applied.is_some()
        || inspected.membership.is_some()
        || log.purged.is_some()
        || log.last.is_some()
        || log.has_vote
        || snapshot_state(&inspected.snapshot).map_err(|_| Refusal::Store)? != untouched
    {
        return Err(Refusal::Provenance);
    }
    #[cfg(test)]
    tests::creation_phase("inspected", owner)?;
    owner.recheck()
}
pub(super) enum Observation {
    Fresh,
    UntouchedGenesis {
        cluster: ClusterId,
    },
    Recovered {
        floor: u64,
        sample: u64,
        applied: Option<LogPos>,
        membership: Option<(LogPos, MembershipBlob)>,
        clock: HostClock,
    },
}
/// `expected` and root authentication come from a separately trusted caller.
/// The root's fingerprint is in expected, never trusted from the record alone.
pub(super) fn prepare(
    plan: Plan,
    expected: Option<&ExpectedIdentity>,
    fresh_cluster: Option<ClusterId>,
    require_historical_genesis: bool,
) -> Result<Prepared> {
    if plan.mode.recovery() && require_historical_genesis {
        return Err(Refusal::Provenance);
    }
    if !plan.mode.recovery() && expected.is_some() {
        return Err(Refusal::Identity);
    }
    let mut owner = Owner::acquire(plan)?;
    if !owner.plan.mode.recovery() {
        if owner.has_stores() {
            let cluster = fresh_cluster.ok_or(Refusal::Provenance)?;
            let (inspected, log) = inspect_pair(&mut owner, cluster)?;
            let StateInspection {
                snapshot,
                applied,
                membership,
            } = inspected;

            let state = snapshot_state(&snapshot).map_err(|_| Refusal::Store)?;
            let untouched = crate::cluster::enrollment::EnrollmentState::new(
                cluster,
                std::num::NonZeroU64::new(2).unwrap(),
            );
            if applied.is_some()
                || membership.is_some()
                || log.last.is_some()
                || log.has_vote
                || state != untouched
            {
                return Err(Refusal::Provenance);
            }
            owner.recheck()?;
            return Ok(Prepared {
                owner,
                observation: Observation::UntouchedGenesis { cluster },
                binding: None,
                fresh_cluster,
                creation: Creation::Ready,
            });
        }
        return Ok(Prepared {
            owner,
            observation: Observation::Fresh,
            binding: None,
            fresh_cluster,
            creation: Creation::Ready,
        });
    }
    let expected = expected.ok_or(Refusal::Identity)?;
    authenticate(&owner, expected)?;
    // Existing SQL locks are held throughout immutable inspection. The outer
    // owner stays held across their release and the current-store openers.
    #[cfg(test)]
    tests::phase("before-inspection", &owner);
    let (state, log) = inspect_pair(&mut owner, expected.cluster)?;
    let StateInspection {
        snapshot,
        applied,
        membership,
    } = state;
    validate_preflight(applied, &log).map_err(|_| Refusal::Store)?;
    if applied.is_none_or(|p| {
        p.index < expected.origin.position().index || p < expected.origin.position()
    }) {
        return Err(Refusal::Identity);
    }
    let inspected = snapshot_state(&snapshot).map_err(|_| Refusal::Store)?;
    inspected
        .authorize_peer(expected.cluster, expected.node, expected.key)
        .map_err(|_| Refusal::Identity)?;
    HostClock::new(inspected.committed_ms())
        .sample_ms()
        .map_err(|_| Refusal::Clock)?;
    #[cfg(test)]
    tests::phase("before-open", &owner);
    owner.recheck()?;
    #[cfg(test)]
    tests::mark_mutable_open();
    let sm = StateStore::open(
        &owner.plan.state,
        Init {
            cluster: expected.cluster,
            first_free: std::num::NonZeroU64::new(2).unwrap(),
        },
    )
    .map_err(|_| Refusal::Store)?;
    if sm.snapshot() != snapshot || sm.membership() != membership.as_ref() {
        return Err(Refusal::Store);
    }
    let mut log =
        VoteLogStore::open(&owner.plan.state, expected.cluster).map_err(|_| Refusal::Store)?;
    preflight(&mut log, &sm).map_err(|_| Refusal::Store)?;
    sm.state()
        .authorize_peer(expected.cluster, expected.node, expected.key)
        .map_err(|_| Refusal::Identity)?;
    let floor = sm.state().committed_ms();
    let clock = HostClock::new(floor);
    let sample = clock.sample_ms().map_err(|_| Refusal::Clock)?;
    drop(log);
    drop(sm);
    #[cfg(test)]
    tests::phase("before-publication", &owner);
    owner.recheck()?;
    Ok(Prepared {
        owner,
        binding: Some((expected.cluster, expected.node)),
        fresh_cluster,
        creation: Creation::Ready,
        observation: Observation::Recovered {
            floor,
            sample,
            applied,
            membership,
            clock,
        },
    })
}
fn inspect_pair(owner: &mut Owner, cluster: ClusterId) -> Result<(StateInspection, LogInspection)> {
    use crate::cluster::bound_open::{BoundFailure, BoundIntent, BoundStores};
    let mut bound = BoundStores::acquire(owner, BoundIntent::Inspect(cluster));
    bound
        .inspect(|state, state_file, log, log_file| {
            inspect_connections(state, state_file, log, log_file, cluster)
                .map_err(|_| StoreError::Corrupt)
        })
        .map_err(|f| match f {
            BoundFailure::Inspection(refusal) => refusal,
            BoundFailure::Layout => Refusal::Layout,
            _ => Refusal::Store,
        })
}
fn authenticate(owner: &Owner, expected: &ExpectedIdentity) -> Result<()> {
    if owner.plan.mode.founder() != expected.origin.founder() {
        return Err(Refusal::Identity);
    }
    let record = PublicIdentity::decode(&owner.read("identity.json", 8192)?)
        .map_err(|_| Refusal::Identity)?;
    let root = owner.read("ca.der", 4096)?;
    if owner.read(record.artifact(), 65536)?.is_empty() {
        return Err(Refusal::Identity);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Refusal::Clock)?;
    record
        .verify(
            expected,
            &root,
            rustls::pki_types::UnixTime::since_unix_epoch(now),
        )
        .map_err(|_| Refusal::Identity)?;
    owner.recheck()?;
    Ok(())
}
fn inspect_connections(
    state: &Connection,
    state_file: &File,
    log: &Connection,
    log_file: &File,
    cluster: ClusterId,
) -> Result<(StateInspection, LogInspection)> {
    state_schema(state, state_file)?;
    log_schema(log, log_file)?;
    integrity(state)?;
    integrity(log)?;
    let state = inspect_state_rows(state, cluster)?;
    let log = inspect_log_rows(log, cluster)?;
    validate_preflight(state.applied, &log).map_err(|_| Refusal::Store)?;
    Ok((state, log))
}
/// Closed operational preparation; never calls the terminal offline inspector.
pub(super) fn prepare_bound(
    owner: &mut Owner,
    intent: &StartIntent,
    expected: Option<&ExpectedIdentity>,
) -> (crate::cluster::bound_open::OwnedStores, Option<Observation>) {
    use crate::cluster::bound_open::{BoundFailure, BoundIntent, BoundStores};
    let recovery = owner.plan.mode.recovery();
    let existing = owner.has_stores();
    let cluster = intent.cluster;
    let init = Init {
        cluster,
        first_free: NodeId::new(2).unwrap(),
    };
    let bound_intent = if recovery {
        BoundIntent::Recovery(cluster)
    } else if existing {
        BoundIntent::UntouchedGenesis(init)
    } else {
        BoundIntent::Fresh(init)
    };
    let validate_owner = |owner: &Owner| {
        owner.recheck()?;
        if intent.mode != owner.plan.mode
            || Some(intent.peer) != owner.plan.peer.parse().ok()
            || Some(intent.enrollment) != owner.plan.enrollment.parse().ok()
        {
            return Err(Refusal::Plan);
        }
        if recovery {
            let e = expected.ok_or(Refusal::Identity)?;
            if e.cluster != cluster || e.node != intent.node {
                return Err(Refusal::Identity);
            }
            authenticate(owner, e)?;
        } else if intent.mode != Mode::CreateFounder || intent.node.get() != 1 || expected.is_some()
        {
            return Err(Refusal::Identity);
        }
        Ok(())
    };
    let sample = std::cell::Cell::new(None);
    let (stores, inspected) = BoundStores::operational(
        owner,
        bound_intent,
        |stores| {
            stores.validate_owner_for_start(validate_owner)?;
            if !existing {
                return Ok(None);
            }
            let pair = stores.inspect_for_start(cluster, |s, sf, l, lf| {
                inspect_connections(s, sf, l, lf, cluster).map_err(|_| StoreError::Corrupt)
            })?;
            if recovery {
                let e = expected.unwrap();
                let state = &pair.0;
                if state
                    .applied
                    .is_none_or(|p| p.index < e.origin.position().index || p < e.origin.position())
                {
                    return Err(BoundFailure::Inspection(Refusal::Identity));
                }
                let image = snapshot_state(&state.snapshot).map_err(BoundFailure::Init)?;
                image
                    .authorize_peer(e.cluster, e.node, e.key)
                    .map_err(|_| BoundFailure::Inspection(Refusal::Identity))?;
                HostClock::new(image.committed_ms())
                    .sample_ms()
                    .map_err(|_| BoundFailure::Inspection(Refusal::Clock))?;
            } else {
                let (state, log) = &pair;
                if state.applied.is_some()
                    || state.membership.is_some()
                    || log.last.is_some()
                    || log.has_vote
                    || snapshot_state(&state.snapshot).map_err(BoundFailure::Init)?
                        != crate::cluster::enrollment::EnrollmentState::new(
                            cluster,
                            init.first_free,
                        )
                {
                    return Err(BoundFailure::Inspection(Refusal::Provenance));
                }
            }
            Ok(Some(pair))
        },
        |pair, state, log| {
            if let Some((inspected, _)) = pair
                && (state.snapshot() != inspected.snapshot
                    || state.membership() != inspected.membership.as_ref())
            {
                return Err(BoundFailure::Inspection(Refusal::Store));
            }
            // Exact transferred objects; same unchanged preflight as legacy start.
            preflight(log, state).map_err(BoundFailure::Init)?;
            if let Some(e) = expected {
                state
                    .state()
                    .authorize_peer(e.cluster, e.node, e.key)
                    .map_err(|_| BoundFailure::Inspection(Refusal::Identity))?;
                sample.set(Some(
                    HostClock::new(state.state().committed_ms())
                        .sample_ms()
                        .map_err(|_| BoundFailure::Inspection(Refusal::Clock))?,
                ));
            }
            Ok(())
        },
    );
    let observation = inspected.map(|pair| {
        if recovery {
            let (state, _) = pair.unwrap();
            let image = snapshot_state(&state.snapshot).unwrap();
            let floor = image.committed_ms();
            let clock = HostClock::new(floor);
            // Preserve the actual validated sample.
            Observation::Recovered {
                floor,
                sample: sample.get().unwrap(),
                applied: state.applied,
                membership: state.membership,
                clock,
            }
        } else {
            Observation::UntouchedGenesis { cluster }
        }
    });
    (stores, observation)
}
#[cfg(target_os = "linux")]
pub(super) fn prepare_published_join(
    owner: &mut Owner,
    intent: &StartIntent,
) -> (crate::cluster::bound_open::OwnedStores, Option<()>) {
    use crate::cluster::bound_open::{BoundFailure, BoundIntent, BoundStores};
    BoundStores::operational(
        owner,
        BoundIntent::PublishedJoin(Init {
            cluster: intent.cluster,
            first_free: NodeId::new(2).unwrap(),
        }),
        |stores| {
            stores.validate_owner_for_start(|owner| {
                check_learner_intent(owner, intent, Mode::Join)?;
                owner.published_baseline()
            })
        },
        |(), state, log| preflight(log, state).map_err(BoundFailure::Init),
    )
}

fn check_learner_intent(owner: &Owner, intent: &StartIntent, mode: Mode) -> Result<()> {
    if owner.plan.mode != mode
        || intent.mode != mode
        || intent.node.get() < 2
        || super::peer_transport::endpoint(&owner.plan.peer).ok() != Some(intent.peer)
        || super::peer_transport::endpoint(&owner.plan.enrollment).ok() != Some(intent.enrollment)
    {
        return Err(Refusal::Plan);
    }
    owner.recheck()
}

// D8 completion authority is checked by the dedicated caller under its original
// acquired Owner. This arm handles history on the retained read-only and live pair.
pub(super) fn prepare_strict_joiner(
    owner: &mut Owner,
    intent: &StartIntent,
    expected: &ExpectedIdentity,
) -> (crate::cluster::bound_open::OwnedStores, Option<HostClock>) {
    use super::peer_registry::crossed;
    use crate::cluster::bound_open::{BoundFailure, BoundIntent, BoundStores};
    let sample = std::cell::Cell::new(None);
    let (stores, inspected) = BoundStores::operational(
        owner,
        BoundIntent::Recovery(expected.cluster),
        |stores| {
            stores.validate_owner_for_start(|owner| {
                check_learner_intent(owner, intent, Mode::RecoverJoiner)?;
                if expected.origin.founder()
                    || intent.node != expected.node
                    || intent.cluster != expected.cluster
                {
                    return Err(Refusal::Identity);
                }
                authenticate(owner, expected)
            })?;
            let pair = stores.inspect_for_start(expected.cluster, |s, sf, l, lf| {
                inspect_connections(s, sf, l, lf, expected.cluster).map_err(|_| StoreError::Corrupt)
            })?;
            let (state, log) = &pair;
            let original = expected.origin.position();
            let image = snapshot_state(&state.snapshot).map_err(BoundFailure::Init)?;
            if !state.applied.is_some_and(|applied| {
                crossed(
                    expected,
                    intent.peer,
                    applied,
                    state.membership.as_ref(),
                    &image,
                )
            }) || log
                .rows
                .iter()
                .chain(log.purged.iter())
                .any(|p| p.index == original.index && *p != original)
            {
                return Err(BoundFailure::Inspection(Refusal::Identity));
            }
            HostClock::new(image.committed_ms())
                .sample_ms()
                .map_err(|_| BoundFailure::Inspection(Refusal::Clock))?;
            Ok(pair)
        },
        |(read, inspected_log), state, log| {
            preflight(log, state).map_err(BoundFailure::Init)?;
            let original = expected.origin.position();
            let live = log.log_state().map_err(BoundFailure::Init)?;
            let rows = log
                .entries(0, i64::MAX as u64 + 1, MAX_LIVE)
                .map_err(BoundFailure::Init)?;
            if state.is_poisoned()
                || state.snapshot() != read.snapshot
                || live.last != inspected_log.last
                || live.purged != inspected_log.purged
                || !rows.iter().map(|(p, _)| p).eq(&inspected_log.rows)
                || !state.applied().is_some_and(|applied| {
                    crossed(
                        expected,
                        intent.peer,
                        applied,
                        state.membership(),
                        state.state(),
                    )
                })
                || rows
                    .iter()
                    .map(|(p, _)| p)
                    .chain(live.purged.iter())
                    .any(|p| p.index == original.index && *p != original)
            {
                return Err(BoundFailure::Inspection(Refusal::Identity));
            }
            let clock = HostClock::new(state.state().committed_ms());
            clock
                .sample_ms()
                .map_err(|_| BoundFailure::Inspection(Refusal::Clock))?;
            sample.set(Some(clock));
            Ok(())
        },
    );
    (stores, inspected.and_then(|_| sample.into_inner()))
}

/// The terminal result owns the original outer lock, reopened installation
/// and every acquired store slot regardless of the validation disposition.
#[cfg(target_os = "linux")]
pub(super) struct ProcessRecovery {
    owner: Owner,
    history: Option<crate::cluster::restart_history::Recovering>,
    stores: Option<crate::cluster::bound_open::OwnedStores>,
    ready: Result<HostClock>,
}

#[cfg(target_os = "linux")]
pub(super) struct HistoryReady {
    pub(super) owner: Owner,
    pub(super) history: crate::cluster::restart_history::Recovering,
    pub(super) stores: crate::cluster::bound_open::OwnedStores,
    pub(super) clock: HostClock,
}

#[cfg(target_os = "linux")]
impl ProcessRecovery {
    #[cfg(test)]
    pub(super) fn sqlite_opens(&self) -> usize {
        self.stores
            .as_ref()
            .map_or(0, |stores| stores.io_counts().1)
    }

    pub(super) fn into_ready(self) -> std::result::Result<HistoryReady, Box<Self>> {
        if self.ready.is_err() {
            return Err(Box::new(self));
        }
        let Self {
            owner,
            history,
            stores,
            ready,
        } = self;
        Ok(HistoryReady {
            owner,
            history: history.expect("validated history retained"),
            stores: stores.expect("validated pair retained"),
            clock: ready.expect("checked clock retained"),
        })
    }
}

#[cfg(target_os = "linux")]
pub(super) fn recover_process(
    mut owner: Owner,
    slot: crate::cluster::restart_history::Slot,
    wrap: &Path,
    custody: &Path,
    require_applied_route: bool,
) -> ProcessRecovery {
    use crate::cluster::restart_history::Recovering;
    let mut history = match Recovering::open(&owner, slot, wrap, custody) {
        Ok(history) => history,
        Err(e) => {
            return ProcessRecovery {
                owner,
                history: None,
                stores: None,
                ready: Err(e),
            };
        }
    };
    let (stores, ready) = match history.authenticate(&owner) {
        Err(e) => (None, Err(e)),
        Ok(()) => {
            let (stores, ready) = historical_pair(&mut owner, &mut history, require_applied_route);
            (Some(stores), ready)
        }
    };
    ProcessRecovery {
        owner,
        history: Some(history),
        stores,
        ready,
    }
}

#[cfg(target_os = "linux")]
fn historical_pair(
    owner: &mut Owner,
    history: &mut crate::cluster::restart_history::Recovering,
    require_applied_route: bool,
) -> (crate::cluster::bound_open::OwnedStores, Result<HostClock>) {
    use crate::cluster::bound_open::{BoundFailure, BoundIntent, BoundStores};
    let cluster = history.verified().unwrap().expected.cluster;
    let address = owner.plan.peer.parse().expect("validated owner peer");
    let (stores, inspected) = BoundStores::operational(
        owner,
        BoundIntent::Recovery(cluster),
        |stores| {
            let (state, log) = stores.inspect_for_start(cluster, |s, sf, l, lf| {
                inspect_connections(s, sf, l, lf, cluster).map_err(|_| StoreError::Corrupt)
            })?;
            let image = snapshot_state(&state.snapshot).map_err(BoundFailure::Init)?;
            let evidence = history.verified().map_err(BoundFailure::Inspection)?;
            check_history_state(
                evidence,
                address,
                require_applied_route,
                state.applied,
                state.membership.as_ref(),
                &image,
                &log,
            )
            .map_err(BoundFailure::Inspection)?;
            let clock = HostClock::new(image.committed_ms());
            let now = clock
                .sample_ms()
                .map_err(|_| BoundFailure::Inspection(Refusal::Clock))?;
            stores.validate_owner_for_start(|owner| history.validate_credentials(owner, now))?;
            Ok((state, log, clock))
        },
        |(inspected, inspected_log, clock), state, log| {
            preflight(log, state).map_err(BoundFailure::Init)?;
            let live = log.log_state().map_err(BoundFailure::Init)?;
            let rows = log
                .entries(0, i64::MAX as u64 + 1, MAX_LIVE)
                .map_err(BoundFailure::Init)?;
            if state.is_poisoned()
                || state.snapshot() != inspected.snapshot
                || state.applied() != inspected.applied
                || state.membership() != inspected.membership.as_ref()
                || live.last != inspected_log.last
                || live.purged != inspected_log.purged
                || !rows
                    .iter()
                    .map(|(position, _)| position)
                    .eq(&inspected_log.rows)
            {
                return Err(BoundFailure::Inspection(Refusal::Provenance));
            }
            clock
                .sample_ms()
                .map_err(|_| BoundFailure::Inspection(Refusal::Clock))?;
            Ok(())
        },
    );
    let result = match inspected {
        Some((_, _, clock)) => clock
            .sample_ms()
            .map_err(|_| Refusal::Clock)
            .and_then(|now| history.validate_credentials(owner, now))
            .map(|()| clock),
        None => Err(stores.first_failure_for_learner()),
    };
    (stores, result)
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn check_history_state(
    history: &crate::cluster::restart_history::Verified,
    address: std::net::SocketAddr,
    require_applied_route: bool,
    applied: Option<LogPos>,
    membership: Option<&(LogPos, MembershipBlob)>,
    state: &crate::cluster::enrollment::EnrollmentState,
    log: &LogInspection,
) -> Result<()> {
    use crate::cluster::restart_history::covers;
    let expected = &history.expected;
    let applied = applied.ok_or(Refusal::Provenance)?;
    let original = expected.origin.position();
    for required in [
        Some(original),
        history.strict_position(),
        history.voter_position(),
    ]
    .into_iter()
    .flatten()
    {
        if !covers(applied, required)
            || log.rows.iter().chain(log.purged.iter()).any(|p| {
                if p.index < required.index {
                    !covers(required, *p)
                } else {
                    !covers(*p, required)
                }
            })
        {
            return Err(Refusal::Provenance);
        }
    }
    // The canonical state image was decoded as a whole and validated with
    // preflight. A truncated retained log must still be compatible with it.
    if !super::peer_registry::crossed(expected, address, applied, membership, state) {
        return Err(Refusal::Identity);
    }
    if expected.origin.founder() || require_applied_route {
        let (_, blob) = membership.ok_or(Refusal::Identity)?;
        let members = super::decode(blob).map_err(|_| Refusal::Identity)?;
        if members
            .get_node(&expected.node.get())
            .is_none_or(|n| n.addr != address.to_string())
        {
            return Err(Refusal::Identity);
        }
    }
    Ok(())
}

fn readonly(db: &Connection, file: &File, max_pages: u64, schema: &[(&str, &str)]) -> Result<()> {
    let len = file.metadata().map_err(|_| Refusal::Store)?.len();
    if len < 4096 || len > max_pages * 4096 {
        return Err(Refusal::Store);
    }
    let mut header = [0u8; 100];
    let mut offset = 0;
    while offset < header.len() {
        #[cfg(windows)]
        let n = {
            use std::os::windows::fs::FileExt;
            file.seek_read(&mut header[offset..], offset as u64)
        };
        #[cfg(unix)]
        let n = {
            use std::os::unix::fs::FileExt;
            file.read_at(&mut header[offset..], offset as u64)
        };
        let n = n.map_err(|_| Refusal::Store)?;
        if n == 0 {
            return Err(Refusal::Store);
        }
        offset += n;
    }
    if &header[..16] != b"SQLite format 3\0" || header[18] != 1 || header[19] != 1 {
        return Err(Refusal::Store);
    }
    let defensive = db
        .set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(|_| Refusal::Store)?;
    let trusted = db
        .set_db_config(
            rusqlite::config::DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA,
            false,
        )
        .map_err(|_| Refusal::Store)?;
    if !defensive || trusted {
        return Err(Refusal::Store);
    }
    #[cfg(test)]
    tests::observe_schema_expression(db);
    #[cfg(test)]
    crate::cluster::bound_open::tests::application_sql();
    for (key, exact, max) in [
        ("user_version", Some(2), 2),
        ("page_size", Some(4096), 4096),
        ("page_count", None, max_pages),
    ] {
        let n: u64 = db
            .pragma_query_value(None, key, |r| r.get(0))
            .map_err(|_| Refusal::Store)?;
        if n == 0 || n > max || exact.is_some_and(|e| n != e) {
            return Err(Refusal::Store);
        }
        if key == "page_count" && n * 4096 != len {
            return Err(Refusal::Store);
        }
    }
    db.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA query_only=ON;")
        .map_err(|_| Refusal::Store)?;
    let mut stmt = db
        .prepare("SELECT type,substr(name,1,65),substr(sql,1,513) FROM sqlite_schema ORDER BY name LIMIT 4")
        .map_err(|_| Refusal::Store)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|_| Refusal::Store)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Refusal::Store)?;
    if rows.len() != schema.len()
        || rows
            .iter()
            .zip(schema)
            .any(|((t, n, s), (name, sql))| t != "table" || n != name || s != sql)
    {
        return Err(Refusal::Store);
    }
    drop(stmt);
    Ok(())
}
fn integrity(db: &Connection) -> Result<()> {
    // Integrity traversal evaluates CHECK expressions. Only the exact bounded
    // current schemas are permitted to reach it, even with trusted_schema off.
    #[cfg(test)]
    tests::mark_integrity();
    let integrity: String = db
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(|_| Refusal::Store)?;
    if integrity != "ok" {
        return Err(Refusal::Store);
    }
    Ok(())
}
struct StateInspection {
    snapshot: Vec<u8>,
    applied: Option<LogPos>,
    membership: Option<(LogPos, MembershipBlob)>,
}
fn state_schema(db: &Connection, file: &File) -> Result<()> {
    readonly(
        db,
        file,
        64,
        &[(
            "sm",
            "CREATE TABLE sm(singleton INTEGER PRIMARY KEY CHECK(singleton=1), snapshot BLOB NOT NULL CHECK(length(snapshot)<=16384))",
        )],
    )
}
#[cfg(test)]
fn inspect_state_bound(
    db: &Connection,
    file: &File,
    cluster: ClusterId,
) -> Result<StateInspection> {
    state_schema(db, file)?;
    integrity(db)?;
    inspect_state_rows(db, cluster)
}
fn inspect_state_rows(db: &Connection, cluster: ClusterId) -> Result<StateInspection> {
    let mut stmt = db
        .prepare("SELECT singleton,snapshot FROM sm LIMIT 2")
        .map_err(|_| Refusal::Store)?;
    let mut rows = stmt.query([]).map_err(|_| Refusal::Store)?;
    let row = rows
        .next()
        .map_err(|_| Refusal::Store)?
        .ok_or(Refusal::Store)?;
    if row.get::<_, i64>(0).map_err(|_| Refusal::Store)? != 1 {
        return Err(Refusal::Store);
    }
    let raw = row
        .get_ref(1)
        .map_err(|_| Refusal::Store)?
        .as_blob()
        .map_err(|_| Refusal::Store)?;
    if raw.len() > 16384 {
        return Err(Refusal::Store);
    }
    let blob = raw.to_vec();
    let (applied, membership) =
        StateStore::inspect_snapshot(&blob, cluster).map_err(|_| Refusal::Store)?;
    if rows.next().map_err(|_| Refusal::Store)?.is_some() {
        return Err(Refusal::Store);
    }
    Ok(StateInspection {
        snapshot: blob,
        applied,
        membership,
    })
}
#[cfg(test)]
fn inspect_state(dir: &Path, cluster: ClusterId) -> Result<StateInspection> {
    let path = dir.join("cluster-state.sqlite3");
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| Refusal::Store)?;
    let file = File::open(path).map_err(|_| Refusal::Store)?;
    inspect_state_bound(&db, &file, cluster)
}
#[cfg(test)]
fn inspect_log(dir: &Path, cluster: ClusterId) -> Result<LogInspection> {
    let path = dir.join("cluster-log.sqlite3");
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| Refusal::Store)?;
    let file = File::open(path).map_err(|_| Refusal::Store)?;
    inspect_log_bound(&db, &file, cluster)
}
fn snapshot_state(
    blob: &[u8],
) -> std::result::Result<crate::cluster::enrollment::EnrollmentState, StoreError> {
    // Called only after inspect_snapshot has validated the complete canonical image.
    let mut r = Reader(&blob[..blob.len() - 32]);
    r.bytes(10)?;
    if r.flag()? {
        get_pos(&mut r)?;
    }
    if r.flag()? {
        get_pos(&mut r)?;
        let n = r.u16()? as usize;
        r.bytes(n)?;
    }
    codec::decode(&mut r)
}
struct LogInspection {
    purged: Option<LogPos>,
    last: Option<LogPos>,
    rows: Vec<LogPos>,
    has_vote: bool,
}
fn reader(b: &[u8], max: usize) -> std::result::Result<Reader<'_>, StoreError> {
    if !(32..=max).contains(&b.len()) {
        return Err(StoreError::Corrupt);
    }
    let (p, h) = b.split_at(b.len() - 32);
    if Sha256::digest(p).as_slice() != h {
        return Err(StoreError::Corrupt);
    }
    Ok(Reader(p))
}
fn position(r: &mut Reader<'_>) -> std::result::Result<LogPos, StoreError> {
    let p = get_pos(r)?;
    if p.index > i64::MAX as u64 {
        return Err(StoreError::Corrupt);
    }
    Ok(p)
}
fn log_schema(db: &Connection, file: &File) -> Result<()> {
    readonly(
        db,
        file,
        2048,
        &[
            (
                "log",
                "CREATE TABLE log(idx INTEGER PRIMARY KEY CHECK(idx>=0), body BLOB NOT NULL CHECK(length(body)<=4163))",
            ),
            (
                "meta",
                "CREATE TABLE meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1), blob BLOB NOT NULL CHECK(length(blob)<=256))",
            ),
        ],
    )
}
#[cfg(test)]
fn inspect_log_bound(db: &Connection, file: &File, cluster: ClusterId) -> Result<LogInspection> {
    log_schema(db, file)?;
    integrity(db)?;
    inspect_log_rows(db, cluster)
}
fn inspect_log_rows(db: &Connection, cluster: ClusterId) -> Result<LogInspection> {
    inspect_log_connection(db, cluster).map_err(|_| Refusal::Store)
}
fn inspect_log_connection(
    db: &Connection,
    cluster: ClusterId,
) -> std::result::Result<LogInspection, StoreError> {
    let mut stmt = db.prepare("SELECT singleton,blob FROM meta LIMIT 2")?;
    let mut rows = stmt.query([])?;
    let row = rows.next()?.ok_or(StoreError::Corrupt)?;
    if row.get::<_, i64>(0)? != 1 {
        return Err(StoreError::Corrupt);
    }
    let raw = row.get_ref(1)?;
    let b = raw.as_blob().map_err(|_| StoreError::Corrupt)?;
    let mut r = reader(b, 256)?;
    if r.bytes(8)? != b"ARXLM1\0\0" || r.u16()? != 2 || r.array::<16>()? != cluster {
        return Err(StoreError::Corrupt);
    }
    let has_vote = r.flag()?;
    let mut canonical = b"ARXLM1\0\0".to_vec();
    canonical.extend(2u16.to_be_bytes());
    canonical.extend(cluster);
    canonical.push(u8::from(has_vote));
    if has_vote {
        canonical.extend(r.u64()?.to_be_bytes());
        canonical.extend(r.u64()?.to_be_bytes());
        canonical.push(u8::from(r.flag()?));
    }
    let purged = if r.flag()? {
        Some(position(&mut r)?)
    } else {
        None
    };
    let tail = if r.flag()? {
        let first = r.u64()?;
        let last = position(&mut r)?;
        if first > last.index
            || last.index - first >= MAX_LIVE as u64
            || purged.is_some_and(|p| p.index.checked_add(1) != Some(first) || last <= p)
        {
            return Err(StoreError::Corrupt);
        }
        Some((first, last))
    } else {
        None
    };
    canonical.push(u8::from(purged.is_some()));
    if let Some(p) = purged {
        put_pos(&mut canonical, p);
    }
    canonical.push(u8::from(tail.is_some()));
    if let Some((first, last)) = tail {
        canonical.extend(first.to_be_bytes());
        put_pos(&mut canonical, last);
    }
    if !r.0.is_empty() || canonical.as_slice() != &b[..b.len() - 32] || rows.next()?.is_some() {
        return Err(StoreError::Corrupt);
    }
    let mut stmt = db.prepare("SELECT idx,body FROM log ORDER BY idx LIMIT 513")?;
    let mut rows = stmt.query([])?;
    let mut positions = Vec::new();
    while let Some(row) = rows.next()? {
        #[cfg(test)]
        tests::scan_row();
        if positions.len() == MAX_LIVE {
            return Err(StoreError::Corrupt);
        }
        let index: i64 = row.get(0)?;
        let raw = row.get_ref(1)?;
        let b = raw.as_blob().map_err(|_| StoreError::Corrupt)?;
        let mut r = reader(b, 4163)?;
        if r.bytes(8)? != b"ARXLE2\0\0" {
            return Err(StoreError::Corrupt);
        }
        let p = position(&mut r)?;
        if index < 0
            || index as u64 != p.index
            || positions.last().is_some_and(|prev: &LogPos| {
                prev.index.checked_add(1) != Some(p.index) || *prev >= p
            })
        {
            return Err(StoreError::Corrupt);
        }
        let entry = match r.bytes(1)?[0] {
            0 => Entry::Blank,
            t @ (1 | 2 | 4 | 5 | 6) => Entry::Enrollment(codec::decode_command(t, &mut r)?),
            3 => {
                let n = r.u16()? as usize;
                let m = MembershipBlob::new(r.bytes(n)?.to_vec())?;
                crate::cluster::membership::Membership::decode(m.bytes())
                    .map_err(|_| StoreError::Membership)?;
                Entry::Membership(m)
            }
            _ => return Err(StoreError::Corrupt),
        };
        let mut canonical = b"ARXLE2\0\0".to_vec();
        put_pos(&mut canonical, p);
        match entry {
            Entry::Blank => canonical.push(0),
            Entry::Enrollment(c) => codec::encode_command(&c, &mut canonical),
            Entry::Membership(m) => {
                canonical.push(3);
                canonical.extend((m.bytes().len() as u16).to_be_bytes());
                canonical.extend(m.bytes());
            }
        }
        if !r.0.is_empty() || canonical.as_slice() != &b[..b.len() - 32] {
            return Err(StoreError::Corrupt);
        }
        positions.push(p);
    }
    match (tail, positions.first(), positions.last()) {
        (None, None, None) => {}
        (Some((f, l)), Some(a), Some(b))
            if a.index == f
                && *b == l
                && positions.len() == (l.index - f + 1) as usize
                && purged.is_none_or(|p| *a > p) => {}
        _ => return Err(StoreError::Corrupt),
    }
    Ok(LogInspection {
        purged,
        last: tail.map(|(_, l)| l).or(purged),
        rows: positions,
        has_vote,
    })
}
fn validate_preflight(
    applied: Option<LogPos>,
    log: &LogInspection,
) -> std::result::Result<(), StoreError> {
    if applied.is_some_and(|a| a.index > i64::MAX as u64) {
        return Err(StoreError::Corrupt);
    }
    let valid = match applied {
        Some(a) => match log.purged {
            Some(p) if p.index > a.index => false,
            Some(p) if p.index == a.index => p == a,
            _ if log.last.is_none_or(|l| l.index < a.index) => log.last < Some(a),
            _ => log.rows.contains(&a),
        },
        None => log.purged.is_none(),
    };
    valid.then_some(()).ok_or(StoreError::Corrupt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{
        enrollment::{Command, FoundMembers, NodeId},
        join_receipt::{Origin, tests::fixture},
        startup_plan::tests::{Layout, guards},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    #[cfg(target_os = "linux")]
    mod process_history {
        use super::*;
        use crate::cluster::restart_history::{TestFault, tests as fixture};

        fn run(base: &Path, founder: bool, require_route: bool, expected_opens: usize) {
            let plan = fixture::plan(
                base,
                if founder {
                    Mode::CreateFounder
                } else {
                    Mode::Join
                },
            )
            .with_history_intent(if founder {
                Mode::RecoverFounder
            } else {
                Mode::RecoverJoiner
            });
            let owner = Owner::acquire(plan).unwrap();
            let result = recover_process(
                owner,
                fixture::slot(base),
                &base.join("wrap"),
                &base.join("custody"),
                require_route,
            );
            assert_eq!(result.sqlite_opens(), expected_opens);
            let external = OpenOptions::new()
                .read(true)
                .write(true)
                .open(base.join("state/cluster-owner.lock"))
                .unwrap();
            assert!(
                external.try_lock().is_err(),
                "outer lock remains owned by the terminal result"
            );
            if expected_opens == 4 {
                let Ok(ready) = result.into_ready() else {
                    panic!("original pair must qualify")
                };
                let evidence = ready.history.verified().unwrap();
                assert_eq!(evidence.expected.origin.position(), fixture::E);
                assert!(ready.clock.sample_ms().unwrap() >= 100);
                assert_eq!(ready.stores.io_counts().1, 4);
            } else {
                assert!(result.into_ready().is_err());
            }
        }

        const POSITIVE: &str = "cluster::consensus::startup_recovery::tests::process_history::process_history_exact_pair_reopens_in_other_process";
        #[test]
        fn process_history_exact_pair_reopens_in_other_process() {
            if let Some((parent, case)) = fixture::probe() {
                return run(&parent, case == "founder", false, 4);
            }
            for founder in [true, false] {
                let mut f = fixture::Fixture::new(founder, true, None);
                let route = f.owner.plan.peer.clone();
                f.seed(fixture::E, fixture::E, founder.then_some(route.as_str()));
                if !founder {
                    f.strict(fixture::E);
                }
                let layout = f.release();
                fixture::subprocess(
                    POSITIVE,
                    layout.root.path(),
                    if founder { "founder" } else { "learner" },
                );
            }
        }

        const BOUNDARY: &str = "cluster::consensus::startup_recovery::tests::process_history::process_history_temporary_and_final_boundaries";
        #[test]
        fn process_history_temporary_and_final_boundaries() {
            if let Some((parent, case)) = fixture::probe() {
                return run(&parent, true, true, if case == "final" { 4 } else { 0 });
            }
            for (case, fault, file) in [
                ("temporary", TestFault::FileSync, "history/.issuance.tmp"),
                ("final", TestFault::DirectorySync, "history/issuance"),
            ] {
                let mut f = fixture::Fixture::new(true, true, Some(fault));
                let route = f.owner.plan.peer.clone();
                f.seed(fixture::E, fixture::E, Some(&route));
                let layout = f.release();
                assert!(layout.root.path().join(file).is_file());
                fixture::subprocess(BOUNDARY, layout.root.path(), case);
                assert!(layout.root.path().join(file).is_file());
            }
        }

        #[test]
        fn process_history_receipt_only_and_partial_objects_open_no_stores() {
            let e = fixture::E;
            let mut receipt = fixture::Fixture::new(false, false, None);
            receipt.seed(e, e, None);
            let receipt = receipt.release();
            run(receipt.root.path(), false, false, 0);

            for entry in [".voter.tmp", "unknown", ".strict.tmp"] {
                let mut f = fixture::Fixture::new(false, true, None);
                f.seed(e, e, None);
                f.strict(e);
                let layout = f.release();
                let name = format!("history/{entry}");
                layout.write(&name, b"partial");
                run(layout.root.path(), false, false, 0);
                assert!(layout.root.path().join(name).is_file());
            }
            let mut orphan = fixture::Fixture::new(false, true, None);
            orphan.seed(e, e, None);
            orphan.strict(e);
            let orphan = orphan.release();
            fixture::private_dir(orphan.root.path(), "voter-custody");
            run(orphan.root.path(), false, false, 0);
        }

        #[test]
        fn process_history_refuses_pre_e_conflicting_position_and_missing_route_readonly() {
            let e = fixture::E;
            let later = LogPos {
                index: e.index + 1,
                term: e.term + 2,
                ..e
            };
            for (applied, row, strict, route_required) in [
                (
                    LogPos {
                        index: e.index - 1,
                        ..e
                    },
                    e,
                    e,
                    false,
                ),
                (
                    LogPos {
                        term: e.term + 1,
                        ..e
                    },
                    LogPos {
                        term: e.term + 1,
                        ..e
                    },
                    e,
                    false,
                ),
                (
                    LogPos {
                        leader: e.leader + 1,
                        ..e
                    },
                    LogPos {
                        leader: e.leader + 1,
                        ..e
                    },
                    e,
                    false,
                ),
                (
                    later,
                    LogPos {
                        leader: e.leader + 1,
                        ..e
                    },
                    e,
                    false,
                ),
                (e, e, later, false),
                (e, e, e, true),
            ] {
                let mut f = fixture::Fixture::new(false, true, None);
                f.seed(applied, row, None);
                f.strict(strict);
                let layout = f.release();
                let before = layout.snapshot();
                run(layout.root.path(), false, route_required, 2);
                assert_eq!(layout.snapshot(), before);
            }
        }

        #[test]
        fn process_history_reopens_after_same_term_leader_succession() {
            use crate::cluster::membership::Membership;
            let e = fixture::E;
            let promoted = LogPos {
                index: e.index + 1,
                ..e
            };
            let mut f = fixture::Fixture::new(false, true, None);
            let local = f.expected.node.get();
            let successor = LogPos {
                leader: local,
                index: promoted.index + 1,
                ..e
            };
            f.seed(e, e, None);
            let members = Membership::new(
                vec![[e.leader, local].into()],
                [
                    (e.leader, "127.0.0.1:7991".into()),
                    (local, f.owner.plan.peer.clone()),
                ]
                .into(),
            )
            .unwrap();
            let entries = [
                (
                    promoted,
                    Entry::Membership(MembershipBlob::new(members.encode().unwrap()).unwrap()),
                ),
                (successor, Entry::Blank),
            ];
            let state = f.state.as_mut().unwrap();
            for (position, entry) in &entries {
                state.apply(*position, entry).unwrap();
            }
            assert_eq!(state.applied(), Some(successor));
            f.log.as_mut().unwrap().append(&entries).unwrap();
            f.strict(successor);
            let layout = f.release();
            run(layout.root.path(), false, true, 4);
        }

        #[test]
        fn process_history_validated_snapshot_above_e_and_hot_sidecar_refusal() {
            let e = fixture::E;
            let later = LogPos {
                index: e.index + 1,
                term: e.term + 1,
                ..e
            };
            let mut f = fixture::Fixture::new(false, true, None);
            let peer = f.owner.plan.peer.clone();
            f.seed(later, e, Some(&peer));
            f.strict(e);
            f.log.as_mut().unwrap().purge(later).unwrap();
            let layout = f.release();
            run(layout.root.path(), false, true, 4);
            layout.write("state/cluster-log.sqlite3-journal", b"nonzero");
            assert!(matches!(
                Owner::acquire(fixture::plan(layout.root.path(), Mode::RecoverJoiner)),
                Err(Refusal::Store)
            ));
            assert_eq!(
                std::fs::read(layout.root.path().join("state/cluster-log.sqlite3-journal"))
                    .unwrap(),
                b"nonzero"
            );
        }
    }

    thread_local! {
        static SCHEMA_PROBE: std::cell::RefCell<Option<Arc<AtomicUsize>>> = const { std::cell::RefCell::new(None) };
        static MUTABLE_OPENS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static PROBE_CONNECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        static INTEGRITY_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    pub(super) fn mark_mutable_open() {
        MUTABLE_OPENS.with(|n| n.set(n.get() + 1));
    }
    pub(super) fn mark_integrity() {
        INTEGRITY_CALLS.with(|n| n.set(n.get() + 1));
    }
    // Test-only harmless CHECK evaluation counter, deliberately INNOCUOUS so
    // trusted_schema=OFF alone cannot masquerade as schema-before-traversal.
    fn register_schema_probe(db: &Connection, count: Arc<AtomicUsize>) {
        unsafe extern "C" fn evaluate(
            context: *mut rusqlite::ffi::sqlite3_context,
            _: i32,
            _: *mut *mut rusqlite::ffi::sqlite3_value,
        ) {
            let count =
                unsafe { &*rusqlite::ffi::sqlite3_user_data(context).cast::<AtomicUsize>() };
            count.fetch_add(1, Ordering::SeqCst);
            unsafe {
                rusqlite::ffi::sqlite3_result_int(context, 1);
            }
        }
        unsafe extern "C" fn release(data: *mut std::ffi::c_void) {
            drop(unsafe { Arc::from_raw(data.cast::<AtomicUsize>()) });
        }
        let code = unsafe {
            rusqlite::ffi::sqlite3_create_function_v2(
                db.handle(),
                c"arx_schema_probe".as_ptr(),
                1,
                rusqlite::ffi::SQLITE_UTF8 | rusqlite::ffi::SQLITE_INNOCUOUS,
                Arc::into_raw(count).cast_mut().cast(),
                Some(evaluate),
                None,
                None,
                Some(release),
            )
        };
        assert_eq!(code, rusqlite::ffi::SQLITE_OK);
    }
    pub(super) fn observe_schema_expression(db: &Connection) {
        SCHEMA_PROBE.with(|slot| {
            if let Some(count) = slot.borrow().as_ref() {
                PROBE_CONNECTIONS.with(|n| n.set(n.get() + 1));
                register_schema_probe(db, count.clone());
            }
        });
    }
    struct ResetProbe;
    impl Drop for ResetProbe {
        fn drop(&mut self) {
            SCHEMA_PROBE.with(|slot| *slot.borrow_mut() = None);
        }
    }
    thread_local! {
        static CREATION_FAULT: std::cell::RefCell<Option<(&'static str, bool)>> = const { std::cell::RefCell::new(None) };
        static EFFECT: std::cell::RefCell<Vec<(String, Vec<u8>)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    struct CreationFault;
    impl CreationFault {
        fn new(stage: &'static str, panic: bool) -> Self {
            CREATION_FAULT.with(|s| {
                assert!(s.borrow_mut().replace((stage, panic)).is_none());
            });
            Self
        }
    }
    impl Drop for CreationFault {
        fn drop(&mut self) {
            CREATION_FAULT.with(|s| *s.borrow_mut() = None);
        }
    }
    fn state_bytes(owner: &Owner) -> Vec<(String, Vec<u8>)> {
        let mut rows = std::fs::read_dir(&owner.plan.state)
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    e.file_name().into_string().unwrap(),
                    if e.metadata().unwrap().len() == 0 {
                        Vec::new()
                    } else {
                        std::fs::read(e.path()).unwrap()
                    },
                )
            })
            .collect::<Vec<_>>();
        rows.sort();
        rows
    }
    pub(super) fn after_creation_result(result: &Result<()>) {
        if result.is_err() && CREATION_FAULT.with(|s| s.borrow().is_some_and(|(_, panic)| panic)) {
            panic!("ordinary unwind after recorded first refusal");
        }
    }
    pub(super) fn creation_phase(stage: &str, owner: &Owner) -> Result<()> {
        use crate::cluster::startup_plan::tests::objects;
        eprintln!("{stage}: {:?}", objects(&owner.plan.state));
        if stage == "before-capture" {
            assert_eq!(owner.recheck(), Err(Refusal::Layout));
        }
        let fault = CREATION_FAULT.with(|s| *s.borrow());
        if let Some((target, panic)) = fault
            && target == stage
        {
            EFFECT.with(|s| *s.borrow_mut() = state_bytes(owner));
            if panic && stage == "captured" {
                panic!("ordinary creation unwind");
            }
            return Err(Refusal::Clock);
        }
        Ok(())
    }
    fn intent(mode: Mode) -> StartIntent {
        StartIntent {
            mode,
            cluster: [1; 16],
            node: NodeId::new(1).unwrap(),
            peer: "127.0.0.1:7001".parse().unwrap(),
            enrollment: "127.0.0.1:7002".parse().unwrap(),
        }
    }
    #[test]
    fn handoff_genuine_fresh_capture_drop_inspection() {
        let l = Layout::new();
        let prepared = prepare(l.plan("create-founder"), None, Some([1; 16]), false).unwrap();
        creation_phase("owner-only", &prepared.owner).unwrap();
        let original = crate::cluster::startup_plan::tests::guards(&prepared.owner);
        probe_outer(&prepared.owner.plan.state, true);
        let retained = match prepared.handoff(intent(Mode::CreateFounder)) {
            Ok(r) => r,
            Err((r, _owner)) => panic!("genuine positive failed: {r:?}"),
        };
        assert!(
            matches!(retained.observation, Observation::UntouchedGenesis { cluster } if cluster == [1;16])
        );
        assert_eq!(
            &crate::cluster::startup_plan::tests::guards(&retained.owner)[..original.len()],
            original.as_slice()
        );
        probe_outer(&retained.owner.plan.state, true);
        let inputs = retained.checked_inputs().unwrap();
        assert_eq!(inputs.init.cluster, [1; 16]);
        assert_eq!(inputs.init.first_free.get(), 2);
        assert_eq!(inputs.dir, retained.owner.plan.state);
        assert_eq!(inputs.node.get(), 1);
        assert_eq!(inputs.peer, intent(Mode::CreateFounder).peer);
        assert_eq!(inputs.enrollment, intent(Mode::CreateFounder).enrollment);
        let dir = retained.owner.plan.state.clone();
        drop(retained);
        probe_outer(&dir, false);
    }
    fn probe_outer(dir: &Path, held: bool) {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(dir.join("cluster-owner.lock"))
            .unwrap();
        assert_eq!(file.try_lock().is_err(), held);
    }
    #[test]
    fn handoff_existing_owner_moves_and_intent_refusals() {
        for recovery in [false, true] {
            let l = Layout::new();
            initialized(&l);
            let expected = if recovery {
                let e = identity(&l);
                found(&l, &e, 0); // store command only; no ConsensusNode/Engine
                Some(e)
            } else {
                None
            };
            let mode = if recovery {
                Mode::RecoverFounder
            } else {
                Mode::CreateFounder
            };
            let plan_mode = if recovery {
                "recover-founder"
            } else {
                "create-founder"
            };
            let mut prepared =
                prepare(l.plan(plan_mode), expected.as_ref(), Some([1; 16]), false).unwrap();
            let original = crate::cluster::startup_plan::tests::guards(&prepared.owner);
            let bytes = l.snapshot();
            for case in 0..5 {
                let mut bad = intent(mode);
                let refusal = match case {
                    0 => {
                        bad.mode = Mode::RecoverJoiner;
                        Refusal::Plan
                    }
                    1 => {
                        bad.peer.set_port(7003);
                        Refusal::Plan
                    }
                    2 => {
                        bad.enrollment.set_port(7003);
                        Refusal::Plan
                    }
                    3 => {
                        bad.cluster = [2; 16];
                        Refusal::Identity
                    }
                    _ => {
                        bad.node = NodeId::new(2).unwrap();
                        Refusal::Identity
                    }
                };
                let (r, returned) = prepared.handoff(bad).err().unwrap();
                assert_eq!(r, refusal);
                assert!(matches!(returned.creation, Creation::Ready));
                assert_eq!(l.snapshot(), bytes);
                assert_eq!(
                    crate::cluster::startup_plan::tests::guards(&returned.owner),
                    original
                );
                probe_outer(&returned.owner.plan.state, true);
                prepared = returned;
            }
            let retained = prepared.handoff(intent(mode)).ok().unwrap();
            assert_eq!(l.snapshot(), bytes);
            assert_eq!(
                crate::cluster::startup_plan::tests::guards(&retained.owner),
                original
            );
            retained.checked_inputs().unwrap();
            probe_outer(&retained.owner.plan.state, true);
            let dir = retained.owner.plan.state.clone();
            drop(retained);
            probe_outer(&dir, false);
        }
    }
    #[test]
    fn handoff_missing_cluster_join_and_fresh_refusals() {
        for case in 0..7 {
            let l = Layout::new();
            let join = case == 1;
            let mode = if join {
                Mode::Join
            } else {
                Mode::CreateFounder
            };
            let prepared = prepare(
                l.plan(if join { "join" } else { "create-founder" }),
                None,
                if case == 0 { None } else { Some([1; 16]) },
                false,
            )
            .unwrap();
            let bytes = l.snapshot();
            let mut bad = intent(mode);
            let refusal = match case {
                0 | 1 => Refusal::Provenance,
                2 => {
                    bad.cluster = [2; 16];
                    Refusal::Identity
                }
                3 => {
                    bad.node = NodeId::new(2).unwrap();
                    Refusal::Identity
                }
                4 => {
                    bad.mode = Mode::RecoverFounder;
                    Refusal::Plan
                }
                5 => {
                    bad.peer.set_port(7003);
                    Refusal::Plan
                }
                _ => {
                    bad.enrollment.set_port(7003);
                    Refusal::Plan
                }
            };
            let (r, prepared) = prepared.handoff(bad).err().unwrap();
            assert_eq!(r, refusal);
            assert_eq!(l.snapshot(), bytes);
            assert!(matches!(prepared.creation, Creation::Ready));
            probe_outer(&prepared.owner.plan.state, true);
            if case >= 2 {
                let retained = prepared.handoff(intent(mode)).ok().unwrap();
                retained.checked_inputs().unwrap();
            }
        }
    }
    #[test]
    fn handoff_real_stage_failure_sticky_and_unwind() {
        use crate::cluster::startup_plan::tests::{CaptureFault, baseline};
        for stage in [
            "first-open",
            "before-capture",
            "staged",
            "captured",
            "dropped",
            "inspected",
            "unwind",
            "refusal-unwind",
        ] {
            let l = Layout::new();
            let prepared = prepare(l.plan("create-founder"), None, Some([1; 16]), false).unwrap();
            let original = baseline(&prepared.owner);
            let fault = CreationFault::new(
                match stage {
                    "unwind" => "captured",
                    "refusal-unwind" => "first-open",
                    _ => stage,
                },
                matches!(stage, "unwind" | "refusal-unwind"),
            );
            let staged = (stage == "staged").then(CaptureFault::new);
            let (r, mut prepared) = prepared.handoff(intent(Mode::CreateFounder)).err().unwrap();
            assert_eq!(
                r,
                if stage == "unwind" {
                    Refusal::Store
                } else {
                    Refusal::Clock
                }
            );
            assert!(matches!(prepared.creation, Creation::Failed(first) if first == r));
            if stage != "staged" {
                EFFECT.with(|s| assert_eq!(state_bytes(&prepared.owner), *s.borrow()));
            } else {
                assert_eq!(baseline(&prepared.owner), original);
                assert_eq!(
                    state_bytes(&prepared.owner),
                    crate::cluster::startup_plan::tests::capture_effect()
                );
            }
            probe_outer(&prepared.owner.plan.state, true);
            drop(staged);
            drop(fault);
            // Explicit fixture restoration only: make a valid-looking genesis
            // layout after partial first-open; production never repairs anything.
            if matches!(stage, "first-open" | "refusal-unwind") {
                drop(StateStore::open(&prepared.owner.plan.state, init()).unwrap());
            }
            // Existing-only independent OS locks are available after RAII store
            // drop; this observes lock availability, not SQLite close success.
            let mut locks = Vec::new();
            for stem in ["cluster-log", "cluster-state"] {
                let f = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(prepared.owner.plan.state.join(format!("{stem}.lock")))
                    .unwrap();
                f.try_lock().unwrap();
                locks.push(f);
            }
            let inspected = inspect_state(&prepared.owner.plan.state, [1; 16]).unwrap();
            let inspected_log = inspect_log(&prepared.owner.plan.state, [1; 16]).unwrap();
            assert!(inspected.applied.is_none() && inspected.membership.is_none());
            assert!(
                inspected_log.purged.is_none()
                    && inspected_log.last.is_none()
                    && !inspected_log.has_vote
            );
            assert_eq!(
                snapshot_state(&inspected.snapshot).unwrap(),
                crate::cluster::enrollment::EnrollmentState::new([1; 16], NodeId::new(2).unwrap())
            );
            let restored = state_bytes(&prepared.owner);
            for _ in 0..2 {
                let mut bad = intent(Mode::Join);
                bad.cluster = [9; 16];
                let (again, returned) = prepared.handoff(bad).err().unwrap();
                assert_eq!(again, r); // sticky before intent/filesystem access
                assert_eq!(state_bytes(&returned.owner), restored);
                probe_outer(&returned.owner.plan.state, true);
                prepared = returned;
            }
        }
    }
    fn owned_stores(owner: &Owner) -> (VoteLogStore, StateStore) {
        (
            VoteLogStore::open(&owner.plan.state, [1; 16]).unwrap(),
            StateStore::open(&owner.plan.state, init()).unwrap(),
        )
    }
    fn fixture_remove(path: &Path) -> bool {
        match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(e) => {
                #[cfg(windows)]
                {
                    assert!(matches!(e.raw_os_error(), Some(5 | 32 | 33)), "{e}");
                    eprintln!(
                        "Windows prevention ONLY; mutation branch UNQUALIFIED: {}: {e}",
                        path.display()
                    );
                    false
                }
                #[cfg(not(windows))]
                panic!("required mutation failed: {e}")
            }
        }
    }
    #[test]
    fn handoff_capture_all_journal_masks_and_transitions() {
        use crate::cluster::startup_plan::tests::{baseline, guards};
        for (transition_bit, transition_stem) in ["cluster-log", "cluster-state"].iter().enumerate()
        {
            for mask in 0..4 {
                let l = Layout::new();
                let mut prepared =
                    prepare(l.plan("create-founder"), None, Some([1; 16]), false).unwrap();
                let original = guards(&prepared.owner);
                let (log, sm) = owned_stores(&prepared.owner);
                assert_eq!(prepared.owner.recheck(), Err(Refusal::Layout));
                let mut qualified = true;
                for (bit, stem) in ["cluster-log", "cluster-state"].iter().enumerate() {
                    if mask & (1 << bit) == 0 {
                        qualified &= fixture_remove(
                            &prepared
                                .owner
                                .plan
                                .state
                                .join(format!("{stem}.sqlite3-journal")),
                        );
                    }
                }
                if !qualified {
                    continue;
                }
                prepared.owner.capture_created_stores().unwrap();
                assert_eq!(
                    &guards(&prepared.owner)[..original.len()],
                    original.as_slice()
                );
                let fixed = baseline(&prepared.owner);
                let captured =
                    crate::cluster::startup_plan::tests::objects(&prepared.owner.plan.state);
                drop(log);
                drop(sm);
                prepared.owner.recheck().unwrap();
                {
                    let (state, log) = inspect_pair(&mut prepared.owner, [1; 16]).unwrap();
                    assert!(state.applied.is_none() && state.membership.is_none());
                    assert!(log.purged.is_none() && log.last.is_none() && !log.has_vote);
                    validate_preflight(state.applied, &log).unwrap();
                }
                assert_eq!(
                    crate::cluster::startup_plan::tests::objects(&prepared.owner.plan.state),
                    captured
                );
                prepared.owner.recheck().unwrap();
                assert_eq!(
                    prepared.owner.capture_created_stores(),
                    Err(Refusal::Provenance)
                );
                assert_eq!(baseline(&prepared.owner), fixed);
                // Test each actual post-capture transition in an isolated mask fixture.
                let journal = prepared
                    .owner
                    .plan
                    .state
                    .join(format!("{transition_stem}.sqlite3-journal"));
                if mask & (1 << transition_bit) == 0 {
                    l.write(&format!("state/{transition_stem}.sqlite3-journal"), b"");
                    assert_eq!(prepared.owner.recheck(), Err(Refusal::Layout));
                } else {
                    std::fs::write(&journal, b"grew").unwrap();
                    assert_eq!(prepared.owner.recheck(), Err(Refusal::Store));
                    std::fs::write(&journal, b"").unwrap(); // explicit test restoration
                    prepared.owner.recheck().unwrap();
                    if fixture_remove(&journal) {
                        assert_eq!(prepared.owner.recheck(), Err(Refusal::Layout));
                    }
                }
            }
        }
    }
    #[test]
    fn handoff_capture_negatives_preserve_baseline() {
        use crate::cluster::startup_plan::tests::{baseline, guards};
        for case in [
            "nonzero",
            "wal",
            "shm",
            "missing-db",
            "missing-lock",
            "unknown",
            "hardlink",
            "symlink",
            "directory",
        ] {
            let l = Layout::new();
            let mut prepared =
                prepare(l.plan("create-founder"), None, Some([1; 16]), false).unwrap();
            let original = guards(&prepared.owner);
            let base = baseline(&prepared.owner);
            let (log, sm) = owned_stores(&prepared.owner);
            let dir = prepared.owner.plan.state.clone();
            let mutate = match case {
                "nonzero" => {
                    l.write("state/cluster-state.sqlite3-journal", b"x");
                    true
                }
                "wal" => {
                    l.write("state/cluster-state.sqlite3-wal", b"");
                    true
                }
                "shm" => {
                    l.write("state/cluster-log.sqlite3-shm", b"");
                    true
                }
                "unknown" => {
                    l.write("state/unexpected", b"");
                    true
                }
                "missing-db" => fixture_remove(&dir.join("cluster-state.sqlite3")),
                "missing-lock" => fixture_remove(&dir.join("cluster-log.lock")),
                "hardlink" | "symlink" | "directory" => {
                    let target = dir.join("cluster-state.sqlite3-journal");
                    if fixture_remove(&target) {
                        match case {
                            "hardlink" => {
                                std::fs::hard_link(dir.join("cluster-log.sqlite3"), &target)
                                    .unwrap()
                            }
                            "symlink" => {
                                #[cfg(unix)]
                                std::os::unix::fs::symlink(
                                    dir.join("cluster-log.sqlite3"),
                                    &target,
                                )
                                .unwrap();
                                #[cfg(windows)]
                                {
                                    // No owner privilege request; Windows link creation is unqualified.
                                    eprintln!(
                                        "symlink mutation UNRUN on Windows (no admin action)"
                                    );
                                    continue;
                                }
                            }
                            _ => std::fs::create_dir(&target).unwrap(),
                        }
                        true
                    } else {
                        false
                    }
                }
                _ => unreachable!(),
            };
            if !mutate {
                continue;
            }
            assert!(prepared.owner.capture_created_stores().is_err(), "{case}");
            assert_eq!(baseline(&prepared.owner), base, "{case}");
            assert_eq!(guards(&prepared.owner), original, "{case}");
            probe_outer(&dir, true);
            drop(log);
            drop(sm);
        }
    }
    #[test]
    fn handoff_retained_journal_replacement_and_unknown_refuse() {
        for case in ["replacement", "unknown", "wal", "shm"] {
            let l = Layout::new();
            let prepared = prepare(l.plan("create-founder"), None, Some([1; 16]), false).unwrap();
            let retained = prepared.handoff(intent(Mode::CreateFounder)).ok().unwrap();
            retained.checked_inputs().unwrap();
            let dir = &retained.owner.plan.state;
            if case == "replacement" {
                let journal = dir.join("cluster-log.sqlite3-journal");
                let backup = l.root.path().join("saved-journal");
                match std::fs::rename(&journal, backup) {
                    Ok(()) => {
                        l.write("state/cluster-log.sqlite3-journal", b"");
                    }
                    Err(e) => {
                        #[cfg(windows)]
                        {
                            assert!(matches!(e.raw_os_error(), Some(5 | 32 | 33)), "{e}");
                            eprintln!(
                                "Windows replacement prevented ONLY; replacement refusal UNQUALIFIED"
                            );
                            continue;
                        }
                        #[cfg(not(windows))]
                        panic!("replacement must happen: {e}");
                    }
                }
            } else {
                l.write(
                    match case {
                        "wal" => "state/cluster-state.sqlite3-wal",
                        "shm" => "state/cluster-state.sqlite3-shm",
                        _ => "state/unknown",
                    },
                    b"",
                );
            }
            assert_eq!(retained.recheck(), Err(Refusal::Layout));
            assert!(matches!(retained.checked_inputs(), Err(Refusal::Layout)));
            probe_outer(dir, true);
        }
    }
    fn init() -> Init {
        Init {
            cluster: [1; 16],
            first_free: NodeId::new(2).unwrap(),
        }
    }
    fn initialized(l: &Layout) {
        l.write("state/cluster-owner.lock", b"");
        drop(StateStore::open(&l.root.path().join("state"), init()).unwrap());
        drop(VoteLogStore::open(&l.root.path().join("state"), [1; 16]).unwrap());
    }
    fn identity(l: &Layout) -> ExpectedIdentity {
        let (bytes, root, expected) = fixture(
            1,
            Origin::Found {
                term: 1,
                leader: 1,
                index: 0,
            },
        );
        l.write("credentials/identity.json", &bytes);
        l.write("credentials/ca.der", &root);
        l.write("credentials/sealed-key", b"opaque-synthetic-seal");
        expected
    }
    fn found(l: &Layout, e: &ExpectedIdentity, time: u64) {
        let mut sm = StateStore::open(&l.root.path().join("state"), init()).unwrap();
        let entry = Entry::Enrollment(Command::Found {
            cluster: [1; 16],
            members: FoundMembers::new([(e.node, e.key)]).unwrap(),
            proposed_at_ms: time,
        });
        let p = e.origin.position();
        sm.apply(p, &entry).unwrap();
        drop(sm);
        let mut log = VoteLogStore::open(&l.root.path().join("state"), [1; 16]).unwrap();
        log.append(&[(p, entry)]).unwrap();
    }
    #[test]
    fn bound_recovery_reentry_refuses_without_filesystem_access() {
        use crate::cluster::bound_open::{BoundAttempt, BoundFailure};

        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 1);
        let plan = l.plan("recover-founder");
        let intent = StartIntent {
            mode: plan.mode,
            cluster: expected.cluster,
            node: expected.node,
            peer: plan.peer.parse().unwrap(),
            enrollment: plan.enrollment.parse().unwrap(),
        };
        let mut owner = Owner::acquire(plan).unwrap();
        let (stores, observation) = prepare_bound(&mut owner, &intent, Some(&expected));
        assert!(matches!(observation, Some(Observation::Recovered { .. })));
        let (accesses, opens) = stores.io_counts();
        assert!(accesses > 0);
        assert_eq!(opens, 4);
        assert_eq!(owner.bound_attempt, BoundAttempt::Complete);

        let (mut retry, observation) = prepare_bound(&mut owner, &intent, Some(&expected));
        assert!(observation.is_none());
        assert!(matches!(retry.take_pair(), Err(BoundFailure::Unavailable)));
        assert_eq!(retry.io_counts(), (0, 0));
        assert_eq!(owner.bound_attempt, BoundAttempt::Complete);
    }
    #[test]
    fn joiner_recovery_uses_redeem_and_excludes_own_anchor() {
        use crate::cluster::enrollment::{JoinCode, KeyDigest};
        use crate::cluster::membership::Membership;
        let l = Layout::new();
        initialized(&l);
        let founder = KeyDigest::of_spki(b"synthetic-founder").unwrap();
        let (record, root, expected) = fixture(
            2,
            Origin::Redeem {
                term: 1,
                leader: 1,
                index: 3,
                anchors: vec![(1, *founder.as_bytes())],
            },
        );
        l.write("credentials/identity.json", &record);
        l.write("credentials/ca.der", &root);
        l.write("credentials/sealed-key", b"opaque");
        let dir = l.root.path().join("state");
        let mut sm = StateStore::open(&dir, init()).unwrap();
        let m = Membership::new(
            vec![[1].into_iter().collect()],
            [(1, "127.0.0.1:7001".into())].into_iter().collect(),
        )
        .unwrap();
        let p = |index| LogPos {
            term: 1,
            leader: 1,
            index,
        };
        let mut entries = vec![
            (
                p(0),
                Entry::Enrollment(Command::Found {
                    cluster: [1; 16],
                    members: FoundMembers::new([(NodeId::new(1).unwrap(), founder)]).unwrap(),
                    proposed_at_ms: 10,
                }),
            ),
            (
                p(1),
                Entry::Membership(MembershipBlob::new(m.encode().unwrap()).unwrap()),
            ),
        ];
        for (p, e) in &entries {
            sm.apply(*p, e).unwrap();
        }
        let anchor = sm.state().voter_anchor(&m).unwrap();
        let code = JoinCode::generate([1; 16], expected.ca, anchor, |b: &mut [u8]| {
            b.fill(8);
            Ok::<_, ()>(())
        })
        .unwrap();
        entries.push((
            p(2),
            Entry::Enrollment(Command::Mint {
                cluster: [1; 16],
                actor: [4; 16],
                request: [5; 16],
                code: code.code_id(),
                verifier: code.verifier(),
                proposed_at_ms: 11,
            }),
        ));
        entries.push((
            p(3),
            Entry::Enrollment(Command::redeem(
                [1; 16],
                [6; 16],
                code.code_id(),
                &code.proof(),
                expected.key,
                anchor,
                12,
            )),
        ));
        for (p, e) in &entries[2..] {
            sm.apply(*p, e).unwrap();
        }
        assert_eq!(sm.state().next_node_id().get(), 3);
        drop(sm);
        let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
        log.append(&entries).unwrap();
        drop(log);
        let before = l.snapshot();
        let prepared = prepare(l.plan("recover-joiner"), Some(&expected), None, false).unwrap();
        match &prepared.observation {
            Observation::Recovered {
                floor: 12,
                membership: Some((position, blob)),
                ..
            } => {
                assert_eq!(*position, p(1));
                assert_eq!(blob.bytes(), m.encode().unwrap());
            }
            _ => panic!("exact recovered membership required"),
        }
        let original = crate::cluster::startup_plan::tests::guards(&prepared.owner);
        probe_outer(&dir, true);
        let mut join_intent = intent(Mode::RecoverJoiner);
        join_intent.node = expected.node;
        let retained = prepared.handoff(join_intent).ok().unwrap();
        retained.checked_inputs().unwrap();
        assert_eq!(
            crate::cluster::startup_plan::tests::guards(&retained.owner),
            original
        );
        assert_eq!(l.snapshot(), before);
        probe_outer(&dir, true);
        drop(retained);
        probe_outer(&dir, false);
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            prepare(l.plan("recover-founder"), Some(&expected), None, false),
            Err(Refusal::Identity)
        ));
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            prepare(l.plan("recover-joiner"), Some(&expected), None, true),
            Err(Refusal::Provenance)
        ));
    }
    #[test]
    fn recovery_accepts_wall_behind_floor_within_skew() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let floor = wall + crate::cluster::enrollment::SKEW_MS;
        found(&l, &expected, floor);
        let prepared = prepare(l.plan("recover-founder"), Some(&expected), None, false).unwrap();
        if let Observation::Recovered {
            floor: f, sample, ..
        } = prepared.observation
        {
            assert_eq!(f, floor);
            assert!(sample + 30000 >= floor);
        } else {
            panic!("recovery required");
        }
    }
    #[test]
    fn fresh_and_verified_untouched_genesis() {
        let l = Layout::new();
        let p = prepare(l.plan("create-founder"), None, None, false).unwrap();
        assert!(matches!(p.observation, Observation::Fresh));
        drop(p);
        let l = Layout::new();
        initialized(&l);
        let before = l.snapshot();
        let p = prepare(l.plan("join"), None, Some([1; 16]), false).unwrap();
        assert!(
            matches!(p.observation, Observation::UntouchedGenesis {cluster} if cluster==[1;16])
        );
        drop(p);
        assert_eq!(l.snapshot(), before);
        let l = Layout::new();
        l.write("state/cluster-owner.lock", b"");
        drop(
            StateStore::open(
                &l.root.path().join("state"),
                Init {
                    first_free: NodeId::new(3).unwrap(),
                    ..init()
                },
            )
            .unwrap(),
        );
        drop(VoteLogStore::open(&l.root.path().join("state"), [1; 16]).unwrap());
        let before = l.snapshot();
        assert!(matches!(
            prepare(l.plan("join"), None, Some([1; 16]), false),
            Err(Refusal::Provenance)
        ));
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn actual_floor_current_binding_reopen_and_owner_drop() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 123456);
        let before = l.snapshot();
        for _ in 0..2 {
            let p = prepare(l.plan("recover-founder"), Some(&expected), None, false).unwrap();
            match &p.observation {
                Observation::Recovered {
                    floor,
                    sample,
                    applied,
                    membership,
                    clock,
                } => {
                    assert_eq!(*floor, 123456);
                    assert!(*sample >= *floor);
                    assert_eq!(*applied, Some(expected.origin.position()));
                    assert!(membership.is_none());
                    assert!(clock.sample_ms().is_ok());
                }
                _ => panic!("expected recovered"),
            }
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .open(l.root.path().join("state/cluster-owner.lock"))
                .unwrap();
            assert!(f.try_lock().is_err());
            drop(p);
            assert!(f.try_lock().is_ok());
            drop(f);
        }
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            prepare(l.plan("recover-founder"), Some(&expected), None, true),
            Err(Refusal::Provenance)
        ));
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn inspection_preserves_ownership_and_layout_refusals() {
        for stem in ["cluster-state", "cluster-log"] {
            let l = Layout::new();
            initialized(&l);
            let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
            let lock = OpenOptions::new()
                .read(true)
                .write(true)
                .open(owner.plan.state.join(format!("{stem}.lock")))
                .unwrap();
            lock.try_lock().unwrap();
            let before = l.snapshot();
            PROBE_CONNECTIONS.with(|n| n.set(0));
            assert!(matches!(
                inspect_pair(&mut owner, [1; 16]),
                Err(Refusal::Ownership)
            ));
            assert_eq!(PROBE_CONNECTIONS.with(|n| n.get()), 0);
            assert_eq!(l.snapshot(), before);
        }
        let l = Layout::new();
        initialized(&l);
        let mut owner = Owner::acquire(l.plan("create-founder")).unwrap();
        l.write("state/cluster-log.sqlite3-wal", b"");
        let before = l.snapshot();
        PROBE_CONNECTIONS.with(|n| n.set(0));
        assert!(matches!(
            inspect_pair(&mut owner, [1; 16]),
            Err(Refusal::Layout)
        ));
        assert_eq!(PROBE_CONNECTIONS.with(|n| n.get()), 0);
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn bound_inspection_accepts_original_zero_sidecars_without_changes() {
        for stem in ["cluster-state", "cluster-log"] {
            for suffix in ["-wal", "-journal"] {
                let l = Layout::new();
                initialized(&l);
                let expected = identity(&l);
                found(&l, &expected, 123456);
                l.write(&format!("state/{stem}.sqlite3{suffix}"), b"");
                let before = l.snapshot();
                let mut owner = Owner::acquire(l.plan("recover-founder")).unwrap();
                let original = guards(&owner);
                MUTABLE_OPENS.with(|n| n.set(0));
                let (state, log) = inspect_pair(&mut owner, [1; 16]).unwrap();
                assert_eq!(state.applied, Some(expected.origin.position()));
                assert!(state.membership.is_none());
                assert_eq!(
                    snapshot_state(&state.snapshot).unwrap().committed_ms(),
                    123456
                );
                validate_preflight(state.applied, &log).unwrap();
                assert_eq!(MUTABLE_OPENS.with(|n| n.get()), 0);
                assert_eq!(guards(&owner), original);
                assert_eq!(l.snapshot(), before);
                probe_outer(&owner.plan.state, true);
                assert_eq!(
                    owner.bound_attempt,
                    crate::cluster::bound_open::BoundAttempt::Complete
                );
                drop(owner);
                assert_eq!(l.snapshot(), before);
            }
        }
    }
    #[test]
    fn zero_wal_prepare_legacy_readback_characterization() {
        for stem in ["cluster-state", "cluster-log"] {
            let l = Layout::new();
            initialized(&l);
            let expected = identity(&l);
            found(&l, &expected, 123456);
            l.write(&format!("state/{stem}.sqlite3-wal"), b"");
            let before = l.snapshot();
            let result = prepare(l.plan("recover-founder"), Some(&expected), None, false);
            // Preserve the legacy mutable-readback observation separately from
            // the read-only inspection assertion above. Both outcomes are recorded.
            println!(
                "zero WAL {stem}: prepare={:?}; bytes_unchanged={}",
                result.as_ref().err(),
                l.snapshot() == before
            );
            let after = l.snapshot();
            let removed: Vec<_> = before
                .iter()
                .filter(|(n, _)| !after.iter().any(|(m, _)| m == n))
                .map(|(n, b)| (n, b.len()))
                .collect();
            let added: Vec<_> = after
                .iter()
                .filter(|(n, _)| !before.iter().any(|(m, _)| m == n))
                .map(|(n, b)| (n, b.len()))
                .collect();
            let changed: Vec<_> = before
                .iter()
                .filter_map(|(n, b)| {
                    after
                        .iter()
                        .find(|(m, _)| m == n)
                        .and_then(|(_, c)| (b != c).then_some((n, b.len(), c.len())))
                })
                .collect();
            println!("removed={removed:?}; added={added:?}; changed={changed:?}");
            if let Ok(prepared) = result {
                assert!(matches!(
                    prepared.observation,
                    Observation::Recovered { floor: 123456, .. }
                ));
            }
        }
    }
    #[test]
    fn existing_invalid_pairs_refuse_unchanged() {
        for variant in [
            "zero",
            "sqlite-zero",
            "empty-schema",
            "partial-schema",
            "extra-schema",
            "image-type",
            "row-type",
            "one-sided",
            "unsupported",
            "image",
            "metadata",
            "row",
            "wal",
            "journal",
            "shm",
            "no-owner",
            "no-state-lock",
            "no-log-lock",
            "no-state-db",
            "truncated-state",
            "truncated-log",
            "sqlite-zero-log",
            "empty-schema-log",
            "partial-schema-log",
            "metadata-type",
            "zero-log",
            "unsupported-log",
            "extra-schema-log",
            "wal-log",
            "journal-log",
            "shm-log",
        ] {
            let l = Layout::new();
            initialized(&l);
            let expected = identity(&l);
            found(&l, &expected, 1);
            let state = l.root.path().join("state/cluster-state.sqlite3");
            let log = l.root.path().join("state/cluster-log.sqlite3");
            match variant {
                "no-log-lock" => std::fs::remove_file(log.with_extension("lock")).unwrap(),
                "no-state-db" => std::fs::remove_file(&state).unwrap(),
                "truncated-state" => l.write("state/cluster-state.sqlite3", b"SQLite format 3\0"),
                "truncated-log" => l.write("state/cluster-log.sqlite3", b"SQLite format 3\0"),
                "sqlite-zero-log" => {
                    std::fs::remove_file(&log).unwrap();
                    l.write("state/cluster-log.sqlite3", b"");
                    Connection::open(&log)
                        .unwrap()
                        .execute_batch("CREATE TABLE unrelated(x); PRAGMA user_version=0;")
                        .unwrap();
                }
                "empty-schema-log" => {
                    std::fs::remove_file(&log).unwrap();
                    l.write("state/cluster-log.sqlite3", b"");
                    Connection::open(&log)
                        .unwrap()
                        .execute_batch("VACUUM;")
                        .unwrap();
                }
                "partial-schema-log" => Connection::open(&log)
                    .unwrap()
                    .execute_batch("DROP TABLE meta;")
                    .unwrap(),
                "metadata-type" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE meta SET blob='text'", [])
                        .unwrap();
                }
                "zero-log" => l.write("state/cluster-log.sqlite3", b""),
                "unsupported-log" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute_batch("PRAGMA user_version=3;")
                        .unwrap();
                }
                "extra-schema-log" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute_batch("CREATE TABLE extra(x);")
                        .unwrap();
                }
                "wal-log" => l.write("state/cluster-log.sqlite3-wal", b"pending"),
                "journal-log" => l.write("state/cluster-log.sqlite3-journal", b"pending"),
                "shm-log" => l.write("state/cluster-log.sqlite3-shm", b""),
                "zero" => l.write("state/cluster-state.sqlite3", b""),
                "sqlite-zero" => {
                    std::fs::remove_file(&state).unwrap();
                    let db = Connection::open(&state).unwrap();
                    db.execute_batch("CREATE TABLE unrelated(x); PRAGMA user_version=0;")
                        .unwrap();
                }
                "one-sided" => std::fs::remove_file(&log).unwrap(),
                "empty-schema" => {
                    std::fs::remove_file(&state).unwrap();
                    Connection::open(&state)
                        .unwrap()
                        .execute_batch("VACUUM;")
                        .unwrap();
                }
                "partial-schema" => Connection::open(&state)
                    .unwrap()
                    .execute_batch("DROP TABLE sm;")
                    .unwrap(),
                "extra-schema" => Connection::open(&state)
                    .unwrap()
                    .execute_batch("CREATE TABLE extra(x);")
                    .unwrap(),
                "image-type" => {
                    Connection::open(&state)
                        .unwrap()
                        .execute("UPDATE sm SET snapshot='text'", [])
                        .unwrap();
                }
                "row-type" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE log SET body='text'", [])
                        .unwrap();
                }
                "unsupported" => Connection::open(&state)
                    .unwrap()
                    .execute_batch("PRAGMA user_version=9")
                    .unwrap(),
                "image" => {
                    Connection::open(&state)
                        .unwrap()
                        .execute("UPDATE sm SET snapshot=zeroblob(40)", [])
                        .unwrap();
                }
                "metadata" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE meta SET blob=zeroblob(40)", [])
                        .unwrap();
                }
                "row" => {
                    Connection::open(&log)
                        .unwrap()
                        .execute("UPDATE log SET body=zeroblob(40)", [])
                        .unwrap();
                }
                "wal" => l.write("state/cluster-state.sqlite3-wal", b"pending"),
                "journal" => l.write("state/cluster-state.sqlite3-journal", b"pending"),
                "shm" => l.write("state/cluster-state.sqlite3-shm", b""),
                "no-owner" => {
                    std::fs::remove_file(l.root.path().join("state/cluster-owner.lock")).unwrap()
                }
                _ => std::fs::remove_file(l.root.path().join("state/cluster-state.lock")).unwrap(),
            }
            let before = l.snapshot();
            assert!(
                prepare(l.plan("recover-founder"), Some(&expected), None, false).is_err(),
                "{variant}"
            );
            assert_eq!(l.snapshot(), before, "{variant}");
        }
    }
    #[test]
    fn current_store_log_conformance_and_independent_corruption() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 17);
        let dir = l.root.path().join("state");
        assert_eq!(
            inspect_log(&dir, [1; 16]).unwrap().rows,
            vec![expected.origin.position()]
        );
        let db = Connection::open(dir.join("cluster-log.sqlite3")).unwrap();
        let original: Vec<u8> = db
            .query_row("SELECT blob FROM meta", [], |r| r.get(0))
            .unwrap();
        for offset in [0, 8, 26] {
            let mut bad = original.clone();
            bad[offset] ^= 1;
            let n = bad.len() - 32;
            let h = Sha256::digest(&bad[..n]);
            bad[n..].copy_from_slice(&h);
            db.execute("UPDATE meta SET blob=?1", [bad]).unwrap();
            assert!(inspect_log(&dir, [1; 16]).is_err());
        }
        db.execute("UPDATE meta SET blob=?1", [original]).unwrap();
        assert!(inspect_log(&dir, [1; 16]).is_ok());
        db.execute("UPDATE log SET idx=2", []).unwrap();
        assert!(inspect_log(&dir, [1; 16]).is_err());
    }
    #[test]
    fn missing_artifact_wrong_binding_and_behind_floor_unchanged() {
        for variant in ["missing", "binding", "floor"] {
            let l = Layout::new();
            initialized(&l);
            let mut expected = identity(&l);
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            found(
                &l,
                &expected,
                if variant == "floor" { wall + 60000 } else { 1 },
            );
            if variant == "missing" {
                std::fs::remove_file(l.root.path().join("credentials/sealed-key")).unwrap();
            }
            if variant == "binding" {
                expected.key = crate::cluster::enrollment::KeyDigest::of_spki(b"foreign").unwrap();
            }
            let before = l.snapshot();
            assert!(prepare(l.plan("recover-founder"), Some(&expected), None, false).is_err());
            assert_eq!(l.snapshot(), before);
        }
    }
    #[test]
    fn unsupported_schema_refuses_without_evaluating_check() {
        for stem in ["cluster-state", "cluster-log"] {
            let l = Layout::new();
            initialized(&l);
            let expected = identity(&l);
            found(&l, &expected, 17);
            let count = Arc::new(AtomicUsize::new(0));
            let db = Connection::open(l.root.path().join(format!("state/{stem}.sqlite3"))).unwrap();
            register_schema_probe(&db, count.clone());
            db.execute_batch(
                "CREATE TABLE extra(x CHECK(arx_schema_probe(x))); INSERT INTO extra VALUES(1);",
            )
            .unwrap();
            count.store(0, Ordering::SeqCst);
            let checked: String = db
                .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
                .unwrap();
            assert_eq!(checked, "ok");
            assert!(
                count.load(Ordering::SeqCst) > 0,
                "fixture CHECK must be observable"
            );
            drop(db);
            count.store(0, Ordering::SeqCst);
            let before = l.snapshot();
            SCHEMA_PROBE.with(|slot| *slot.borrow_mut() = Some(count.clone()));
            let _reset = ResetProbe;
            let mut owner = Owner::acquire(l.plan("recover-founder")).unwrap();
            PROBE_CONNECTIONS.with(|n| n.set(0));
            INTEGRITY_CALLS.with(|n| n.set(0));
            let result = inspect_pair(&mut owner, [1; 16]);
            assert!(matches!(result, Err(Refusal::Store)));
            assert_eq!(
                PROBE_CONNECTIONS.with(|n| n.get()),
                if stem == "cluster-state" { 1 } else { 2 },
                "must reach read-only SQLite inspection"
            );
            assert_eq!(
                INTEGRITY_CALLS.with(|n| n.get()),
                0,
                "unsupported schema reached integrity traversal"
            );
            assert_eq!(l.snapshot(), before);
            assert_eq!(
                count.load(Ordering::SeqCst),
                0,
                "unsupported CHECK executed before schema rejection"
            );
        }
    }
    #[test]
    fn oversized_applied_index_refuses_before_mutable_open() {
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 17);
        let db = Connection::open(l.root.path().join("state/cluster-state.sqlite3")).unwrap();
        let mut blob: Vec<u8> = db
            .query_row("SELECT snapshot FROM sm", [], |r| r.get(0))
            .unwrap();
        assert_eq!(blob[10], 1);
        blob[27..35].copy_from_slice(&(i64::MAX as u64 + 1).to_be_bytes());
        let n = blob.len() - 32;
        let hash = Sha256::digest(&blob[..n]);
        blob[n..].copy_from_slice(&hash);
        assert_eq!(
            StateStore::inspect_snapshot(&blob, [1; 16])
                .unwrap()
                .0
                .unwrap()
                .index,
            i64::MAX as u64 + 1
        );
        db.execute("UPDATE sm SET snapshot=?1", [blob]).unwrap();
        drop(db);
        let before = l.snapshot();
        MUTABLE_OPENS.with(|n| n.set(0));
        assert!(matches!(
            prepare(l.plan("recover-founder"), Some(&expected), None, false),
            Err(Refusal::Store)
        ));
        assert_eq!(l.snapshot(), before);
        assert_eq!(
            MUTABLE_OPENS.with(|n| n.get()),
            0,
            "invalid applied index reached mutable store opener"
        );
    }
    type PhaseHook = Box<dyn FnMut(&'static str, &Owner)>;
    thread_local! {
        static PHASE: std::cell::RefCell<Option<PhaseHook>> = const { std::cell::RefCell::new(None) };
        static SCANNED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    pub(super) fn phase(stage: &'static str, owner: &Owner) {
        PHASE.with(|h| {
            if let Some(h) = h.borrow_mut().as_mut() {
                h(stage, owner);
            }
        });
    }
    pub(super) fn scan_row() {
        SCANNED.with(|n| n.set(n.get() + 1));
    }
    struct ResetPhase;
    impl Drop for ResetPhase {
        fn drop(&mut self) {
            PHASE.with(|h| *h.borrow_mut() = None);
        }
    }
    fn pos(index: u64) -> LogPos {
        LogPos {
            term: 1,
            leader: 1,
            index,
        }
    }
    // Independent fixture encoder, separate from either validator's encoding.
    fn checksummed(mut payload: Vec<u8>) -> Vec<u8> {
        payload.extend(Sha256::digest(&payload));
        payload
    }
    fn blank_row(index: u64) -> Vec<u8> {
        let mut b = b"ARXLE2\0\0".to_vec();
        for n in [1u64, 1, index] {
            b.extend(n.to_be_bytes());
        }
        b.push(0);
        checksummed(b)
    }
    fn assert_log_readback(l: &Layout, vote: Option<crate::cluster::log_store::StoredVote>) {
        let dir = l.root.path().join("state");
        let before = l.snapshot();
        let inspected = inspect_log(&dir, [1; 16]).unwrap();
        let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
        assert_eq!(log.vote().unwrap(), vote);
        assert_eq!(inspected.has_vote, vote.is_some());
        let state = log.log_state().unwrap();
        assert_eq!(inspected.purged, state.purged);
        assert_eq!(inspected.last, state.last);
        let rows = log.entries(0, i64::MAX as u64 + 1, MAX_LIVE).unwrap();
        assert_eq!(
            inspected.rows,
            rows.iter().map(|(p, _)| *p).collect::<Vec<_>>()
        );
        let sm = StateStore::open(&dir, init()).unwrap();
        assert_eq!(
            validate_preflight(sm.applied(), &inspected).is_ok(),
            preflight(&mut log, &sm).is_ok()
        );
        drop(sm);
        drop(log);
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn validator_current_vote_tail_purge_and_preflight_conformance() {
        use crate::cluster::log_store::StoredVote;
        let l = Layout::new();
        initialized(&l);
        let expected = identity(&l);
        found(&l, &expected, 17);
        let dir = l.root.path().join("state");
        let vote = StoredVote {
            term: 7,
            node: 9,
            committed: true,
        };
        {
            let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
            log.save_vote(vote).unwrap();
            log.append(&[(pos(1), Entry::Blank), (pos(2), Entry::Blank)])
                .unwrap();
        }
        assert_log_readback(&l, Some(vote));
        // Syntactically valid purge beyond applied: both preflights must refuse.
        {
            let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
            log.purge(pos(1)).unwrap();
        }
        assert_log_readback(&l, Some(vote));
        assert!(validate_preflight(Some(pos(0)), &inspect_log(&dir, [1; 16]).unwrap()).is_err());
        {
            let mut sm = StateStore::open(&dir, init()).unwrap();
            sm.apply(pos(1), &Entry::Blank).unwrap();
            sm.apply(pos(2), &Entry::Blank).unwrap();
        }
        {
            let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
            log.purge(pos(2)).unwrap();
        }
        assert_log_readback(&l, Some(vote));
        assert!(inspect_log(&dir, [1; 16]).unwrap().rows.is_empty());
        assert!(validate_preflight(Some(pos(2)), &inspect_log(&dir, [1; 16]).unwrap()).is_ok());
    }
    #[test]
    fn validator_512_boundary_and_513_overflow_sentinel() {
        let l = Layout::new();
        initialized(&l);
        let dir = l.root.path().join("state");
        {
            let mut log = VoteLogStore::open(&dir, [1; 16]).unwrap();
            for start in (0..512).step_by(64) {
                let entries = (start..start + 64)
                    .map(|i| (pos(i), Entry::Blank))
                    .collect::<Vec<_>>();
                log.append(&entries).unwrap();
            }
            let before = l.snapshot();
            assert!(matches!(
                log.append(&[(pos(512), Entry::Blank)]),
                Err(StoreError::Full)
            ));
            assert_eq!(l.snapshot(), before);
            assert_eq!(log.live().unwrap(), 512);
        }
        assert_log_readback(&l, None);
        // Keep valid 512-row metadata: the additional independently encoded row
        // must hit the sentinel before tail-equality validation.
        let db = Connection::open(dir.join("cluster-log.sqlite3")).unwrap();
        db.execute("INSERT INTO log VALUES(512,?1)", [blank_row(512)])
            .unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM log", [], |r| r.get::<_, u64>(0))
                .unwrap(),
            513
        );
        drop(db);
        let before = l.snapshot();
        SCANNED.with(|n| n.set(0));
        assert!(matches!(inspect_log(&dir, [1; 16]), Err(Refusal::Store)));
        assert_eq!(SCANNED.with(|n| n.get()), 513, "sentinel must be reached");
        assert_eq!(l.snapshot(), before);
        assert!(matches!(
            VoteLogStore::open(&dir, [1; 16]),
            Err(StoreError::Corrupt)
        ));
        assert_eq!(l.snapshot(), before);
    }
    #[test]
    fn validator_independent_checksummed_grammar_and_extra_rows_refuse_unchanged() {
        for variant in [
            "row-magic",
            "row-tag",
            "row-trailing",
            "meta-trailing",
            "vote-flag",
            "negative",
            "extra-row",
            "extra-meta",
        ] {
            let l = Layout::new();
            initialized(&l);
            let expected = identity(&l);
            found(&l, &expected, 17);
            let dir = l.root.path().join("state");
            let db = Connection::open(dir.join("cluster-log.sqlite3")).unwrap();
            if variant == "negative" || variant == "extra-meta" {
                // Preserve exact schema; inject CHECK violations. Raw validator
                // call below bypasses quick_check and proves its own refusal.
                db.execute_batch("PRAGMA ignore_check_constraints=ON")
                    .unwrap();
            }
            match variant {
                "negative" => {
                    db.execute("INSERT INTO log VALUES(-1,?1)", [blank_row(0)])
                        .unwrap();
                }
                "extra-row" => {
                    db.execute("INSERT INTO log VALUES(1,?1)", [blank_row(1)])
                        .unwrap();
                }
                "extra-meta" => {
                    db.execute("INSERT INTO meta SELECT 2,blob FROM meta", [])
                        .unwrap();
                }
                _ => {
                    let meta = variant.starts_with("meta") || variant == "vote-flag";
                    let mut b: Vec<u8> = db
                        .query_row(
                            if meta {
                                "SELECT blob FROM meta"
                            } else {
                                "SELECT body FROM log"
                            },
                            [],
                            |r| r.get(0),
                        )
                        .unwrap();
                    b.truncate(b.len() - 32);
                    match variant {
                        "row-magic" => b[0] ^= 1,
                        "row-tag" => b[32] = 255,
                        "vote-flag" => b[26] = 2,
                        _ => b.push(0),
                    }
                    let b = checksummed(b);
                    assert!(
                        reader(&b, if meta { 256 } else { 4163 }).is_ok(),
                        "valid checksum: {variant}"
                    );
                    db.execute(
                        if meta {
                            "UPDATE meta SET blob=?1"
                        } else {
                            "UPDATE log SET body=?1"
                        },
                        [b],
                    )
                    .unwrap();
                }
            }
            drop(db);
            let before = l.snapshot();
            let db = Connection::open_with_flags(
                dir.join("cluster-log.sqlite3"),
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            assert!(
                matches!(
                    inspect_log_connection(&db, [1; 16]),
                    Err(StoreError::Corrupt)
                ),
                "{variant}"
            );
            drop(db);
            assert!(
                matches!(
                    prepare(l.plan("recover-founder"), Some(&expected), None, false),
                    Err(Refusal::Store)
                ),
                "{variant}"
            );
            assert_eq!(l.snapshot(), before, "{variant}: prepare refusal");
            assert!(
                matches!(VoteLogStore::open(&dir, [1; 16]), Err(StoreError::Corrupt)),
                "{variant}"
            );
            assert_eq!(l.snapshot(), before, "{variant}: current opener refusal");
            eprintln!(
                "case={variant} checksum-valid/raw-validator=Corrupt/prepare=Store/current-store=Corrupt unchanged"
            );
        }
    }
    #[test]
    fn retained_identity_interstage_replacement_observations() {
        use std::{cell::RefCell, rc::Rc};
        for stage in ["before-inspection", "before-open", "before-publication"] {
            let l = Rc::new(Layout::new());
            initialized(&l);
            let expected = identity(&l);
            found(&l, &expected, 17);
            let before = l.snapshot();
            let observation = Rc::new(RefCell::new(None));
            let visits = Rc::new(RefCell::new(Vec::new()));
            let hook_l = l.clone();
            let hook_observation = observation.clone();
            let hook_visits = visits.clone();
            PHASE.with(|h| {
                *h.borrow_mut() = Some(Box::new(move |actual, owner| {
                    hook_visits.borrow_mut().push(actual);
                    if actual != stage {
                        return;
                    }
                    let path = owner.plan.state.join("cluster-log.sqlite3");
                    let bytes = std::fs::read(&path).unwrap();
                    // Outside state: unexpected directory entries cannot explain refusal.
                    let old = hook_l.root.path().join("displaced.sqlite3");
                    let rename = std::fs::rename(&path, &old);
                    let replaced = rename.is_ok();
                    if replaced {
                        hook_l.write("state/cluster-log.sqlite3", &bytes);
                        assert_eq!(std::fs::read(&old).unwrap(), bytes);
                    } else {
                        eprintln!(
                            "stage={stage} OS-denied rename: {:?}",
                            rename.unwrap_err().raw_os_error()
                        );
                    }
                    let snapshot = hook_l.snapshot();
                    assert_eq!(snapshot, before, "identical entries and bytes after hook");
                    probe_outer(&owner.plan.state, true);
                    *hook_observation.borrow_mut() = Some((replaced, snapshot));
                }))
            });
            let reset = ResetPhase;
            let count = Arc::new(AtomicUsize::new(0));
            SCHEMA_PROBE.with(|slot| *slot.borrow_mut() = Some(count.clone()));
            let _reset_probe = ResetProbe;
            PROBE_CONNECTIONS.with(|n| n.set(0));
            INTEGRITY_CALLS.with(|n| n.set(0));
            SCANNED.with(|n| n.set(0));
            MUTABLE_OPENS.with(|n| n.set(0));
            let result = prepare(l.plan("recover-founder"), Some(&expected), None, false);
            drop(reset);
            let (replaced, after_hook) = observation
                .borrow_mut()
                .take()
                .expect("requested stage reached");
            assert_eq!(
                l.snapshot(),
                after_hook,
                "{stage}: call changed files after hook"
            );
            #[cfg(unix)]
            assert!(
                replaced,
                "Unix replacement required to qualify identity oracle"
            );
            if replaced {
                assert!(
                    matches!(result, Err(Refusal::Layout)),
                    "{stage}: identity refusal required"
                );
                // Bound acquisition rechecks original guards before SQLite opens:
                // a before-inspection replacement refuses at that same stage.
                let prefix: &[&str] = match stage {
                    "before-inspection" => &["before-inspection"],
                    "before-open" => &["before-inspection", "before-open"],
                    _ => &["before-inspection", "before-open", "before-publication"],
                };
                assert_eq!(visits.borrow().as_slice(), prefix, "{stage}");
                eprintln!(
                    "stage={stage} actual-replacement=true result=Layout after-hook=after-call"
                );
            } else {
                assert!(
                    result.is_ok(),
                    "{stage}: denied mutation preserves valid preparation"
                );
                assert_eq!(
                    visits.borrow().as_slice(),
                    &["before-inspection", "before-open", "before-publication"]
                );
                eprintln!(
                    "stage={stage} actual-replacement=false result=Ok OS-prevention-only after-hook=after-call"
                );
            }
            // The enabled schema witness precedes application PRAGMAs; the
            // healthy/later-stage counts qualify the early zero-query oracle.
            let inspected = usize::from(!replaced || stage != "before-inspection");
            assert_eq!(
                PROBE_CONNECTIONS.with(|n| n.get()),
                inspected * 2,
                "{stage}"
            );
            assert_eq!(INTEGRITY_CALLS.with(|n| n.get()), inspected * 2, "{stage}");
            assert_eq!(SCANNED.with(|n| n.get()), inspected, "{stage}");
            assert_eq!(
                MUTABLE_OPENS.with(|n| n.get()),
                usize::from(!replaced || stage == "before-publication"),
                "{stage}"
            );
            assert_eq!(count.load(Ordering::SeqCst), 0, "{stage}: CHECK evaluated");
            drop(result);
            let f = OpenOptions::new()
                .read(true)
                .write(true)
                .open(l.root.path().join("state/cluster-owner.lock"))
                .unwrap();
            f.try_lock().unwrap();
        }
    }
}
