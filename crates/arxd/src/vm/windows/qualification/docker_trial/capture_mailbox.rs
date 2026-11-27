//! One local, owner-private mailbox for a separately operated capture signer.
use super::*;
use std::ops::Range;

const NETWORK: NativeRefusal = NativeRefusal::NetworkFenceUnverified;
const ARM_WAIT: Duration = Duration::from_secs(120);
const CAPTURE_WAIT: Duration = Duration::from_secs(900);
const POLL: Duration = Duration::from_millis(50);

pub(in crate::vm::windows::qualification) struct Mailbox {
    root: PathPin,
    cancel: Shutdown,
    origin_ms: u64,
    armed: Option<Hash>,
}

impl Mailbox {
    pub(in crate::vm::windows::qualification) fn open(
        path: &LocalPath,
        owner: PSID,
        forbidden: &[&LocalPath],
        origin_ms: u64,
        cancel: Shutdown,
    ) -> Result<Self, NativeRefusal> {
        if path.is_drive_root() || forbidden.iter().any(|root| path.overlaps(root)) {
            return Err(NETWORK);
        }
        let root = PathPin::open(path, true, true).map_err(|_| NETWORK)?;
        if !sink::held_private(&root.leaf, owner) || !sink::empty(&root.leaf)? {
            return Err(NETWORK);
        }
        Ok(Self {
            root,
            cancel,
            origin_ms,
            armed: None,
        })
    }

    fn require_current(&self) -> Result<(), NativeRefusal> {
        if self.cancel.is_cancelled() {
            return Err(NETWORK);
        }
        inventory::binding(&self.root).map_err(|_| NETWORK)?;
        require_held_private(&self.root.leaf).map_err(|_| NETWORK)
    }

    fn write(&self, staging: &str, name: &str, bytes: &[u8]) -> Result<(), NativeRefusal> {
        self.require_current()?;
        let mut file = sink::create_hidden(&self.root.leaf, staging).map_err(|_| NETWORK)?;
        file.write_all(bytes).map_err(|_| NETWORK)?;
        file.sync_all().map_err(|_| NETWORK)?;
        self.require_current()?;
        sink::rename_within(&file, &self.root.leaf, name).map_err(|_| NETWORK)
    }

    fn reply(
        &self,
        name: &str,
        max: u64,
        cutoff_ms: u64,
        wait: Duration,
        current: &mut dyn FnMut() -> Result<(), NativeRefusal>,
    ) -> Result<Vec<u8>, NativeRefusal> {
        let local_cutoff = boot::uptime().saturating_add(wait.as_millis() as u64);
        let cutoff = local_cutoff.min(self.origin_ms.saturating_add(cutoff_ms));
        let mut next_sample = 0;
        loop {
            self.require_current()?;
            if boot::uptime() >= next_sample {
                current()?;
                next_sample = boot::uptime().saturating_add(1000);
            }
            if boot::uptime() >= cutoff {
                return Err(NETWORK);
            }
            match open_relative(&self.root.leaf, name, Node::File, false, false) {
                Ok(file) => {
                    let (identity, _) = file_identity(&file, false).map_err(|_| NETWORK)?;
                    if identity.volume != self.root.identity.volume {
                        return Err(NETWORK);
                    }
                    require_held_private(&file).map_err(|_| NETWORK)?;
                    let bytes = read_bounded(&file, max).map_err(|_| NETWORK)?;
                    self.require_current()?;
                    if boot::uptime() >= cutoff {
                        return Err(NETWORK);
                    }
                    current()?;
                    if boot::uptime() >= cutoff {
                        return Err(NETWORK);
                    }
                    return Ok(bytes);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    std::thread::sleep(POLL);
                }
                Err(_) => return Err(NETWORK),
            }
        }
    }
}

impl Capture for Mailbox {
    fn arm(
        &mut self,
        request: &CaptureRequest<'_>,
        current: &mut dyn FnMut() -> Result<(), NativeRefusal>,
    ) -> Result<Vec<u8>, NativeRefusal> {
        let encoded = serde_json::to_vec(request).map_err(|_| NETWORK)?;
        if self.armed.is_some()
            || request.origin_port == 0
            || request.issued_ms >= request.cutoff_ms
            || request.cutoff_ms > MAX_TRIAL_MS
            || !sink::empty(&self.root.leaf)?
        {
            return Err(NETWORK);
        }
        self.write(".request.tmp", "request.json", &encoded)?;
        let reply = self.reply("armed.sig", 64, request.cutoff_ms, ARM_WAIT, current)?;
        self.armed = Some(digest(&encoded));
        Ok(reply)
    }

    fn fixture(
        &mut self,
        request: &CaptureRequest<'_>,
        window: Range<u64>,
        current: &mut dyn FnMut() -> Result<(), NativeRefusal>,
    ) -> Result<Vec<u8>, NativeRefusal> {
        let encoded = serde_json::to_vec(request).map_err(|_| NETWORK)?;
        let challenge = digest(&encoded);
        if self.armed.take() != Some(challenge)
            || window.start < request.issued_ms
            || window.end != request.cutoff_ms
            || window.start >= window.end
        {
            return Err(NETWORK);
        }
        #[derive(Serialize)]
        struct Started {
            request_sha256: Hash,
            origin_uptime_ms: u64,
            window_start_ms: u64,
            cutoff_ms: u64,
        }
        let started = serde_json::to_vec(&Started {
            request_sha256: challenge,
            origin_uptime_ms: self.origin_ms,
            window_start_ms: window.start,
            cutoff_ms: window.end,
        })
        .map_err(|_| NETWORK)?;
        self.write(".started.tmp", "started.json", &started)?;
        self.reply(
            "capture.bin",
            (64 + witness::MAX_CAPTURE_BYTES) as u64,
            window.end,
            CAPTURE_WAIT,
            current,
        )
    }
}
