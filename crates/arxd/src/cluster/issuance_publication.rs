//! Store-only mechanism. Completion is never issuance, attach or restart authority.
#![allow(dead_code)]
use super::startup_plan::{Guard, Owner};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Unsupported,
    Layout,
    Context,
    IncompleteIssuance,
    Unconfirmed,
    Completed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Ready,
    Begun,
    Failed(Error),
    Completed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Empty,
    Reserved,
    Publishing,
    Complete,
}
#[derive(Default)]
pub(super) struct Attempt {
    state: State,
    phase: Phase,
    // Includes partial files on failure; never recapture or clean up.
    files: Vec<Guard>,
    #[cfg(test)]
    trace: Vec<&'static str>,
    #[cfg(test)]
    fault: Option<(&'static str, bool)>,
}
fn prior(owner: &Owner) -> Result<(), Error> {
    match owner.publication.state {
        State::Ready => Ok(()),
        State::Begun => Err(Error::Unconfirmed),
        State::Failed(e) => Err(e),
        State::Completed => Err(Error::Completed),
    }
}
fn finish(owner: &mut Owner, result: Result<(), Error>) -> Result<(), Error> {
    if let Err(e) = result {
        owner.publication.state = State::Failed(e);
    }
    result
}
/// Must precede issuer entry in later composition. This alone grants no authority.
pub(super) fn reserve(owner: &mut Owner) -> Result<(), Error> {
    prior(owner)?;
    owner.publication.state = State::Begun;
    #[cfg(not(target_os = "linux"))]
    let result = Err(Error::Unsupported);
    #[cfg(target_os = "linux")]
    let result = linux::reserve(owner);
    finish(owner, result)
}
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::cluster::{
        join_receipt::{ExpectedIdentity, PublicIdentity},
        pki::custody::MemberCopy,
    };
    use std::{
        fs::{self, File},
        io::Write,
        os::fd::{AsRawFd, FromRawFd},
    };
    const RESERVATION: &str = "identity.json";
    const TEMP: &str = ".identity-publish";
    fn layout<T>(r: Result<T, impl Sized>) -> Result<T, Error> {
        r.map_err(|_| Error::Layout)
    }
    // Faults live at concrete syscall/readback adapters, including unknown-effect errors.
    fn before(owner: &mut Owner, name: &'static str) -> Result<(), Error> {
        #[cfg(test)]
        if owner.publication.fault == Some((name, false)) {
            owner.publication.trace.push(name);
            return Err(Error::Unconfirmed);
        }
        let _ = (owner, name);
        Ok(())
    }
    fn observed(owner: &mut Owner, name: &'static str) -> Result<(), Error> {
        #[cfg(test)]
        {
            owner.publication.trace.push(name);
            if let Some((at, unwind)) = owner.publication.fault
                && at == name
            {
                if unwind {
                    panic!("synthetic publication adapter unwind");
                }
                return Err(Error::Unconfirmed);
            }
        }
        let _ = (owner, name);
        Ok(())
    }
    fn inventory(owner: &Owner, names: &[&str]) -> Result<(), Error> {
        layout(owner.publication_originals())?;
        for file in &owner.publication.files {
            layout(file.recheck())?;
        }
        for name in names {
            layout(crate::security::private(&owner.plan.credentials.join(name)))?;
        }
        let mut actual = fs::read_dir(&owner.plan.credentials)
            .map_err(|_| Error::Layout)?
            .take(5)
            .map(|e| {
                e.map_err(|_| Error::Layout)?
                    .file_name()
                    .into_string()
                    .map_err(|_| Error::Layout)
            })
            .collect::<Result<Vec<_>, _>>()?;
        actual.sort();
        let mut expected = names.to_vec();
        expected.sort();
        if actual != expected {
            return Err(Error::Layout);
        }
        Ok(())
    }
    fn create(owner: &mut Owner, name: &str, point: &'static str) -> Result<usize, Error> {
        layout(owner.publication_originals())?;
        let dir = layout(owner.publication_directory())?;
        let c = std::ffi::CString::new(name).map_err(|_| Error::Layout)?;
        // SAFETY: retained directory and fixed internal basename, exclusive no-follow open.
        before(owner, point)?;
        let fd = unsafe {
            libc::openat(
                dir.as_raw_fd(),
                c.as_ptr(),
                libc::O_RDWR
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(Error::Layout);
        }
        // SAFETY: openat returned a new owned descriptor.
        let file = unsafe { File::from_raw_fd(fd) };
        // Capture immediately, before any injected error/unwind following the create.
        let guard = layout(Guard::created(owner.plan.credentials.join(name), file))?;
        let index = owner.publication.files.len();
        owner.publication.files.push(guard);
        observed(owner, point)?;
        Ok(index)
    }
    fn sync(owner: &mut Owner, index: Option<usize>, point: &'static str) -> Result<(), Error> {
        let file = match index {
            Some(i) => layout(owner.publication.files[i].file().try_clone())?,
            None => layout(owner.publication_directory())?,
        };
        before(owner, point)?;
        layout(file.sync_all())?;
        observed(owner, point)
    }
    fn write(
        owner: &mut Owner,
        index: usize,
        bytes: &[u8],
        points: [&'static str; 4],
    ) -> Result<(), Error> {
        let mut file = layout(owner.publication.files[index].file().try_clone())?;
        let mid = bytes.len() / 2;
        layout(file.write_all(&bytes[..mid]))?;
        observed(owner, points[0])?;
        before(owner, points[1])?;
        layout(file.write_all(&bytes[mid..]))?;
        observed(owner, points[1])?;
        sync(owner, Some(index), points[2])?;
        sync(owner, None, points[3])
    }
    pub(super) fn reserve(owner: &mut Owner) -> Result<(), Error> {
        if owner.plan.mode.recovery() {
            return Err(Error::Context);
        }
        layout(owner.recheck())?;
        owner.publication.files.reserve(4);
        owner.publication_capacity();
        inventory(owner, &[])?;
        let i = create(owner, RESERVATION, "reservation-create")?;
        sync(owner, Some(i), "reservation-file-sync")?;
        sync(owner, None, "reservation-parent-sync")?;
        inventory(owner, &[RESERVATION])?;
        owner.publication.phase = Phase::Reserved;
        Ok(())
    }
    /// Mechanism validation, never historical acknowledgement or operational recovery.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::cluster) fn check_retained(
        owner: &Owner,
        expected: Option<&ExpectedIdentity>,
        installation: [u8; 16],
        member: &MemberCopy<'_>,
        now: Option<u64>,
    ) -> Result<(), Error> {
        let expected = expected.ok_or(Error::Context)?;
        if owner.plan.mode.founder() != expected.origin.founder() {
            return Err(Error::Context);
        }
        layout(owner.publication_originals())?;
        let root = layout(owner.read("ca.der", 4096))?;
        let key = layout(owner.read("member-key", 16384))?;
        let bytes = layout(owner.read("identity.json", 8192))?;
        let record = PublicIdentity::decode(&bytes).map_err(|_| Error::Context)?;
        if record.artifact() != "member-key" {
            return Err(Error::Context);
        }
        let time = rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_millis(
            now.ok_or(Error::Context)?,
        ));
        record
            .verify(expected, &root, time)
            .map_err(|_| Error::Context)?;
        member
            .validate(
                expected,
                installation,
                &root,
                record.leaf(),
                now,
                &owner.plan.credentials,
                &owner.plan.state,
                &owner.plan.workspace,
            )
            .map_err(|_| Error::Context)?;
        member
            .validate_bytes(&key, expected, &root, record.leaf(), now)
            .map_err(|_| Error::Context)
    }
    /// No Published capability escapes this function, only a store-only unit result.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::cluster) fn store(
        owner: &mut Owner,
        expected: Option<&ExpectedIdentity>,
        installation: [u8; 16],
        root: &[u8],
        leaf: &[u8],
        member: &MemberCopy<'_>,
        now: Option<u64>,
    ) -> Result<(), Error> {
        match owner.publication.state {
            State::Begun if owner.publication.phase == Phase::Reserved => (),
            _ => {
                prior(owner)?;
                return Err(Error::IncompleteIssuance);
            }
        }
        // A panic anywhere below leaves Begun/Publishing, which cannot re-enter.
        owner.publication.phase = Phase::Publishing;
        let result = (|| {
            let expected = expected.ok_or(Error::Context)?;
            if owner.plan.mode.founder() != expected.origin.founder() {
                return Err(Error::Context);
            }
            let bytes = PublicIdentity::assemble(expected, leaf).map_err(|_| Error::Context)?;
            let record = PublicIdentity::decode(&bytes).map_err(|_| Error::Context)?;
            let ms = now.ok_or(Error::Context)?;
            let time =
                rustls::pki_types::UnixTime::since_unix_epoch(std::time::Duration::from_millis(ms));
            record
                .verify(expected, root, time)
                .map_err(|_| Error::Context)?;
            member
                .validate(
                    expected,
                    installation,
                    root,
                    leaf,
                    now,
                    &owner.plan.credentials,
                    &owner.plan.state,
                    &owner.plan.workspace,
                )
                .map_err(|_| Error::Context)?;
            inventory(owner, &[RESERVATION])?;
            let ca = create(owner, "ca.der", "ca-create")?;
            write(
                owner,
                ca,
                root,
                [
                    "ca-partial-write",
                    "ca-full-write",
                    "ca-file-sync",
                    "ca-parent-sync",
                ],
            )?;
            inventory(owner, &[RESERVATION, "ca.der"])?;
            let key = create(owner, "member-key", "member-create")?;
            write(
                owner,
                key,
                member.bytes(),
                [
                    "member-partial-write",
                    "member-full-write",
                    "member-file-sync",
                    "member-parent-sync",
                ],
            )?;
            inventory(owner, &[RESERVATION, "ca.der", "member-key"])?;
            let temp = create(owner, TEMP, "temp-create")?;
            write(
                owner,
                temp,
                &bytes,
                [
                    "temp-partial-write",
                    "temp-full-write",
                    "temp-file-sync",
                    "temp-parent-sync",
                ],
            )?;
            before(owner, "reservation-recheck")?;
            inventory(owner, &[RESERVATION, "ca.der", "member-key", TEMP])?;
            if !layout(owner.publication.files[0].read_at(0))?.is_empty()
                || layout(owner.publication.files[temp].read_at(8192))? != bytes
            {
                return Err(Error::Layout);
            }
            observed(owner, "reservation-recheck")?;
            member
                .validate(
                    expected,
                    installation,
                    root,
                    leaf,
                    now,
                    &owner.plan.credentials,
                    &owner.plan.state,
                    &owner.plan.workspace,
                )
                .map_err(|_| Error::Context)?;
            let dir = layout(owner.publication_directory())?;
            // SAFETY: fixed NUL-terminated names in the same retained directory.
            before(owner, "atomic-replace")?;
            if unsafe {
                libc::renameat(
                    dir.as_raw_fd(),
                    c".identity-publish".as_ptr(),
                    dir.as_raw_fd(),
                    c"identity.json".as_ptr(),
                )
            } != 0
            {
                return Err(Error::Unconfirmed);
            }
            owner.publication.files[temp].rename_to(owner.plan.credentials.join(RESERVATION));
            // Retire only the original reservation after successful atomic replacement.
            owner.publication.files.remove(0);
            observed(owner, "atomic-replace")?;
            sync(owner, None, "replace-parent-sync")?;
            before(owner, "final-inventory")?;
            inventory(owner, &[RESERVATION, "ca.der", "member-key"])?;
            observed(owner, "final-inventory")?;
            before(owner, "retained-readback")?;
            let actual_root = layout(owner.publication.files[0].read_at(4096))?;
            let actual_key = layout(owner.publication.files[1].read_at(16384))?;
            let actual_record = layout(owner.publication.files[2].read_at(8192))?;
            if actual_root != root || actual_key != member.bytes() || actual_record != bytes {
                return Err(Error::Context);
            }
            member
                .validate_bytes(&actual_key, expected, &actual_root, leaf, now)
                .map_err(|_| Error::Context)?;
            observed(owner, "retained-readback")?;
            before(owner, "guard-delta")?;
            observed(owner, "guard-delta")?;
            // All allocation and fallible work precedes these fixed moves.
            let mut delta = std::mem::take(&mut owner.publication.files);
            owner.publication_install(&mut delta);
            owner.publication.phase = Phase::Complete;
            owner.publication.state = State::Completed;
            Ok(())
        })();
        finish(owner, result)
    }
}
#[cfg(target_os = "linux")]
pub(super) use linux::{check_retained, store};

#[cfg(test)]
mod tests;
