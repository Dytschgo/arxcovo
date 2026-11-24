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
    fn open(&self, reference: Id) -> Result<Zeroizing<Vec<u8>>>;
}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) struct Dpapi {
    root: std::path::PathBuf,
    pin: crate::config::RootPin,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl Dpapi {
    pub(crate) fn new(root: &std::path::Path, state: &std::path::Path) -> Result<Self> {
        use super::paths::LocalPath;
        let local = |path: &std::path::Path| {
            LocalPath::parse(path.to_str().unwrap_or_default())
                .map_err(|_| anyhow::anyhow!("invalid Compose custody path"))
        };
        ensure!(
            !local(root)?.overlaps(&local(state)?),
            "Compose custody overlaps VM state"
        );
        let pin = crate::config::RootPin::acquire(root)?;
        crate::local::require_owner(root)?;
        crate::security::private(root)?;
        pin.verify(root)?;
        Ok(Self {
            root: root.into(),
            pin,
        })
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl SecretSource for Dpapi {
    fn open(&self, reference: Id) -> Result<Zeroizing<Vec<u8>>> {
        use std::{
            io::Read,
            os::windows::fs::{MetadataExt, OpenOptionsExt},
        };
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
        };
        const MAX_CIPHERTEXT: u64 = 16 * 1024;
        let result = (|| {
            self.pin.verify(&self.root)?;
            crate::security::private(&self.root)?;
            let path = self.root.join(format!("{}.dpapi", hex::encode(reference)));
            let file = std::fs::OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&path)?;
            let meta = file.metadata()?;
            ensure!(
                meta.is_file()
                    && meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0
                    && (1..=MAX_CIPHERTEXT).contains(&meta.len()),
                "invalid Compose custody file"
            );
            crate::local::require_owner(&path)?;
            crate::security::private(&path)?;
            let mut bytes = Zeroizing::new(Vec::with_capacity(meta.len() as usize));
            file.take(MAX_CIPHERTEXT + 1).read_to_end(&mut bytes)?;
            ensure!(
                bytes.len() as u64 == meta.len() && Sha256::digest(&*bytes)[..16] == reference,
                "Compose ciphertext reference mismatch"
            );
            crate::security::unprotect_current_user(&mut bytes, MAX_SECRET)
        })();
        result.map_err(|_: anyhow::Error| anyhow::anyhow!("Compose secret unavailable"))
    }
}

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
        if submitted {
            // Re-sending the bytes is only a private proof of the settled
            // file's equality. An unknown query never authorizes writing it.
            self.prove(channel, cancel).await?;
        }
        if self
            .send(source, channel, cancel, || {
                if submitted { Ok(()) } else { mark() }
            })
            .await?
        {
            return Ok(());
        }
        self.prove(channel, cancel).await?;
        ensure!(
            self.send(source, channel, cancel, || Ok(())).await?,
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
                OperationState::Succeeded if operation.stage == OperationStage::Settled => {
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
            return Ok(
                view.state == OperationState::Succeeded && view.stage == OperationStage::Settled
            );
        }
        Ok(true)
    }
}
