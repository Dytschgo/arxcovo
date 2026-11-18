//! Advisory local applied-state hint. Query identifiers confer no authority.
use super::*;
use crate::cluster::enrollment::{ClusterId, CodeId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cluster) enum AnchorQueryError {
    Refused,
    Busy,
    Unavailable,
}
impl std::fmt::Display for AnchorQueryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Refused => "anchor query refused",
            Self::Busy => "anchor query busy",
            Self::Unavailable => "anchor query unavailable",
        })
    }
}
impl std::error::Error for AnchorQueryError {}

impl ConsensusNode {
    pub(in crate::cluster) async fn anchor_query(
        &self,
        query: &[u8],
        clock: &dyn checked_clock::CheckedClock,
    ) -> Result<Vec<u8>, AnchorQueryError> {
        let deadline = Instant::now() + READ_BUDGET;
        if query.len() != 32 {
            return Err(AnchorQueryError::Refused);
        }
        let mut cluster: ClusterId = [0; 16];
        let mut code: CodeId = [0; 16];
        cluster.copy_from_slice(&query[..16]);
        code.copy_from_slice(&query[16..]);
        let now_ms = clock
            .sample_ms()
            .map_err(|_| AnchorQueryError::Unavailable)?;
        let issue_unit = self
            .issue_permits
            .clone()
            .try_acquire_owned()
            .map_err(|_| AnchorQueryError::Busy)?;
        let read_unit = match self.read_permits.clone().try_acquire_owned() {
            Ok(unit) => unit,
            Err(_) => {
                drop(issue_unit);
                return Err(AnchorQueryError::Busy);
            }
        };
        let (tx, rx) = oneshot::channel();
        {
            let mut owners = self
                .owners
                .lock()
                .map_err(|_| AnchorQueryError::Unavailable)?;
            if owners.closed {
                return Err(AnchorQueryError::Unavailable);
            }
            while owners.set.try_join_next().is_some() {}
            let sm = self
                .sm
                .as_ref()
                .ok_or(AnchorQueryError::Unavailable)?
                .clone();
            #[cfg(test)]
            let entered = self.read_blocking.clone();
            let handle = owners.set.spawn(async move {
                let result = if tx.is_closed() || Instant::now() >= deadline {
                    drop(sm);
                    drop(read_unit);
                    drop(issue_unit);
                    Err(AnchorQueryError::Unavailable)
                } else {
                    blocking(move || {
                        #[cfg(test)]
                        entered.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let result = (|| {
                            let store = sm.0.lock().map_err(|_| AnchorQueryError::Unavailable)?;
                            let membership = match store.membership() {
                                Some((_, blob)) => Some(
                                    WireMembership::decode(blob.bytes())
                                        .map_err(|_| AnchorQueryError::Unavailable)?,
                                ),
                                None => None,
                            };
                            store
                                .state()
                                .pending_voters(cluster, code, now_ms, membership.as_ref())
                                .map(|voters| voters.encode())
                                .map_err(|_| AnchorQueryError::Refused)
                        })();
                        // Guard first, then Arc, read, issue, then completion.
                        drop(sm);
                        drop(read_unit);
                        drop(issue_unit);
                        Ok(result)
                    })
                    .await
                    .unwrap_or(Err(AnchorQueryError::Unavailable))
                };
                if !tx.is_closed() && Instant::now() < deadline {
                    let _ = tx.send(result);
                }
            });
            #[cfg(test)]
            anchor_query_tests::record_owner(self, handle);
            #[cfg(not(test))]
            drop(handle);
        }
        match timeout_at(deadline, rx).await {
            Ok(Ok(result)) if Instant::now() < deadline => result,
            _ => Err(AnchorQueryError::Unavailable),
        }
    }
}

#[cfg(test)]
#[path = "anchor_query_tests.rs"]
mod anchor_query_tests;
