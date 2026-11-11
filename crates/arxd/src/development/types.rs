use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
#[derive(Clone, Debug)]
pub struct RunSpec {
    /// Absolute executable, passed directly to exec. No implicit shell or PATH lookup.
    pub executable: PathBuf,
    pub arguments: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Total retained bytes per stream, split between head and tail.
    pub output_bytes: usize,
}

/// Bytes are untrusted command output. Never render as HTML or instructions.
#[derive(Debug, PartialEq, Eq)]
pub struct CapturedOutput {
    pub head: Vec<u8>,
    pub tail: Vec<u8>,
    pub dropped_bytes: u64,
}
#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
