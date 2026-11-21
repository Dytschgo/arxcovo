#[cfg(test)]
use std::os::fd::{FromRawFd, OwnedFd};
use std::{mem::MaybeUninit, os::fd::RawFd};

fn nonblocking_writer(fd: RawFd) -> bool {
    // SAFETY: fcntl inspects the descriptor status flags.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    flags >= 0 && flags & libc::O_NONBLOCK != 0 && flags & libc::O_ACCMODE == libc::O_WRONLY
}

fn fifo_identity(fd: RawFd) -> Option<(libc::dev_t, libc::ino_t)> {
    let mut stat = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat inspects the descriptor and initializes stat on success.
    if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: fstat succeeded.
    let stat = unsafe { stat.assume_init() };
    (stat.st_mode & libc::S_IFMT == libc::S_IFIFO).then_some((stat.st_dev, stat.st_ino))
}

pub(crate) fn pipe_writer_identity(fd: RawFd) -> Option<(libc::dev_t, libc::ino_t)> {
    if !nonblocking_writer(fd) {
        return None;
    }
    fifo_identity(fd)
}

#[cfg(test)]
pub(crate) const PUBLICATION_DIAGNOSTIC_FD: RawFd = 4;
#[cfg(test)]
pub(crate) const PUBLICATION_DIAGNOSTIC_PIPE: &str = "ARX_C13_DIAGNOSTIC_PIPE";

/// Claim the private publication writer after exec and fence it from later execs.
///
/// SAFETY: the caller is the child entry point and exclusively owns the
/// descriptor installed by its launcher when the pipe identity matches.
#[cfg(test)]
pub(crate) unsafe fn claim_publication_writer() -> Option<OwnedFd> {
    let expected = std::env::var(PUBLICATION_DIAGNOSTIC_PIPE).ok()?;
    let fd = PUBLICATION_DIAGNOSTIC_FD;
    let (dev, ino) = fifo_identity(fd)?;
    if expected != format!("{dev}:{ino}") {
        return None;
    }
    // SAFETY: the verified handoff belongs to this child entry point.
    let writer = unsafe { OwnedFd::from_raw_fd(fd) };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 || !nonblocking_writer(fd) {
        return None;
    }
    Some(writer)
}
