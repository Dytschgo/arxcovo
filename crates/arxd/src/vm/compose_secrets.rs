//! Compose runtime secrets. The owner provisions user-scope DPAPI files in a
//! separately pinned private directory. Only ciphertext references enter SQLite.
use super::{
    Id,
    compose::{Intent, SecretRef},
    guest::ComposeChannel,
};
use crate::service::Shutdown;
use anyhow::{Result, ensure};
use arx_guest::compose::{ComposeRequest, ComposeResponse, MAX_SECRET, SecretText, encode_base64};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(crate) trait SecretSource: Send + Sync {
    fn verify(&self) -> Result<()> {
        Ok(())
    }

    fn open(&self, reference: Id) -> Result<Zeroizing<Vec<u8>>>;
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) struct Dpapi {
    custody: std::sync::Arc<crate::config::VmCustody>,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl Dpapi {
    pub(crate) fn new(custody: std::sync::Arc<crate::config::VmCustody>) -> Self {
        Self { custody }
    }

    #[cfg(test)]
    pub(crate) fn read_with_binding_check(
        &self,
        reference: Id,
        verify: impl Fn() -> Result<()>,
    ) -> Result<Zeroizing<Vec<u8>>> {
        self.read_from(reference, verify)
    }

    fn read_from(
        &self,
        reference: Id,
        verify: impl Fn() -> Result<()>,
    ) -> Result<Zeroizing<Vec<u8>>> {
        use std::io::Read;
        const MAX_CIPHERTEXT: u64 = 16 * 1024;
        let result = (|| {
            verify()?;
            let mut file = self.custody.open_ciphertext(reference)?;
            let length = file.metadata()?.len();
            ensure!(
                (1..=MAX_CIPHERTEXT).contains(&length),
                "invalid Compose custody file"
            );
            let mut bytes = Zeroizing::new(Vec::with_capacity(length as usize));
            (&mut file)
                .take(MAX_CIPHERTEXT + 1)
                .read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 == length && Sha256::digest(&*bytes)[..16] == reference,
                "Compose ciphertext reference mismatch"
            );
            let plaintext = crate::security::unprotect_current_user(&mut bytes, MAX_SECRET)?;
            verify()?;
            Ok(plaintext)
        })();
        result.map_err(|_: anyhow::Error| anyhow::anyhow!("Compose secret unavailable"))
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl SecretSource for Dpapi {
    fn verify(&self) -> Result<()> {
        self.custody
            .verify()
            .map_err(|_| anyhow::anyhow!("Compose secret unavailable"))
    }

    fn open(&self, reference: Id) -> Result<Zeroizing<Vec<u8>>> {
        self.read_from(reference, || self.verify())
    }
}

#[derive(Debug)]
pub(crate) struct FailedDelivery;

impl std::fmt::Display for FailedDelivery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Compose secret operation failed")
    }
}

impl std::error::Error for FailedDelivery {}

pub(crate) struct Delivery<'a> {
    intent: &'a Intent,
    secret: &'a SecretRef,
    committed: Option<u64>,
    operation: String,
}

impl<'a> Delivery<'a> {
    pub(crate) fn new(intent: &'a Intent, secret: &'a SecretRef, committed: Option<u64>) -> Self {
        let mut hash = Sha256::new();
        hash.update(b"arxcovo-compose-secret-delivery-v1");
        hash.update(intent.id);
        hash.update(intent.release);
        hash.update(intent.digest.as_bytes());
        hash.update(intent.generation);
        hash.update(intent.boot.to_be_bytes());
        hash.update(secret.reference);
        hash.update(secret.epoch.to_be_bytes());
        hash.update(secret.slot.as_bytes());
        Self {
            intent,
            secret,
            committed,
            operation: hex::encode(&hash.finalize()[..16]),
        }
    }

    pub(crate) async fn deliver(
        &self,
        source: &dyn SecretSource,
        channel: &mut ComposeChannel,
        cancel: &Shutdown,
        submitted: bool,
        mark: impl FnOnce() -> Result<()>,
    ) -> Result<()> {
        ensure!(
            channel.generation() == self.intent.boot,
            "Compose secret boot mismatch"
        );
        source.verify()?;
        if submitted {
            // Re-sending the bytes is only a private comparison against the
            // guest's exact journaled file instance. An unknown query never authorizes writing it.
            self.prove(channel, cancel).await?;
        }
        if self
            .send(source, channel, cancel, !submitted, || {
                if submitted { Ok(()) } else { mark() }
            })
            .await?
        {
            return Ok(());
        }
        source.verify()?;
        self.prove(channel, cancel).await?;
        ensure!(
            self.send(source, channel, cancel, false, || Ok(())).await?,
            "Compose secret receipt missing"
        );
        Ok(())
    }

    fn attributed(&self, view: &arx_guest::compose::OperationView) -> bool {
        use arx_guest::compose::OperationKind;
        view.project_id == self.intent.target.project
            && view.operation_id == self.operation
            && match view.kind {
                OperationKind::Recreate => Some(view.revision) == self.committed,
                OperationKind::SecretPut => {
                    view.revision == 0 || Some(view.revision) == self.committed
                }
                _ => false,
            }
    }

    async fn prove(&self, channel: &mut ComposeChannel, cancel: &Shutdown) -> Result<()> {
        use arx_guest::compose::{OperationStage, OperationState};
        use std::time::Duration;
        use tokio::time::{Instant, sleep};

        let deadline = Instant::now() + Duration::from_secs(120);
        let mut reconcile = false;
        loop {
            let response = channel
                .exchange(cancel, |id| {
                    let operation_id = self.operation.clone();
                    if reconcile {
                        ComposeRequest::ComposeReconcile { id, operation_id }
                    } else {
                        ComposeRequest::ComposeOperation { id, operation_id }
                    }
                })
                .await?;
            let ComposeResponse::ComposeOperationState { operation, .. } = response else {
                anyhow::bail!("Compose secret operation is unproven");
            };
            ensure!(
                self.attributed(&operation),
                "Compose secret operation attribution mismatch"
            );
            match operation.state {
                OperationState::Failed
                    if operation.stage == OperationStage::Settled
                        && operation.failure.is_some() =>
                {
                    return Err(FailedDelivery.into());
                }
                OperationState::Ambiguous
                    if operation.kind == arx_guest::compose::OperationKind::SecretPut =>
                {
                    return Ok(());
                }
                OperationState::Succeeded
                    if operation.stage == OperationStage::Settled
                        && operation.failure.is_none() =>
                {
                    return Ok(());
                }
                OperationState::Ambiguous if !reconcile => reconcile = true,
                OperationState::Running if Instant::now() < deadline => {
                    sleep(Duration::from_millis(250)).await
                }
                _ => anyhow::bail!("Compose secret operation is unproven"),
            }
        }
    }

    async fn send(
        &self,
        source: &dyn SecretSource,
        channel: &mut ComposeChannel,
        cancel: &Shutdown,
        initial: bool,
        mark: impl FnOnce() -> Result<()>,
    ) -> Result<bool> {
        use arx_guest::compose::{OperationKind, OperationStage, OperationState};
        let bytes = source.open(self.secret.reference)?;
        ensure!(
            (1..=MAX_SECRET).contains(&bytes.len()),
            "invalid Compose secret size"
        );
        let data = SecretText::new(encode_base64(&bytes));
        drop(bytes);
        mark()?;
        let response = channel
            .exchange(cancel, |id| ComposeRequest::ComposeSecretPut {
                id,
                operation_id: self.operation.clone(),
                generation: self.intent.boot,
                project_id: self.intent.target.project.clone(),
                digest: self.intent.digest.clone(),
                slot: self.secret.slot.clone(),
                epoch: self.secret.epoch,
                data,
            })
            .await?;
        use arx_guest::compose::RejectCode;
        if initial
            && matches!(
                &response,
                ComposeResponse::ComposeRejected {
                    code: RejectCode::StaleEpoch
                        | RejectCode::SecretConflict
                        | RejectCode::UnknownSlot
                        | RejectCode::UnknownRelease
                        | RejectCode::SecretSize
                        | RejectCode::GenerationMismatch
                        | RejectCode::Busy
                        | RejectCode::ReconcileRequired
                        | RejectCode::JournalFull,
                    ..
                }
            )
        {
            return Err(FailedDelivery.into());
        }
        if let ComposeResponse::ComposeOperationState { operation, .. } = &response {
            ensure!(
                self.attributed(operation),
                "Compose secret operation attribution mismatch"
            );
            if operation.stage == OperationStage::Settled
                && operation.state == OperationState::Failed
                && operation.failure.is_some()
            {
                return Err(FailedDelivery.into());
            }
            return Ok(false);
        }
        let ComposeResponse::ComposeSecretAccepted {
            project_id,
            digest,
            slot,
            epoch,
            recreate,
            ..
        } = response
        else {
            anyhow::bail!("Compose secret receipt unavailable")
        };
        ensure!(
            project_id == self.intent.target.project
                && digest == self.intent.digest
                && slot == self.secret.slot
                && epoch == self.secret.epoch,
            "Compose secret receipt attribution mismatch"
        );
        if let Some(view) = recreate {
            ensure!(
                self.attributed(&view) && view.kind == OperationKind::Recreate,
                "Compose recreate receipt attribution mismatch"
            );
            if view.state == OperationState::Failed
                && view.stage == OperationStage::Settled
                && view.failure.is_some()
            {
                return Err(FailedDelivery.into());
            }
            return Ok(view.failure.is_none()
                && view.state == OperationState::Succeeded
                && view.stage == OperationStage::Settled);
        }
        Ok(true)
    }
}
