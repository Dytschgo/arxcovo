//! Read only the two Windows Setup states from an offline, writer-excluded pair.
//! All guest offsets are interpreted locally and never used as host paths.

mod ntfs;
mod qcow;
mod setup;

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    sync::Arc,
    time::Instant,
};

use super::{PathPin, file_identity, verify_volume_root};
use crate::vm::{ArtifactEvidence, PairSlots, runtime::StopDisposition};

const GENERALIZED: &str = "IMAGE_STATE_GENERALIZE_RESEAL_TO_OOBE";
const MAX_DISK_BYTES: u64 = 200 * (1 << 30);
const MAX_VARS_BYTES: u64 = 16 * (1 << 20);
const MAX_PARSE_IO: u64 = 1024 * (1 << 20);
const MAX_PARSE_READS: u32 = 2_000_000;
const MAX_PARSE_STEPS: u64 = 4_000_000;
const MAX_PARSE_HEAP: u64 = 256 * (1 << 20);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::vm) struct Invalid;

type Result<T> = std::result::Result<T, Invalid>;

/// Cooperative end-to-end work, I/O, heap and deadline budget shared by
/// hashing, qcow2, NTFS and registry parsing.
pub(super) struct Budget {
    io_bytes: u64,
    reads: u32,
    steps: u64,
    heap_bytes: u64,
    deadline: Instant,
    /// The owning operation's cancellation, checked with the deadline.
    stop: Option<crate::service::Shutdown>,
}

impl Budget {
    pub(super) fn new(deadline: Instant) -> Self {
        Self::with_limits(
            MAX_PARSE_IO,
            MAX_PARSE_READS,
            MAX_PARSE_STEPS,
            MAX_PARSE_HEAP,
            deadline,
        )
    }

    pub(super) fn with_limits(
        io_bytes: u64,
        reads: u32,
        steps: u64,
        heap_bytes: u64,
        deadline: Instant,
    ) -> Self {
        Self {
            io_bytes,
            reads,
            steps,
            heap_bytes,
            deadline,
            stop: None,
        }
    }

    /// Also end the parse promptly when the owning operation is cancelled.
    pub(super) fn with_stop(mut self, stop: Option<crate::service::Shutdown>) -> Self {
        self.stop = stop;
        self
    }

    pub(super) fn spend_io(&mut self, size: usize) -> Result<()> {
        self.io_bytes = self.io_bytes.checked_sub(size as u64).ok_or(Invalid)?;
        self.reads = self.reads.checked_sub(1).ok_or(Invalid)?;
        self.check_deadline()
    }

    pub(super) fn spend_step(&mut self, count: u64) -> Result<()> {
        self.steps = self.steps.checked_sub(count).ok_or(Invalid)?;
        // Check wall deadline periodically inside tight cell/entry scans.
        if self.steps & 0x3ff == 0 {
            self.check_deadline()?;
        }
        Ok(())
    }

    pub(super) fn reserve_heap(&mut self, size: usize) -> Result<()> {
        self.heap_bytes = self.heap_bytes.checked_sub(size as u64).ok_or(Invalid)?;
        self.check_deadline()
    }

    pub(super) fn check_deadline(&self) -> Result<()> {
        (Instant::now() < self.deadline && !stopped(self.stop.as_ref()))
            .then_some(())
            .ok_or(Invalid)
    }
}

/// The native adapter retains this after collecting independent release
/// evidence for Windows Finalize. Both leaves exclude host and guest writers
/// until the offline observation finishes.
pub(in crate::vm) struct ReleasedPair {
    slots: PairSlots,
    pins: Arc<[PathPin]>,
    pair: [usize; 2],
    readers: [File; 2],
}

impl ReleasedPair {
    pub(super) fn hold(
        slots: PairSlots,
        pins: Arc<[PathPin]>,
        pair: [usize; 2],
        release: StopDisposition,
    ) -> Result<Self> {
        let disk = pins.get(pair[0]).ok_or(Invalid)?;
        let vars = pins.get(pair[1]).ok_or(Invalid)?;
        if release != StopDisposition::CleanRelease
            || disk.identity == vars.identity
            || disk.path.as_str().rsplit_once('\\').map(|(dir, _)| dir)
                != vars.path.as_str().rsplit_once('\\').map(|(dir, _)| dir)
        {
            return Err(Invalid);
        }
        let disk_reader = disk.reopen_without_writer().map_err(|_| Invalid)?;
        let vars_reader = vars.reopen_without_writer().map_err(|_| Invalid)?;
        Ok(Self {
            slots,
            pins,
            pair,
            readers: [disk_reader, vars_reader],
        })
    }

    #[allow(dead_code, reason = "Finalize wiring is deferred to PR 2")]
    pub(in crate::vm) fn observe(
        &self,
        slots: PairSlots,
        files: [ArtifactEvidence; 2],
        deadline: Instant,
    ) -> Result<(bool, bool)> {
        self.observe_until(slots, files, deadline, None)
    }

    /// As [`ReleasedPair::observe`], also ending both hash passes and the
    /// offline parse promptly when `stop` is cancelled.
    pub(in crate::vm) fn observe_until(
        &self,
        slots: PairSlots,
        files: [ArtifactEvidence; 2],
        deadline: Instant,
        stop: Option<&crate::service::Shutdown>,
    ) -> Result<(bool, bool)> {
        if self.slots != slots || files[0].directory != files[1].directory {
            return Err(Invalid);
        }
        // Account for pre- and post-parse hash work inside one bounded byte
        // ceiling and cooperative deadline check.
        let mut hash_remaining = files[0]
            .pin
            .size
            .checked_add(files[1].pin.size)
            .and_then(|total| total.checked_mul(2))
            .filter(|&total| total <= 2 * (MAX_DISK_BYTES + MAX_VARS_BYTES))
            .ok_or(Invalid)?;
        self.verify(files, deadline, stop, &mut hash_remaining)?;
        // Finalize attests the standalone candidate, never a clone.
        let result = ntfs::setup_state(&mut qcow::Image::open(
            &self.readers[0],
            None,
            Budget::new(deadline).with_stop(stop.cloned()),
        )?);
        self.verify(files, deadline, stop, &mut hash_remaining)?;
        result
    }

    #[allow(dead_code, reason = "Finalize wiring is deferred to PR 2")]
    fn verify(
        &self,
        files: [ArtifactEvidence; 2],
        deadline: Instant,
        stop: Option<&crate::service::Shutdown>,
        hash_remaining: &mut u64,
    ) -> Result<()> {
        for (((index, mut file), expected), limit) in self
            .pair
            .into_iter()
            .zip(&self.readers)
            .zip(files)
            .zip([MAX_DISK_BYTES, MAX_VARS_BYTES])
        {
            let pin = self.pins.get(index).ok_or(Invalid)?;
            let root = pin._ancestors.first().ok_or(Invalid)?;
            verify_volume_root(
                &pin.path.as_str()[..3],
                file_identity(root, true).map_err(|_| Invalid)?.0,
            )
            .map_err(|_| Invalid)?;
            if pin.identity != expected.identity
                || file_identity(file, false).map_err(|_| Invalid)?.0 != expected.identity
                || file.metadata().map_err(|_| Invalid)?.len() != expected.pin.size
                || Instant::now() >= deadline
                || stopped(stop)
            {
                return Err(Invalid);
            }
            file.seek(SeekFrom::Start(0)).map_err(|_| Invalid)?;
            let observed = expected.pin.verify(
                BoundedHashRead {
                    file,
                    deadline,
                    stop,
                    remaining: hash_remaining,
                },
                limit,
            );
            if !matches!(observed, Ok(true)) || Instant::now() >= deadline || stopped(stop) {
                return Err(Invalid);
            }
        }
        Ok(())
    }
}

#[allow(dead_code, reason = "Finalize wiring is deferred to PR 2")]
struct BoundedHashRead<'a> {
    file: &'a File,
    deadline: Instant,
    stop: Option<&'a crate::service::Shutdown>,
    remaining: &'a mut u64,
}

fn stopped(stop: Option<&crate::service::Shutdown>) -> bool {
    stop.is_some_and(crate::service::Shutdown::is_cancelled)
}

impl Read for BoundedHashRead<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        if stopped(self.stop) {
            return Err(io::Error::other("offline verification cancelled"));
        }
        let max = buffer
            .len()
            .min(usize::try_from(*self.remaining).unwrap_or(usize::MAX));
        // ArtifactPin asks for one more byte to distinguish EOF from truncation.
        if max == 0 && !buffer.is_empty() && self.file.read(&mut [0u8; 1])? != 0 {
            return Err(io::Error::other("hash byte budget exhausted"));
        }
        let n = self.file.read(&mut buffer[..max])?;
        *self.remaining = self
            .remaining
            .checked_sub(n as u64)
            .ok_or_else(|| io::Error::other("hash byte budget exhausted"))?;
        if Instant::now() >= self.deadline {
            return Err(io::ErrorKind::TimedOut.into());
        }
        // Exercise cancellation at the read boundary without racing the file I/O.
        #[cfg(test)]
        if n != 0
            && let Some(stop) = self.stop
        {
            cancel_tests::cancel_after_hash_read(stop);
        }
        if stopped(self.stop) {
            return Err(io::Error::other("offline verification cancelled"));
        }
        Ok(n)
    }
}

fn bytes(data: &[u8], at: usize, size: usize) -> Result<&[u8]> {
    data.get(at..at.checked_add(size).ok_or(Invalid)?)
        .ok_or(Invalid)
}

fn word<const N: usize>(data: &[u8], at: usize) -> Result<[u8; N]> {
    bytes(data, at, N)?.try_into().map_err(|_| Invalid)
}

fn le16(data: &[u8], at: usize) -> Result<u16> {
    word(data, at).map(u16::from_le_bytes)
}

fn le32(data: &[u8], at: usize) -> Result<u32> {
    word(data, at).map(u32::from_le_bytes)
}

fn le64(data: &[u8], at: usize) -> Result<u64> {
    word(data, at).map(u64::from_le_bytes)
}

fn be16(data: &[u8], at: usize) -> Result<u16> {
    word(data, at).map(u16::from_be_bytes)
}

fn be32(data: &[u8], at: usize) -> Result<u32> {
    word(data, at).map(u32::from_be_bytes)
}

fn be64(data: &[u8], at: usize) -> Result<u64> {
    word(data, at).map(u64::from_be_bytes)
}

fn range(at: u64, size: u64, limit: u64) -> Result<()> {
    at.checked_add(size)
        .filter(|end| *end <= limit)
        .map(|_| ())
        .ok_or(Invalid)
}

#[cfg(test)]
mod cancel_tests {
    use super::*;
    use crate::{service::Shutdown, vm::paths::ArtifactPin};
    use sha2::{Digest, Sha256};
    use std::{cell::Cell, io::Write, sync::mpsc, thread, time::Duration};

    thread_local! {
        static CANCEL_AFTER_HASH_READ: Cell<bool> = const { Cell::new(false) };
    }

    pub(super) fn arm_hash_cancellation() {
        CANCEL_AFTER_HASH_READ.set(true);
    }

    pub(super) fn cancel_after_hash_read(stop: &Shutdown) {
        if CANCEL_AFTER_HASH_READ.take() {
            stop.cancel();
        }
    }

    pub(super) fn returns_promptly<T: Send + 'static>(
        operation: impl FnOnce(Instant) -> T + Send + 'static,
    ) -> T {
        let (send, receive) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = operation(Instant::now() + Duration::from_secs(20));
            let _ = send.send(result);
        });
        match receive.recv_timeout(Duration::from_millis(200)) {
            Ok(result) => {
                worker.join().unwrap();
                result
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                worker.join().unwrap();
                unreachable!()
            }
            Err(mpsc::RecvTimeoutError::Timeout) => panic!("offline cancellation exceeded 200 ms"),
        }
    }

    fn verify_cancelled_hash(cancel_before_read: bool) {
        let contents = [7u8; 32 * 1024];
        let mut file = tempfile::tempfile().unwrap();
        file.write_all(&contents).unwrap();
        file.rewind().unwrap();
        let pin = ArtifactPin {
            size: contents.len() as u64,
            sha256: Sha256::digest(contents).into(),
        };
        let remaining = returns_promptly(move |deadline| {
            let cancel = Shutdown::default();
            if cancel_before_read {
                cancel.cancel();
            } else {
                arm_hash_cancellation();
            }
            let mut remaining = pin.size;
            let error = pin
                .verify(
                    BoundedHashRead {
                        file: &file,
                        deadline,
                        stop: Some(&cancel),
                        remaining: &mut remaining,
                    },
                    pin.size,
                )
                .unwrap_err();
            assert_eq!(error.to_string(), "offline verification cancelled");
            assert!(cancel.is_cancelled());
            remaining
        });
        if cancel_before_read {
            assert_eq!(remaining, pin.size);
        } else {
            assert!(remaining < pin.size);
        }
    }

    #[test]
    fn artifact_verification_returns_promptly_if_already_cancelled() {
        verify_cancelled_hash(true);
    }

    #[test]
    fn artifact_verification_returns_promptly_if_cancelled_after_a_chunk() {
        verify_cancelled_hash(false);
    }

    #[test]
    fn cancellation_ends_the_offline_parse_before_the_deadline() {
        let cancel = Shutdown::default();
        let deadline = Instant::now() + Duration::from_secs(20);
        let budget = Budget::new(deadline).with_stop(Some(cancel.clone()));
        assert_eq!(budget.check_deadline(), Ok(()));
        cancel.cancel();
        assert_eq!(budget.check_deadline(), Err(Invalid));
        // Without a stop the deadline alone still governs.
        assert_eq!(Budget::new(deadline).check_deadline(), Ok(()));
    }
}
