use super::super::{
    log_store::{MAX_LIVE, VoteLogStore},
    state_store::StateStore,
};
use super::*;
use futures_util::{Stream, StreamExt};
use openraft::{
    RaftLogReader, RaftSnapshotBuilder, Snapshot, SnapshotMeta, StoredMembership,
    storage::{EntryResponder, IOFlushed, LogState, RaftLogStorage, RaftStateMachine},
};
use std::{
    fmt::Debug,
    io,
    ops::{Bound, RangeBounds},
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub(super) struct LogAdapter(pub Arc<Mutex<VoteLogStore>>);
#[derive(Clone)]
pub(super) struct SmAdapter(pub Arc<Mutex<StateStore>>);
pub(super) struct SnapBuilder(SmAdapter);
type Snap = openraft::type_config::alias::SnapshotOf<Arx, Vec<u8>>;
pub(super) fn err(e: StoreError) -> io::Error {
    io::Error::other(e)
}
pub(super) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, StoreError> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| err(StoreError::Poisoned))?
        .map_err(err)
}
impl LogAdapter {
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut VoteLogStore) -> Result<T, StoreError> + Send + 'static,
    ) -> io::Result<T> {
        let s = self.0.clone();
        blocking(move || f(&mut *s.lock().map_err(|_| StoreError::Poisoned)?)).await
    }
}
impl RaftLogReader<Arx> for LogAdapter {
    async fn read_vote(
        &mut self,
    ) -> io::Result<Option<Vote<openraft::impls::leader_id_adv::LeaderId<u64, u64>>>> {
        self.run(|s| Ok(s.vote()?.map(vote))).await
    }
    async fn try_get_log_entries<RB: RangeBounds<u64> + Clone + Debug + Send>(
        &mut self,
        r: RB,
    ) -> io::Result<Vec<openraft::type_config::alias::EntryOf<Arx>>> {
        let start = match r.start_bound() {
            Bound::Included(v) => *v,
            Bound::Excluded(v) => v.saturating_add(1),
            Bound::Unbounded => 0,
        };
        let end = match r.end_bound() {
            Bound::Included(v) => v.saturating_add(1),
            Bound::Excluded(v) => *v,
            Bound::Unbounded => u64::MAX,
        };
        self.run(move |s| {
            s.entries(start, end, MAX_LIVE)?
                .into_iter()
                .map(from_stored)
                .collect()
        })
        .await
    }
}
#[cfg(test)]
pub(super) static PANIC_APPEND: Mutex<Option<usize>> = Mutex::new(None);
#[cfg(test)]
pub(super) struct AppendCompletionPause {
    pub store: usize,
    pub entered: tokio::sync::oneshot::Sender<()>,
    pub release: std::sync::mpsc::Receiver<()>,
}
#[cfg(test)]
pub(super) static APPEND_COMPLETION_PAUSE: Mutex<Option<AppendCompletionPause>> = Mutex::new(None);
#[cfg(test)]
fn pause_append_completion(store: usize) {
    let pause = {
        let mut hook = APPEND_COMPLETION_PAUSE.lock().unwrap();
        if hook.as_ref().is_some_and(|p| p.store == store) {
            hook.take()
        } else {
            None
        }
    };
    if let Some(pause) = pause {
        let _ = pause.entered.send(());
        let _ = pause.release.recv();
    }
}
struct Completion(Option<IOFlushed<Arx>>);
impl Completion {
    fn new(c: IOFlushed<Arx>) -> Self {
        Self(Some(c))
    }
    fn finish(mut self, r: &io::Result<()>) {
        if let Some(c) = self.0.take() {
            c.io_completed(match r {
                Ok(()) => Ok(()),
                Err(_) => Err(err(StoreError::Poisoned)),
            });
        }
    }
}
impl Drop for Completion {
    fn drop(&mut self) {
        if let Some(c) = self.0.take() {
            c.io_completed(Err(err(StoreError::Poisoned)));
        }
    }
}
impl RaftLogStorage<Arx> for LogAdapter {
    type LogReader = Self;
    async fn get_log_reader(&mut self) -> Self {
        self.clone()
    }
    async fn get_log_state(&mut self) -> io::Result<LogState<Arx>> {
        self.run(|s| {
            let l = s.log_state()?;
            Ok(LogState {
                last_purged_log_id: l.purged.map(id),
                last_log_id: l.last.map(id),
            })
        })
        .await
    }
    async fn save_vote(
        &mut self,
        v: &Vote<openraft::impls::leader_id_adv::LeaderId<u64, u64>>,
    ) -> io::Result<()> {
        let v = stored_vote(*v);
        self.run(move |s| s.save_vote(v)).await
    }
    fn append<I>(
        &mut self,
        entries: I,
        callback: IOFlushed<Arx>,
    ) -> impl std::future::Future<Output = io::Result<()>> + Send
    where
        I: IntoIterator<Item = openraft::type_config::alias::EntryOf<Arx>> + Send,
        I::IntoIter: Send,
    {
        let guard = Completion::new(callback);
        let batch: Vec<_> = entries.into_iter().take(65).collect();
        let converted = if batch.len() > 64 {
            Err(StoreError::TooLarge)
        } else {
            batch
                .into_iter()
                .map(to_stored)
                .collect::<Result<Vec<_>, _>>()
        };
        let prepared = match converted {
            Ok(b) => Ok((b, guard)),
            Err(e) => {
                let r = Err(err(e));
                guard.finish(&r);
                Err(err(e))
            }
        };
        let store = self.0.clone();
        async move {
            match prepared {
                Err(e) => Err(e),
                Ok((b, guard)) => {
                    blocking(move || {
                        let r = store
                            .lock()
                            .map_err(|_| StoreError::Poisoned)
                            .and_then(|mut s| {
                                #[cfg(test)]
                                {
                                    let mut hook = PANIC_APPEND.lock().unwrap();
                                    if *hook == Some(Arc::as_ptr(&store) as usize) {
                                        *hook = None;
                                        drop(hook);
                                        panic!("injected blocking append panic");
                                    }
                                }
                                s.append(&b)
                            });
                        #[cfg(test)]
                        let store_id = Arc::as_ptr(&store) as usize;
                        // A cancelled caller can reopen as soon as it receives the
                        // callback. Release this worker's ownership (including the
                        // file lock if it is last) before publishing completion.
                        drop(store);
                        guard.finish(&r.as_ref().map(|_| ()).map_err(|e| err(*e)));
                        #[cfg(test)]
                        pause_append_completion(store_id);
                        r
                    })
                    .await
                }
            }
        }
    }
    async fn truncate_after(&mut self, p: Option<Id>) -> io::Result<()> {
        self.run(move |s| s.truncate_after(p.map(pos))).await
    }
    async fn purge(&mut self, p: Id) -> io::Result<()> {
        self.run(move |s| s.purge(pos(p))).await
    }
}
fn membership(
    m: Option<&(LogPos, MembershipBlob)>,
) -> Result<openraft::type_config::alias::StoredMembershipOf<Arx>, StoreError> {
    Ok(match m {
        None => StoredMembership::default(),
        Some((p, m)) => StoredMembership::new(Some(id(*p)), decode(m)?),
    })
}
impl SmAdapter {
    async fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut StateStore) -> Result<T, StoreError> + Send + 'static,
    ) -> io::Result<T> {
        let s = self.0.clone();
        blocking(move || f(&mut *s.lock().map_err(|_| StoreError::Poisoned)?)).await
    }
    async fn snapshot(&self) -> io::Result<Snap> {
        self.run(|s| {
            Ok(Snapshot {
                meta: SnapshotMeta {
                    last_log_id: s.applied().map(id),
                    last_membership: membership(s.membership())?,
                },
                snapshot: s.snapshot(),
            })
        })
        .await
    }
}
impl RaftSnapshotBuilder<Arx> for SnapBuilder {
    type SnapshotData = Vec<u8>;
    async fn build_snapshot(&mut self) -> io::Result<Snap> {
        self.0.snapshot().await
    }
}
#[cfg(test)]
pub(super) struct ApplyPause {
    pub store: usize,
    pub entered: tokio::sync::oneshot::Sender<()>,
    pub release: std::sync::mpsc::Receiver<()>,
}
#[cfg(test)]
pub(super) static APPLY_PAUSE: Mutex<Option<ApplyPause>> = Mutex::new(None);
#[cfg(test)]
fn pause_apply(store: usize) {
    let pause = {
        let mut hook = APPLY_PAUSE.lock().unwrap();
        if hook.as_ref().is_some_and(|p| p.store == store) {
            hook.take()
        } else {
            None
        }
    };
    if let Some(pause) = pause {
        let _ = pause.entered.send(());
        let _ = pause.release.recv();
    }
}
impl RaftStateMachine<Arx> for SmAdapter {
    type SnapshotData = Vec<u8>;
    type SnapshotBuilder = SnapBuilder;
    async fn applied_state(
        &mut self,
    ) -> io::Result<(
        Option<Id>,
        openraft::type_config::alias::StoredMembershipOf<Arx>,
    )> {
        self.run(|s| Ok((s.applied().map(id), membership(s.membership())?)))
            .await
    }
    async fn apply<S>(&mut self, mut entries: S) -> io::Result<()>
    where
        S: Stream<Item = io::Result<EntryResponder<Arx>>> + Unpin + Send,
    {
        while let Some(e) = entries.next().await {
            let (e, responder) = e?;
            let (p, e) = to_stored(e).map_err(err)?;
            let store = self.0.clone();
            let outcome = blocking(move || {
                #[cfg(test)]
                pause_apply(Arc::as_ptr(&store) as usize);
                store.lock().map_err(|_| StoreError::Poisoned)?.apply(p, &e)
            })
            .await?;
            if let Some(r) = responder {
                r.send(Applied(outcome));
            }
        }
        Ok(())
    }
    async fn get_snapshot_builder(&mut self) -> SnapBuilder {
        SnapBuilder(self.clone())
    }
    async fn get_current_snapshot(&mut self) -> io::Result<Option<Snap>> {
        let s = self.snapshot().await?;
        Ok(s.meta.last_log_id.map(|_| s))
    }
    async fn install_snapshot(
        &mut self,
        meta: &openraft::type_config::alias::SnapshotMetaOf<Arx>,
        blob: Vec<u8>,
    ) -> io::Result<()> {
        let meta = meta.clone();
        self.run(move |s| {
            let (p, m) = StateStore::peek(&blob)?;
            if p.map(id) != meta.last_log_id || membership(m.as_ref())? != meta.last_membership {
                return Err(StoreError::Corrupt);
            }
            s.install(&blob)
        })
        .await
    }
}

impl Default for Params {
    fn default() -> Self {
        Self {
            snapshot_logs: 64,
            keep: 32,
            purge_batch: 32,
            lag: 128,
            election_min: 500,
            election_max: 1000,
        }
    }
}
impl Params {
    pub fn config(self) -> Result<Config, StoreError> {
        if self.snapshot_logs == 0
            || self.purge_batch == 0
            || self.lag <= self.snapshot_logs
            || self
                .snapshot_logs
                .checked_add(self.keep)
                .and_then(|v| v.checked_add(self.purge_batch))
                .and_then(|v| v.checked_add(72))
                .is_none_or(|v| v > 512)
        {
            return Err(StoreError::TooLarge);
        }
        Config {
            heartbeat_interval: 100,
            election_timeout_min: self.election_min,
            election_timeout_max: self.election_max,
            max_payload_entries: 64,
            max_append_entries: Some(64),
            api_batch_capacity: Some(64),
            api_channel_size: Some(1024),
            snapshot_policy: SnapshotPolicy::LogsSinceLast(self.snapshot_logs),
            max_in_snapshot_log_to_keep: self.keep,
            purge_batch_size: self.purge_batch,
            replication_lag_threshold: self.lag,
            enable_pre_vote: Some(false),
            ..Config::default()
        }
        .validate()
        .map_err(|_| StoreError::Corrupt)
    }
}
