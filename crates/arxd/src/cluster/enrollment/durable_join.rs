//! A private, process-owned join before any learner transport or replication.
#![allow(dead_code)] // The owner-local caller is deferred to step 13.

use crate::cluster::{
    enrollment::{
        MAX_TIME_MS, RequestId,
        session::SocketLimits,
        transfer::{self, LocalRoute, Transfer},
    },
    issuance_publication as publication,
    join_receipt::{ExpectedIdentity, Origin},
    pki::{
        csr::ProvenCsr,
        custody::LearnerCustody,
        preproof::{self, Completed, JoinBinding},
    },
    startup_plan::{Mode, Owner, Plan},
};
use rustls::time_provider::TimeProvider;
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) enum Status {
    Joining,
    Refused,
    Incomplete,
    Unconfirmed,
    Published,
}

// Item 9 will need to consume this inside the owning job.
struct LiveMember {
    expected: ExpectedIdentity,
    leaf: Vec<u8>,
}

pub(in crate::cluster) struct PublishedJoin {
    pub(in crate::cluster) owner: Owner,
    pub(in crate::cluster) custody: LearnerCustody,
    pub(in crate::cluster) expected: ExpectedIdentity,
    pub(in crate::cluster) leaf: Vec<u8>,
    pub(in crate::cluster) time: Arc<dyn TimeProvider>,
}

#[derive(Default)]
struct Retained {
    owner: Option<Owner>,
    custody: Option<LearnerCustody>,
    #[cfg(test)]
    original_csr: Vec<u8>,
    live: Option<LiveMember>,
}

struct Job {
    retained: Mutex<Retained>,
    status: watch::Sender<Status>,
    request: RequestId,
    time: Arc<dyn TimeProvider>,
}

struct Staged {
    binding: JoinBinding,
    key: crate::cluster::enrollment::KeyDigest,
    csr: Vec<u8>,
}

pub(in crate::cluster) struct Observer(watch::Receiver<Status>);

impl Observer {
    pub(in crate::cluster) async fn terminal(&mut self) -> Status {
        let status = self.0.wait_for(|s| *s != Status::Joining).await;
        status.map_or(Status::Unconfirmed, |s| *s)
    }
}

pub(in crate::cluster) struct Destination {
    pub plan: Plan,
    pub wrap: PathBuf,
    pub staging: PathBuf,
}

struct Intent {
    destination: Destination,
    transfer: Transfer,
    time: Arc<dyn TimeProvider>,
}

impl Intent {
    fn parse(
        destination: Destination,
        paste: &str,
        routes: &[LocalRoute],
        time: Arc<dyn TimeProvider>,
    ) -> Result<Self, Status> {
        if destination.plan.mode != Mode::Join {
            return Err(Status::Refused);
        }
        let transfer = transfer::parse(paste, routes).map_err(|_| Status::Refused)?;
        Ok(Self {
            destination,
            transfer,
            time,
        })
    }
}

pub(super) fn start(
    destination: Destination,
    paste: &str,
    routes: &[LocalRoute],
    time: Arc<dyn TimeProvider>,
) -> Result<Observer, Status> {
    start_retained(destination, paste, routes, time).map(|(_, observer)| observer)
}

pub(in crate::cluster) struct RetainedJoin(&'static Job);

pub(in crate::cluster) fn start_retained(
    destination: Destination,
    paste: &str,
    routes: &[LocalRoute],
    time: Arc<dyn TimeProvider>,
) -> Result<(RetainedJoin, Observer), Status> {
    let intent = Intent::parse(destination, paste, routes, time)?;
    launch(intent).map(|(job, observer)| (RetainedJoin(job), observer))
}

impl RetainedJoin {
    pub(in crate::cluster) fn consume(self) -> Result<PublishedJoin, Status> {
        let mut retained = self.0.retained();
        if *self.0.status.borrow() != Status::Published {
            return Err(Status::Refused);
        }
        let member = retained.live.take().ok_or(Status::Refused)?;
        Ok(PublishedJoin {
            expected: member.expected,
            leaf: member.leaf,
            owner: retained.owner.take().unwrap(),
            custody: retained.custody.take().unwrap(),
            time: self.0.time.clone(),
        })
    }
}

fn launch(intent: Intent) -> Result<(&'static Job, Observer), Status> {
    let mut request = [0; 16];
    getrandom::fill(&mut request).map_err(|_| Status::Refused)?;
    if request == [0; 16] {
        return Err(Status::Refused);
    }
    let (status, receiver) = watch::channel(Status::Joining);
    // Keep the original lock, installation and terminal state independent
    // of any observer, including after an unwind at a publication adapter.
    let job: &'static Job = Box::leak(Box::new(Job {
        retained: Mutex::default(),
        status,
        request,
        time: intent.time.clone(),
    }));
    tokio::spawn(async move {
        let status = match tokio::spawn(job.run(intent)).await {
            Ok(Ok(())) => Status::Published,
            Ok(Err(first)) => first,
            Err(_) => Status::Unconfirmed,
        };
        job.status.send_replace(status);
    });
    Ok((job, Observer(receiver)))
}

impl Job {
    fn retained(&self) -> std::sync::MutexGuard<'_, Retained> {
        self.retained.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn run(&self, intent: Intent) -> Result<(), Status> {
        {
            let mut retained = self.retained();
            retained.owner =
                Some(Owner::acquire(intent.destination.plan).map_err(|_| Status::Refused)?);
            retained
                .owner
                .as_ref()
                .unwrap()
                .validate_fresh_join()
                .map_err(|_| Status::Refused)?;
        }
        let Transfer {
            endpoint,
            code,
            root,
        } = &intent.transfer;
        let connection = preproof::anchor(
            *endpoint,
            code,
            root,
            intent.time.clone(),
            SocketLimits::default(),
        )
        .await
        .map_err(|_| Status::Refused)?;
        let staged = self.stage(&intent.destination.wrap, &intent.destination.staging, code)?;
        let completed = connection
            .redeem(self.request, &staged.csr)
            .await
            .map_err(|error| match error {
                preproof::ExchangeError::Refused => Status::Incomplete,
                preproof::ExchangeError::PossibleEffect => Status::Unconfirmed,
            })?;
        let now = intent
            .time
            .current_time()
            .and_then(|t| t.as_secs().checked_mul(1000))
            .filter(|t| *t <= MAX_TIME_MS)
            .ok_or(Status::Incomplete)?;
        self.publish(staged, completed, root, code.anchor(), now)
    }

    fn stage(
        &self,
        wrap: &std::path::Path,
        staging: &std::path::Path,
        code: &crate::cluster::enrollment::JoinCode,
    ) -> Result<Staged, Status> {
        let mut retained = self.retained();
        let Retained { owner, custody, .. } = &mut *retained;
        let owner = owner.as_mut().unwrap();
        let cluster = code.cluster();
        let root = code.ca_fingerprint();
        *custody = Some(LearnerCustody::create(owner, wrap, staging).map_err(|_| Status::Refused)?);
        let csr = custody
            .as_mut()
            .unwrap()
            .seal(cluster, root)
            .map_err(|_| Status::Refused)?;
        custody
            .as_ref()
            .unwrap()
            .member()
            .map_err(|_| Status::Refused)?;
        let key = ProvenCsr::parse(&csr)
            .map_err(|_| Status::Refused)?
            .digest();
        publication::reserve(owner).map_err(publication_failure)?;
        #[cfg(test)]
        {
            retained.original_csr = csr.clone();
        }
        Ok(Staged {
            binding: JoinBinding {
                cluster,
                code: code.code_id(),
                root,
                request: self.request,
                csr: Sha256::digest(&csr).into(),
            },
            key,
            csr,
        })
    }

    fn publish(
        &self,
        staged: Staged,
        completed: Completed,
        root: &[u8],
        anchor: crate::cluster::enrollment::JoinAnchor,
        now: u64,
    ) -> Result<(), Status> {
        let completed = completed
            .for_join(&staged.binding)
            .map_err(|_| Status::Incomplete)?;
        let (node, pos, leaf) = completed.reply().parts();
        let cluster = staged.binding.cluster;
        let voters = completed.voters();
        if voters.anchor(cluster) != anchor
            || node.get() < 2
            || pos.term == 0
            || pos.index == 0
            || pos.index > i64::MAX as u64
        {
            return Err(Status::Incomplete);
        }
        let canonical = voters.encode();
        let anchors = canonical[1..]
            .chunks_exact(40)
            .map(|entry| {
                (
                    u64::from_be_bytes(entry[..8].try_into().unwrap()),
                    entry[8..].try_into().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        if !anchors.iter().any(|(n, _)| *n == pos.leader) {
            return Err(Status::Incomplete);
        }
        let expected = ExpectedIdentity {
            cluster,
            node,
            key: staged.key,
            ca: staged.binding.root,
            origin: Origin::Redeem {
                term: pos.term,
                leader: pos.leader,
                index: pos.index,
                anchors,
            },
        };
        self.store_member(expected, root, leaf, now)
    }

    fn store_member(
        &self,
        expected: ExpectedIdentity,
        root: &[u8],
        leaf: &[u8],
        now: u64,
    ) -> Result<(), Status> {
        let mut retained = self.retained();
        let Retained {
            owner,
            custody,
            live,
            ..
        } = &mut *retained;
        let owner = owner.as_mut().unwrap();
        let custody = custody.as_ref().unwrap();
        let member = custody.member().map_err(|_| Status::Incomplete)?;
        custody
            .match_leaf(expected.node, root, leaf, now)
            .map_err(|_| Status::Incomplete)?;
        publication::store(
            owner,
            Some(&expected),
            custody.installation(),
            root,
            leaf,
            &member,
            Some(now),
        )
        .map_err(publication_failure)?;
        publication::check_retained(
            owner,
            Some(&expected),
            custody.installation(),
            &member,
            Some(now),
        )
        .map_err(publication_failure)?;
        *live = Some(LiveMember {
            expected,
            leaf: leaf.to_vec(),
        });
        Ok(())
    }
}

fn publication_failure(e: publication::Error) -> Status {
    match e {
        publication::Error::Context => Status::Incomplete,
        _ => Status::Unconfirmed,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::cluster) use tests::exercise_retained_issuer;
