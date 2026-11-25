//! Held evidence output. The directory is privately owned and claimed
//! exclusively before any record is written. Diagnostics and complete
//! evidence use distinct publication paths.
//!
//! The writer is the provenance boundary for every opaque value. It draws
//! the scope and run from its own random generator, derives the release,
//! the scope-keyed dependency digest and the record chain by hashing, and
//! the schema pins the distribution digests to the reviewed stock values.
//! A caller supplies only stages, times, results and summary fields.
//!
//! Every file is committed through a hidden temporary entry that is
//! written, flushed and renamed relative to the held directory, so an
//! interrupted write never leaves an entry under a final name.
//!
//! An evidence directory serves one run. Before a claim exists, a directory
//! holding only the sink's own temporary entries is debris of an interrupted
//! commit, and the claim removes those entries through the held handle.
//! Once a claim exists the directory belongs to that run whatever its
//! outcome, and a retry uses a fresh empty directory.

use super::*;
use crate::vm::qualification_record::{self as records, Fresh, Instant2, SealBundle, Selection};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::cell::Cell;
use windows_sys::Wdk::Storage::FileSystem::{
    FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
};

/// The fixed diagnostic outputs. A completion marker does not exist here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiagnosticFile {
    Inputs,
    HostBaseline,
    Distribution,
    CustodyJobCapacity,
    ActivityReserves,
    WhpxQmpEndpoint,
    NetworkRestrictedTest,
    RecipeUbuntu,
    RecipeWindows,
    NetworkRestrictedOrigin,
    RecipeStaticWeb,
    NetworkPublicIpv4,
    RecipeDocker,
    Cleanup,
    /// The run summary. It is written last and closes the sink.
    Incomplete,
}

impl DiagnosticFile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Inputs => "inputs.json",
            Self::HostBaseline => "host-baseline.json",
            Self::Distribution => "distribution.json",
            Self::CustodyJobCapacity => "custody-job-capacity.json",
            Self::ActivityReserves => "activity-reserves.json",
            Self::WhpxQmpEndpoint => "whpx-qmp-endpoint.json",
            Self::NetworkRestrictedTest => "network-restricted-test.json",
            Self::RecipeUbuntu => "recipe-ubuntu.json",
            Self::RecipeWindows => "recipe-windows.json",
            Self::NetworkRestrictedOrigin => "network-restricted-origin.json",
            Self::RecipeStaticWeb => "recipe-static-web.json",
            Self::NetworkPublicIpv4 => "network-public-ipv4.json",
            Self::RecipeDocker => "recipe-docker.json",
            Self::Cleanup => "cleanup.json",
            Self::Incomplete => "incomplete.json",
        }
    }
}

const CLAIM: &str = "claim.json";
const COMPLETE: &str = "qualification.json";
const CLAIM_BYTES: &[u8] = br#"{"claim":"arx-vm-qualify"}"#;
const MAX_OUTPUT: usize = 16 * 1024;
/// Seven days, the maximum qualification validity.
const VALIDITY_SECS: u64 = 7 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SinkError {
    Closed,
    Invalid,
    Private,
    Io,
}

/// The independently collected live projection. Custody stays with the
/// caller and the capture time precedes its first check.
pub(super) struct PublishSample<'a> {
    pub(super) dependencies: &'a records::Dependencies,
    pub(super) live_captured_ms: u64,
    pub(super) settled: bool,
}

impl<'a> PublishSample<'a> {
    fn check(
        &self,
        now: Instant2,
        issued: Instant2,
        binding: records::Binding,
        finished_utc: u64,
    ) -> Result<Fresh<'a>, SinkError> {
        if !self.settled
            || self.live_captured_ms < issued.mono_ms
            || now.mono_ms > records::MAX_TRIAL_MS
            || now
                .mono_ms
                .checked_sub(self.live_captured_ms)
                .is_none_or(|age| age > records::LIVE_FRESHNESS_MS)
            || !records::clocks_agree(issued, now)
            || !binding.live_at(now.utc, self.dependencies.key(binding.scope))
            || now.utc < finished_utc
        {
            return Err(SinkError::Invalid);
        }
        Ok(Fresh {
            now: binding.boot.on_boot(now).ok_or(SinkError::Invalid)?,
            boot_marker: binding.boot.marker,
            dependencies: self.dependencies,
        })
    }
}

/// One record as a caller publishes it. The writer adds the contract,
/// origin, binding and chain digest; there is no field for any of them.
pub struct Entry<'a, St, R> {
    pub stage: &'a St,
    pub started_ms: u64,
    pub finished_ms: u64,
    pub result: &'a R,
}

/// The two steps between creating a temporary entry and renaming it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Write,
    Flush,
}

#[cfg(test)]
thread_local! {
    /// Test-only: fail one commit step of one file name on this thread.
    static FAULT: Cell<Option<(&'static str, Step)>> = const { Cell::new(None) };
    /// Test-only: end the whole process at one commit step, as a kill would.
    static EXIT: Cell<Option<(&'static str, Step)>> = const { Cell::new(None) };
    /// Test-only: runs once between the debris listing and its removal.
    static AFTER_LISTING: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

fn fault(name: &str, step: Step) -> io::Result<()> {
    #[cfg(test)]
    if EXIT
        .with(Cell::get)
        .is_some_and(|(at, when)| at == name && when == step)
    {
        std::process::exit(3);
    }
    #[cfg(test)]
    if FAULT
        .with(Cell::get)
        .is_some_and(|(at, when)| at == name && when == step)
    {
        return Err(io::Error::other("injected commit fault"));
    }
    #[cfg(not(test))]
    let _ = (name, step);
    Ok(())
}

pub(super) struct EvidenceDir {
    pin: PathPin,
    canaries: Vec<Vec<u8>>,
    scope: [u8; 32],
    run: [u8; 16],
    boot: records::BootAnchor,
    closed: Cell<bool>,
}

fn json<T: Serialize + ?Sized>(value: &T) -> Result<String, SinkError> {
    serde_json::to_string(value).map_err(|_| SinkError::Invalid)
}

impl EvidenceDir {
    /// Hold the directory, require private ownership and emptiness through
    /// the held object, draw the run's opaque identifiers, then claim the
    /// directory with an exclusive commit. A concurrent runner loses the
    /// claim and writes nothing; an interrupted claim leaves no claim entry.
    pub(super) fn claim(
        pin: PathPin,
        owner: PSID,
        private_values: &[&str],
        boot: records::BootAnchor,
    ) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::PathCustodyUnverified;
        if !pin.mutable || !held_private(&pin.leaf, owner) {
            return Err(reason);
        }
        clear_debris(&pin.leaf).map_err(|_| reason)?;
        if !empty(&pin.leaf)? {
            return Err(reason);
        }
        let (mut scope, mut run) = ([0; 32], [0; 16]);
        getrandom::fill(&mut scope).map_err(|_| reason)?;
        getrandom::fill(&mut run).map_err(|_| reason)?;
        let dir = Self {
            pin,
            canaries: private_values
                .iter()
                .filter(|value| value.len() >= 3)
                .flat_map(|value| {
                    let lower = value.to_ascii_lowercase();
                    [
                        lower.clone().into_bytes(),
                        lower.replace('\\', "\\\\").into_bytes(),
                        lower.replace('\\', "/").into_bytes(),
                    ]
                })
                .collect(),
            scope,
            run,
            boot,
            closed: Cell::new(false),
        };
        dir.commit(CLAIM, CLAIM_BYTES).map_err(|_| reason)?;
        Ok(dir)
    }

    pub(super) fn scope(&self) -> [u8; 32] {
        self.scope
    }

    pub(super) fn run(&self) -> [u8; 16] {
        self.run
    }

    pub(super) fn boot_anchor(&self) -> records::BootAnchor {
        self.boot
    }

    /// Publish the run once: every record file in manifest order, then the
    /// summary last. The writer composes each record around the caller's
    /// stage, times and result, and chains it to the previous record in the
    /// caller's order. It returns the record files it could not commit,
    /// which the summary lists; a summary it cannot commit is an error.
    pub(super) fn publish<C, D, St, R, B>(
        &self,
        class: &C,
        dependencies: &D,
        issued_utc: u64,
        entries: &[Entry<'_, St, R>],
        summary: &B,
    ) -> Result<Vec<DiagnosticFile>, SinkError>
    where
        C: Serialize,
        D: Serialize,
        St: Serialize,
        R: Serialize,
        B: Serialize,
    {
        if self.closed.replace(true) {
            return Err(SinkError::Closed);
        }
        let binding = self.binding(class, dependencies, issued_utc)?;
        let mut grouped: Vec<Vec<Vec<u8>>> = vec![Vec::new(); super::schema::RECORD_KINDS.len()];
        let mut prior = [0u8; 32];
        let mut composed = true;
        for entry in entries {
            match record(&binding, entry, prior) {
                Some((index, bytes)) => {
                    prior = Sha256::digest(&bytes).into();
                    grouped[index].push(bytes);
                }
                None => composed = false,
            }
        }
        let mut unwritten = Vec::new();
        for (records, file) in grouped.iter().zip(super::schema::RECORD_KINDS) {
            if records.is_empty() {
                continue;
            }
            let joined = records.join(&b',');
            let bytes = [&b"["[..], joined.as_slice(), &b"]"[..]].concat();
            // An entry that cannot be composed breaks the chain, so no
            // record file is published.
            if !composed || self.commit_checked(file, &bytes).is_err() {
                unwritten.push(file);
            }
        }
        let names: Vec<&str> = unwritten.iter().map(|file| file.name()).collect();
        let body = json(summary)?;
        let fields = body
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
            .filter(|fields| !fields.is_empty())
            .ok_or(SinkError::Invalid)?;
        let bytes = format!(
            r#"{{"contract":{RECORD_CONTRACT},"origin":"native","binding":{binding},{fields},"unwritten":{}}}"#,
            json(&names)?
        );
        self.commit_checked(DiagnosticFile::Incomplete, bytes.as_bytes())?;
        if composed {
            Ok(unwritten)
        } else {
            Err(SinkError::Invalid)
        }
    }

    /// Native-only publication. The closed trial must supply its complete
    /// chain and a fresh local observation. Once claimed, a partially written
    /// run is consumed and lacks a selection marker.
    pub(super) fn publish_complete<'a>(
        &self,
        bundle: &SealBundle,
        issued: Instant2,
        cancel: &crate::service::Shutdown,
        clock: impl Fn() -> Instant2,
        mut refresh: impl FnMut() -> Result<PublishSample<'a>, SinkError>,
    ) -> Result<(), SinkError> {
        if self.closed.replace(true) {
            return Err(SinkError::Closed);
        }
        let SealBundle {
            seal: seal_bytes,
            files,
        } = bundle;
        let seal: records::Seal = records::decode(seal_bytes).map_err(|_| SinkError::Invalid)?;
        if seal.binding.run != self.run
            || seal.binding.scope != self.scope
            || seal.binding.boot != self.boot
            || seal.binding.issued_mono_ms != issued.mono_ms
            || seal.binding.issued_utc != issued.utc
        {
            return Err(SinkError::Invalid);
        }
        let selection = Selection {
            seal_sha256: records::digest(seal_bytes),
            scope: self.scope,
        };
        let check = |sample: &PublishSample<'a>| {
            if cancel.is_cancelled() {
                return Err(SinkError::Invalid);
            }
            sample.check(clock(), issued, seal.binding, seal.finished_utc)
        };
        records::verify_native(seal_bytes, files, &selection, &check(&refresh()?)?)
            .map_err(|_| SinkError::Invalid)?;
        if self.leaks(seal_bytes) || files.iter().any(|(_, bytes)| self.leaks(bytes)) {
            return Err(SinkError::Private);
        }
        for (kind, bytes) in files {
            self.commit(&kind.name(), bytes)
                .map_err(|_| SinkError::Io)?;
        }
        // Reopen by the held directory, and hold each exact row without
        // write/delete sharing across verification and the final rename.
        let mut held = Vec::with_capacity(files.len());
        records::read_native(
            seal_bytes.as_slice(),
            seal_bytes.len() as u64,
            |kind| {
                let file = open_relative(&self.pin.leaf, &kind.name(), Node::File, false, false)
                    .map_err(|_| records::RunError::Output)?;
                file_identity(&file, false).map_err(|_| records::RunError::Output)?;
                held.push(file.try_clone().map_err(|_| records::RunError::Output)?);
                Ok(file)
            },
            &selection,
            &check(&refresh()?)?,
        )
        .map_err(|_| SinkError::Invalid)?;
        self.commit_checked_at_rename(COMPLETE, seal_bytes, || {
            let stale = |_| io::Error::other("qualification freshness unavailable");
            let sample = refresh().map_err(stale)?;
            records::verify_native(
                seal_bytes,
                files,
                &selection,
                &check(&sample).map_err(stale)?,
            )
            .map_err(|_| io::Error::other("qualification invalid before publication"))?;
            check(&sample).map_err(stale)?;
            Ok(())
        })
        .map_err(|_| SinkError::Io)?;
        drop(held);
        Ok(())
    }

    /// The binding every published file carries. The scope and run are the
    /// writer's own; the dependency digest is keyed by the random scope, so
    /// the private identities in the projection cannot be recovered.
    fn binding<C: Serialize, D: Serialize>(
        &self,
        class: &C,
        dependencies: &D,
        issued_utc: u64,
    ) -> Result<String, SinkError> {
        let keyed: [u8; 32] = Sha256::digest(json(&(&self.scope, dependencies))?).into();
        Ok(format!(
            r#"{{"class":{},"scope":{},"release":{},"run":{},"dependencies":{},"boot":{},"issued_mono_ms":0,"issued_utc":{issued_utc},"expires_utc":{}}}"#,
            json(class)?,
            json(&self.scope)?,
            json(&super::schema::release())?,
            json(&self.run)?,
            json(&keyed)?,
            json(&self.boot)?,
            issued_utc.saturating_add(VALIDITY_SECS),
        ))
    }

    /// Bounded, one canonical value of the file's closed schema, free of raw
    /// private values, then committed under its final name.
    fn commit_checked(&self, file: DiagnosticFile, bytes: &[u8]) -> Result<(), SinkError> {
        if !(1..=MAX_OUTPUT).contains(&bytes.len()) || !super::schema::check(file, bytes) {
            return Err(SinkError::Invalid);
        }
        if self.leaks(bytes) {
            return Err(SinkError::Private);
        }
        self.commit(file.name(), bytes).map_err(|_| SinkError::Io)
    }

    fn leaks(&self, bytes: &[u8]) -> bool {
        let lower = bytes.to_ascii_lowercase();
        let contains = |needle: &[u8]| lower.windows(needle.len()).any(|w| w == needle);
        contains(b"s-1-")
            || contains(b":\\\\")
            || contains(b":/")
            || self.canaries.iter().any(|canary| contains(canary))
    }

    /// Write a hidden temporary entry, flush it and rename it to `name`
    /// relative to the held directory. On any failure the temporary entry is
    /// deleted with its handle and no final name appears.
    fn commit(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        self.commit_checked_at_rename(name, bytes, || Ok(()))
    }

    fn commit_checked_at_rename(
        &self,
        name: &str,
        bytes: &[u8],
        final_check: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let temporary = create_hidden(&self.pin.leaf, &temporary_name(name))?;
        let committed = finish_commit(&temporary, &self.pin.leaf, name, bytes, final_check);
        if committed.is_err() {
            let _ = discard(&temporary);
        }
        committed
    }
}

/// One record around the caller's stage, times and result, and the index of
/// the record file that holds its stage.
fn record<St: Serialize, R: Serialize>(
    binding: &str,
    entry: &Entry<'_, St, R>,
    prior: [u8; 32],
) -> Option<(usize, Vec<u8>)> {
    let stage = serde_json::to_string(entry.stage).ok()?;
    let index = super::schema::record_file(&stage)?;
    let record = format!(
        r#"{{"contract":{RECORD_CONTRACT},"collector":{RECORD_COLLECTOR},"origin":"native","binding":{binding},"stage":{stage},"started_ms":{},"finished_ms":{},"prior":{},"result":{}}}"#,
        entry.started_ms,
        entry.finished_ms,
        serde_json::to_string(&prior).ok()?,
        serde_json::to_string(entry.result).ok()?,
    );
    Some((index, record.into_bytes()))
}

fn finish_commit(
    file: &File,
    dir: &File,
    name: &str,
    bytes: &[u8],
    final_check: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    use std::io::Write;
    let mut writer = file;
    if writer.write(bytes)? != bytes.len() {
        return Err(io::Error::other("short evidence write"));
    }
    fault(name, Step::Write)?;
    // The final entry is an ordinary file; only the temporary was hidden.
    let basic = FILE_BASIC_INFO {
        FileAttributes: FILE_ATTRIBUTE_NORMAL,
        ..Default::default()
    };
    // Zero timestamps leave the times unchanged.
    succeeded(unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileBasicInfo,
            std::ptr::from_ref(&basic).cast(),
            size_of_val(&basic) as u32,
        )
    })?;
    file.sync_all()?;
    fault(name, Step::Flush)?;
    final_check()?;
    rename_within(file, dir, name)
}

/// The sink's own temporary entry for a final name.
fn temporary_name(name: &str) -> String {
    format!(".{name}.partial")
}

/// Exactly one of the sink's fixed temporary names: the claim's or a
/// diagnostic file's.
fn is_temporary(name: &str) -> bool {
    std::iter::once(CLAIM)
        .chain(super::schema::RECORD_KINDS.iter().map(|file| file.name()))
        .chain([DiagnosticFile::Incomplete.name()])
        .any(|final_name| name == temporary_name(final_name))
}

/// Before a claim exists, a directory whose every entry is a regular file
/// with one of the sink's fixed temporary names holds only the debris of an
/// interrupted commit. Each entry is opened exclusively relative to the
/// held handle and deleted through that handle. Anything else is left
/// untouched, and the emptiness check then refuses the claim.
fn clear_debris(dir: &File) -> io::Result<()> {
    let entries = listing(dir)?;
    let debris = entries.iter().all(|(name, attributes)| {
        is_temporary(name)
            && attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) == 0
    });
    if !debris {
        return Ok(());
    }
    #[cfg(test)]
    AFTER_LISTING.with(|hook| {
        if let Some(mut run) = hook.borrow_mut().take() {
            run();
        }
    });
    for (name, _) in &entries {
        // An entry another process still holds open refuses the exclusive
        // open, so a live commit is never disturbed.
        let entry = open_child(dir, name, DELETE | SYNCHRONIZE, 0, FILE_OPEN)?;
        // A run creates its temporary entries only after its claim.json is
        // committed, and final names are never removed. So while this
        // exclusive handle is held, the absence of claim.json under the held
        // directory proves the entry is pre-claim debris that no run owns,
        // however stale the listing above is. Holding the entry also stalls
        // a concurrent claimant's own cleanup and emptiness check. If a claim
        // exists, the handle closes without deletion and the claim refuses.
        if claim_exists(dir)? {
            return Err(io::Error::other("a claim exists beside the debris"));
        }
        discard(&entry)?;
    }
    Ok(())
}

/// Whether claim.json exists under the held directory, looked up relative to
/// its handle. The probe shares everything, so it never conflicts with
/// another open; any outcome other than found or not found is an error.
fn claim_exists(dir: &File) -> io::Result<bool> {
    let probe = nt_open(
        dir,
        CLAIM,
        FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        0,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        FILE_OPEN,
        FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
    );
    match probe {
        Ok(_) => Ok(true),
        Err(error) if error.raw_os_error() == Some(ERROR_FILE_NOT_FOUND as i32) => Ok(false),
        Err(error) => Err(error),
    }
}

/// Exclusively create a hidden entry under the held directory, with the
/// delete access its rename or disposal needs.
pub(super) fn create_hidden(dir: &File, name: &str) -> io::Result<File> {
    open_child(
        dir,
        name,
        FILE_GENERIC_WRITE | DELETE,
        FILE_ATTRIBUTE_HIDDEN,
        FILE_CREATE,
    )
}

/// Open or create one regular, non-reparse entry relative to the held
/// directory, with no sharing.
fn open_child(
    dir: &File,
    name: &str,
    access: u32,
    attributes_on_create: u32,
    disposition: u32,
) -> io::Result<File> {
    nt_open(
        dir,
        name,
        access,
        attributes_on_create,
        0,
        disposition,
        FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
    )
}

/// One relative open under the held directory that never follows a reparse
/// point in the name.
fn nt_open(
    dir: &File,
    name: &str,
    access: u32,
    attributes_on_create: u32,
    share: u32,
    disposition: u32,
    options: u32,
) -> io::Result<File> {
    let mut units: Vec<u16> = name.encode_utf16().collect();
    let bytes = u16::try_from(units.len() * 2).map_err(io::Error::other)?;
    let object = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: units.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: dir.as_raw_handle(),
        ObjectName: &object,
        Attributes: OBJ_CASE_INSENSITIVE | OBJ_DONT_REPARSE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let mut handle = null_mut();
    // Zero is a valid initial state for this output-only status block.
    let mut status_block: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // The name buffer, attributes and parent handle outlive this call.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut status_block,
            null(),
            attributes_on_create,
            share,
            disposition,
            options,
            null(),
            0,
        )
    };
    if status < 0 {
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    // NtCreateFile succeeded and transferred a fresh handle.
    Ok(File::from(unsafe { owned(handle) }?))
}

/// Rename the open entry to `name` under the held directory. An existing
/// final name is never replaced.
fn rename_within(file: &File, dir: &File, name: &str) -> io::Result<()> {
    let units: Vec<u16> = name.encode_utf16().collect();
    let length = units.len() * 2;
    let size = size_of::<FILE_RENAME_INFORMATION>() + length;
    let mut buffer = vec![0u64; size.div_ceil(8)];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // Zero is a valid initial state for this output-only status block.
    let mut status_block: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // The zeroed, aligned buffer holds the header and the trailing name.
    let status = unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = dir.as_raw_handle();
        (*info).FileNameLength = length as u32;
        std::ptr::copy_nonoverlapping(
            units.as_ptr(),
            std::ptr::addr_of_mut!((*info).FileName).cast::<u16>(),
            units.len(),
        );
        NtSetInformationFile(
            file.as_raw_handle(),
            &mut status_block,
            info.cast(),
            size as u32,
            FileRenameInformation,
        )
    };
    if status < 0 {
        let code = unsafe { RtlNtStatusToDosError(status) };
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    Ok(())
}

/// Mark the open entry for deletion; it disappears when its handle closes.
pub(super) fn discard(file: &File) -> io::Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // A fixed-size input structure for the matching information class.
    succeeded(unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            std::ptr::from_ref(&disposition).cast(),
            size_of_val(&disposition) as u32,
        )
    })
}

pub(super) fn discard_owned(dir: &File, name: &str, expected: FileIdentity) -> io::Result<()> {
    let file = open_child(dir, name, FILE_GENERIC_READ | DELETE, 0, FILE_OPEN)?;
    if file_identity(&file, false)?.0 != expected {
        return Err(io::Error::other("owned entry changed"));
    }
    discard(&file)
}

/// The held directory has no entries, enumerated through its own handle.
fn empty(dir: &File) -> Result<bool, NativeRefusal> {
    listing(dir)
        .map(|entries| entries.is_empty())
        .map_err(|_| NativeRefusal::PathCustodyUnverified)
}

/// Owner and DACL of the held object itself: owned by the expected user,
/// with allow entries only for that user, SYSTEM and Administrators.
pub(super) fn held_private(file: &File, owner: PSID) -> bool {
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    let mut actual_owner = null_mut();
    let mut dacl = null_mut();
    let mut descriptor = null_mut();
    // The descriptor owns the returned owner and DACL views.
    let status = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut actual_owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return false;
    }
    // Windows-owned SID and ACL storage stays live until LocalFree below.
    let private = unsafe { private_descriptor(actual_owner, dacl, owner) };
    unsafe { LocalFree(descriptor) };
    private
}

/// The owner/DACL decision. All pointers must be null or valid for the call.
unsafe fn private_descriptor(actual_owner: PSID, dacl: *const ACL, owner: PSID) -> bool {
    // The caller keeps the SIDs and the ACL alive for this call.
    unsafe {
        !actual_owner.is_null()
            && EqualSid(actual_owner, owner) != 0
            && !dacl.is_null()
            && (0..u32::from((*dacl).AceCount)).all(|index| {
                let mut ace = null_mut();
                if GetAce(dacl, index, &mut ace) == 0 {
                    return false;
                }
                let header = &*ace.cast::<ACE_HEADER>();
                match header.AceType {
                    // A deny entry grants nothing.
                    1 => true,
                    0 => {
                        let allow = &*ace.cast::<ACCESS_ALLOWED_ACE>();
                        let sid = std::ptr::addr_of!(allow.SidStart).cast_mut().cast();
                        EqualSid(sid, owner) != 0
                            || IsWellKnownSid(sid, WinLocalSystemSid) != 0
                            || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0
                    }
                    _ => false,
                }
            })
    }
}

#[cfg(test)]
pub(super) mod tests {
    #[path = "foreground_tests.rs"]
    mod foreground;
    const TEST_BOOT: records::BootAnchor = records::BootAnchor {
        marker: [1; 32],
        origin_mono_ms: 100_000_000,
    };
    use super::*;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;

    fn user() -> (Vec<usize>, String) {
        let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).unwrap();
        let data = token_data(&token, TokenUser).unwrap();
        let sid = unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut text = null_mut();
        assert_ne!(
            unsafe {
                windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW(sid, &mut text)
            },
            0
        );
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let string = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).unwrap();
        unsafe { LocalFree(text.cast()) };
        (data, string)
    }

    fn sid(data: &[usize]) -> PSID {
        unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid }
    }

    /// A fresh directory with an explicit owner and protected DACL.
    fn directory(parent: &Path, name: &str, sddl: &str) -> LocalPath {
        let path = parent.join(name);
        let mut descriptor = null_mut();
        let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    wide_sddl.as_ptr(),
                    1,
                    &mut descriptor,
                    null_mut(),
                )
            },
            0
        );
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let ok = unsafe { CreateDirectoryW(wide(&path).as_ptr(), &attributes) };
        unsafe { LocalFree(descriptor) };
        assert_ne!(ok, 0, "{}", io::Error::last_os_error());
        LocalPath::parse(path.to_str().unwrap()).unwrap()
    }

    fn private_dir(parent: &Path, name: &str) -> (Vec<usize>, LocalPath) {
        let (data, text) = user();
        let path = directory(parent, name, &format!("O:{text}D:P(A;OICI;FA;;;{text})"));
        (data, path)
    }

    fn names(path: &LocalPath) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path.as_str())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_refusal_at_the_last_rename_never_exposes_the_marker() {
        let scratch = tempfile::tempdir().unwrap();
        let (data, path) = private_dir(scratch.path(), "evidence");
        let pin = PathPin::open(&path, true, true).unwrap();
        let sink = EvidenceDir::claim(pin, sid(&data), &[], TEST_BOOT).unwrap();
        let called = Cell::new(false);
        let refused = sink.commit_checked_at_rename(COMPLETE, b"{}", || {
            called.set(true);
            assert!(
                !names(&path).contains(&COMPLETE.to_owned()),
                "marker must never be renamed into place before final_check runs"
            );
            Err(io::ErrorKind::TimedOut.into())
        });
        assert!(called.get());
        assert_eq!(refused.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert_eq!(names(&path), [CLAIM]);
    }

    const INPUTS: &str = r#"{"passed":{"kind":"input_pins","held_roots":5,"enrollment_private":true,"evidence_private":true,"trial_root_private":true,"evidence_claimed":true}}"#;

    fn raw(text: &str) -> Box<serde_json::value::RawValue> {
        serde_json::value::RawValue::from_string(text.to_owned()).unwrap()
    }

    /// The summary fields a caller supplies, in schema order.
    #[derive(Serialize)]
    struct Body {
        blocked_at: Option<&'static str>,
        unattempted: Vec<&'static str>,
        unmeasured: Vec<&'static str>,
        dispatched_effects: u16,
        charge_retained: bool,
        late: Option<&'static str>,
    }

    fn body() -> Body {
        Body {
            blocked_at: Some("disposable_store"),
            unattempted: vec!["suspended_child"],
            unmeasured: vec!["artifact"],
            dispatched_effects: 0,
            charge_retained: false,
            late: None,
        }
    }

    type Results = [(&'static str, Box<serde_json::value::RawValue>)];

    fn entries(
        results: &Results,
    ) -> Vec<Entry<'_, &'static str, Box<serde_json::value::RawValue>>> {
        results
            .iter()
            .map(|(stage, result)| Entry {
                stage,
                started_ms: 1,
                finished_ms: 2,
                result,
            })
            .collect()
    }

    /// A claimed private directory whose held handle shares write, as the
    /// preflight holds it.
    fn claimed(parent: &Path, canaries: &[&str]) -> (LocalPath, EvidenceDir) {
        let (owner, path) = private_dir(parent, "evidence");
        let dir = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            canaries,
            TEST_BOOT,
        )
        .unwrap();
        (path, dir)
    }

    fn read(path: &LocalPath, name: &str) -> Vec<u8> {
        std::fs::read(format!(r"{}\{name}", path.as_str())).unwrap()
    }

    fn value(path: &LocalPath, name: &str) -> serde_json::Value {
        serde_json::from_slice(&read(path, name)).unwrap()
    }

    #[test]
    fn live_network_socket_observations_publish_as_incomplete_only() {
        use records::{
            NetworkProbeReport, Origin, ProbeOutcome, RestrictedNetworkDiagnostic, Stage,
            StageResult,
        };
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (path, dir) = claimed(root.path(), &[]);
        let report = RestrictedNetworkDiagnostic {
            guest: NetworkProbeReport {
                nic_ready: true,
                ipv4_lease: true,
                default_route: true,
                no_ipv6_egress_route: true,
                outcomes: [ProbeOutcome::SocketError; 8],
            },
            host_controls_post: true,
            clean_guest_shutdown: true,
            seed_charge_released: true,
        };
        let result = StageResult::NetworkDiagnostic(report);
        assert!(!result.passes(Stage::RestrictedTestNetwork, Origin::Native));
        let wire = serde_json::to_string(&result).unwrap();
        let publish = |dir: &EvidenceDir, stage, result: &str| {
            dir.publish(
                &"test",
                &"projection",
                1_800_000_000,
                &entries(&[(stage, raw(result))]),
                &Body {
                    blocked_at: Some(stage),
                    ..body()
                },
            )
        };
        assert_eq!(publish(&dir, "restricted_test_network", &wire), Ok(vec![]));
        let file = DiagnosticFile::NetworkRestrictedTest;
        let bytes = read(&path, file.name());
        let rows: Vec<records::Record> = records::decode(&bytes).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stage, Stage::RestrictedTestNetwork);
        assert_eq!(rows[0].result, result);
        assert_eq!(
            names(&path),
            [
                "claim.json",
                "incomplete.json",
                "network-restricted-test.json"
            ]
        );
        let check = |bytes: String| super::super::schema::check(file, bytes.as_bytes());
        let row = String::from_utf8(bytes).unwrap();
        assert!(!check(row.replacen(",\"socket_error\"", "", 1)));
        assert!(!check(row.replacen(
            "\"socket_error\"",
            "\"socket_error\",\"socket_error\"",
            1
        )));
        assert!(!check(row.replacen("\"socket_error\"", "\"denied\"", 1)));
        assert!(!check(row.replacen(
            "\"network_diagnostic\"",
            "\"passed\"",
            1
        )));

        let other = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (other_path, other_dir) = claimed(other.path(), &[]);
        assert_eq!(
            publish(&other_dir, "qmp_channel", &wire),
            Ok(vec![DiagnosticFile::WhpxQmpEndpoint]),
        );
        assert_eq!(names(&other_path), ["claim.json", "incomplete.json"]);
    }

    pub(crate) fn with_refused_issuer_dossier(
        stale: bool,
        run: impl FnOnce(LocalPath, Selection, &dyn Fn(&str, &Selection) -> Result<(), NativeRefusal>),
    ) {
        use super::super::foreground::ReadDossier;
        let work = tempfile::tempdir().unwrap();
        let (path, dir) = claimed(work.path(), &[]);
        let (dependencies, _, mut bundle) = complete_test_fixture(&dir);
        if !stale {
            let mut seal: records::Seal = serde_json::from_slice(&bundle.seal).unwrap();
            seal.origin = records::Origin::Fixture;
            bundle.seal = serde_json::to_vec(&seal).unwrap();
        }
        for (kind, bytes) in &bundle.files {
            dir.commit(&kind.name(), bytes).unwrap();
        }
        dir.commit(COMPLETE, &bundle.seal).unwrap();
        let selection = Selection {
            seal_sha256: records::digest(&bundle.seal),
            scope: dir.scope(),
        };
        let mut now = TEST_BOOT.on_boot(NOW).unwrap();
        if stale {
            now.utc += VALIDITY_SECS + 1;
            now.mono_ms += (VALIDITY_SECS + 1) * 1000;
        }
        let fresh = Fresh {
            now,
            boot_marker: TEST_BOOT.marker,
            dependencies: &dependencies,
        };
        let (owner, _) = user();
        run(path.clone(), selection, &|name, pin| {
            assert_eq!(name, path.as_str());
            let dir = ReadDossier::open(PathPin::open(&path, true, false)?, sid(&owner))?;
            dir.read_selected(pin, &fresh)
                .map(|_| ())
                .map_err(|_| NativeRefusal::QualificationMissing)
        });
    }

    fn complete_test_fixture(
        dir: &EvidenceDir,
    ) -> (records::Dependencies, records::TestRecipes, SealBundle) {
        use records::*;
        let recipes = TestRecipes {
            ubuntu: RecipeIdentity {
                recipe_id: [7; 16],
                recipe_sha256: [7; 32],
            },
            windows: RecipeIdentity {
                recipe_id: crate::vm::template::windows_recipe_id(),
                recipe_sha256: [8; 32],
            },
        };
        let deps = Dependencies {
            release: release(),
            artifact: Measured::Measured([5; 32]),
            host: Measured::Measured(HostDependency {
                windows_build: 26_100,
                windows_revision: 1,
                machine: [1; 16],
            }),
            store: Measured::Measured(StoreDependency {
                trial_volume: 1,
                trial_volume_root: [1; 16],
                trial_volume_guid: [2; 16],
                trial_root: [3; 16],
                outer_volume: 2,
                outer_volume_root: [4; 16],
                outer_volume_guid: [5; 16],
                backing_file: [6; 16],
                vhd_virtual_bytes: GIB,
                inner_extent_bytes: GIB,
                test_scope: Some([6; 32]),
            }),
            owner_profile: Measured::Measured([7; 32]),
            distribution: Measured::Measured(DistributionDependency {
                receipt: crate::vm::public_egress::STOCK_RECEIPT_1113_SHA256,
                manifest: stock_manifest(),
            }),
            install_media: Measured::Measured([8; 32]),
            recipes: Measured::Measured(recipes.key()),
            firmware: Measured::Measured([10; 32]),
            guest_protocol: Measured::Measured(1),
            network_topology: Measured::Measured([11; 32]),
            policy: PolicyDependency::CURRENT,
        };
        let binding = Binding {
            class: Class::Test,
            scope: dir.scope(),
            release: release(),
            run: dir.run(),
            dependencies: deps.key(dir.scope()),
            boot: TEST_BOOT,
            issued_mono_ms: ISSUE.mono_ms,
            issued_utc: ISSUE.utc,
            expires_utc: ISSUE.utc + MAX_VALIDITY,
        };
        let mut prior = [0; 32];
        let chain: Vec<_> = stages(Class::Test)
            .iter()
            .copied()
            .enumerate()
            .map(|(i, stage)| {
                let start = i as u64 * 100;
                let result = StageResult::Passed(match stage {
                    Stage::Inputs => Observation::InputPins {
                        held_roots: HELD_ROOTS,
                        enrollment_private: true,
                        evidence_private: true,
                        trial_root_private: true,
                        evidence_claimed: true,
                    },
                    Stage::HostBaseline => Observation::Host {
                        windows_build: 26_100,
                        windows_revision: 1,
                    },
                    Stage::Distribution => Observation::Distribution {
                        receipt: crate::vm::public_egress::STOCK_RECEIPT_1113_SHA256,
                        receipt_bytes: STOCK_RECEIPT_BYTES,
                        manifest: stock_manifest(),
                        manifest_bytes: STOCK_MANIFEST_BYTES,
                        payload_files: STOCK_PAYLOAD_FILES,
                        payload_bytes: 1,
                    },
                    Stage::CustodyJobCapacityBaseline => Observation::Capacity {
                        inner_caller_free_bytes: GIB,
                        inner_total_free_bytes: GIB,
                        inner_total_bytes: GIB,
                        inner_extent_bytes: GIB,
                        vhd_virtual_bytes: GIB,
                        outer_caller_free_bytes: OUTER_FLOOR,
                        outer_total_free_bytes: OUTER_FLOOR,
                        backing_allocated_bytes: GIB,
                        backing_length_bytes: GIB,
                        disk_ceiling_bytes: DISK_CEILING,
                        outer_floor_bytes: OUTER_FLOOR,
                        disk_budget_bytes: GIB,
                        empty_job_memory_limit_bytes: JOB_MEMORY_BYTES,
                        empty_job_cpu_rate: JOB_CPU_RATE,
                    },
                    Stage::ActivityReservesBaseline => Observation::Activity {
                        console_idle: true,
                        active_cpus: 8,
                        available_ram_bytes: 32 * GIB,
                        owner_ram_reserve_bytes: RAM_RESERVE,
                        owner_cpu_reserve: 4,
                    },
                    Stage::WhpxCapability => Observation::Whpx { present: true },
                    Stage::DisposableStore => Observation::PausedStore { validated: true },
                    Stage::SuspendedChild
                    | Stage::QemuPrelaunchPause
                    | Stage::HeldBundleInventory => Observation::PausedTrial { verified: true },
                    Stage::FirstGuestExecution => Observation::FirmwareSec {
                        sec_marker_verified: true,
                        prelaunch_quiet: true,
                        job_memory_limit_bytes: JOB_MEMORY_BYTES,
                        job_cpu_rate: JOB_CPU_RATE,
                        clean_release: true,
                    },
                    Stage::SupervisorLossRelease => Observation::SupervisorLoss {
                        job_memory_limit_bytes: JOB_MEMORY_BYTES,
                        job_cpu_rate: JOB_CPU_RATE,
                        active_process_limit: 1,
                        child_exited: true,
                        job_empty_notified: true,
                        dirty_release: true,
                    },
                    Stage::CapacityTrials => Observation::CapacityExercised {
                        trial_allocated_bytes: 1 << 20,
                        recipe_allocated_bytes: 1 << 20,
                        denial_checks: 5,
                        overlap_excluded: true,
                        aggregate_cap_checked: true,
                        trial_released: true,
                        recipe_released: true,
                    },
                    Stage::ReserveActivityTrials => Observation::ReserveActivity {
                        controlled_faults: Box::new(std::array::from_fn(|i| FaultRecord {
                            fault: ControlledFault::ALL[i],
                            sampled_ms: start + i as u64 * 2,
                            settled_ms: start + i as u64 * 2 + 1,
                            console_idle: true,
                            effective_cpus: 8,
                            available_ram_bytes: 32 * GIB,
                            inner_free_bytes: GIB,
                            outer_free_bytes: OUTER_FLOOR,
                        })),
                        console_transitions_ms: [10, 11, 12, 13, 14].map(|n| start + n),
                    },
                    Stage::QmpChannel => Observation::QmpFirmware {
                        greeting_verified: true,
                        whpx_enabled: true,
                        quit_clean_eof: true,
                        clean_release: true,
                    },
                    Stage::RestrictedTestNetwork => Observation::RestrictedNetworkWitness {
                        ipv4_and_ipv6_setup: true,
                        reachable_gateway_control: true,
                        validated_attempts: 8,
                        witness_receipts: 0,
                        witnesses_settled: true,
                        clean_guest_shutdown: true,
                        seed_charge_released: true,
                    },
                    Stage::UbuntuRecipe => Observation::UbuntuRecipeVerified {
                        recipe: recipes.ubuntu,
                        offline_installer: true,
                        pinned_seed: true,
                        detached_health: true,
                        pair_unchanged: true,
                        installed_isolation: true,
                    },
                    Stage::WindowsRecipe => Observation::WindowsRecipeVerified {
                        recipe: recipes.windows,
                        offline_setup_and_boot_next: true,
                        signed_drivers: true,
                        installed_readiness: true,
                        disk_only_finalize: true,
                        registry_generalized: true,
                        state_ini_generalized: true,
                        media_free_health: true,
                        distinct_clone_identities: true,
                        pair_unchanged: true,
                        installed_isolation: true,
                    },
                    Stage::UbuntuInstalledEndpoint | Stage::WindowsInstalledEndpoint => {
                        let ubuntu = stage == Stage::UbuntuInstalledEndpoint;
                        Observation::TestInstalledEndpoint {
                            recipe: if ubuntu {
                                recipes.ubuntu
                            } else {
                                recipes.windows
                            },
                            boot_identity: [if ubuntu { 1 } else { 2 }; 16],
                            live_job_and_same_peer: true,
                            nonce_bound_script: true,
                            script_contained: true,
                            clean_stop: true,
                        }
                    }
                    Stage::Cleanup => Observation::Cleanup {
                        dispatched_effects: 12,
                        released: true,
                        charge_retained: false,
                        late: false,
                    },
                    _ => unreachable!("TEST stage"),
                });
                let row = Record {
                    contract: CONTRACT,
                    collector: COLLECTOR,
                    origin: Origin::Native,
                    binding,
                    stage,
                    started_ms: start,
                    finished_ms: start + 16,
                    prior,
                    result,
                };
                prior = digest(&canonical(&row).unwrap());
                row
            })
            .collect();
        let bundle = produce_bundle(Origin::Native, binding, &chain, NOW.utc).unwrap();
        (deps, recipes, bundle)
    }

    const ISSUE: Instant2 = Instant2 {
        utc: 1_800_000_000,
        mono_ms: 0,
    };
    const NOW: Instant2 = Instant2 {
        utc: ISSUE.utc + 3,
        mono_ms: 3000,
    };

    #[test]
    fn complete_native_bundle_round_trips_both_recipes_through_the_held_directory() {
        use records::{FileKind, Record};
        let work = tempfile::tempdir().unwrap();
        let (path, dir) = claimed(work.path(), &[]);
        let (deps, recipes, bundle) = complete_test_fixture(&dir);
        let calls = Cell::new(0);
        dir.publish_complete(
            &bundle,
            ISSUE,
            &crate::service::Shutdown::default(),
            || NOW,
            || {
                let next = calls.get() + 1;
                calls.set(next);
                assert!(!names(&path).contains(&COMPLETE.to_owned()));
                if next == 3 {
                    for (kind, bytes) in &bundle.files {
                        assert_eq!(read(&path, &kind.name()), *bytes);
                        let path = path.join(&kind.name()).unwrap();
                        assert!(
                            std::fs::OpenOptions::new()
                                .write(true)
                                .open(path.as_str())
                                .is_err()
                        );
                    }
                }
                Ok(PublishSample {
                    dependencies: &deps,
                    live_captured_ms: NOW.mono_ms,
                    settled: true,
                })
            },
        )
        .unwrap();
        assert!(deps.complete());
        assert_eq!(calls.get(), 3);
        let marker = read(&path, COMPLETE);
        assert_eq!(marker, bundle.seal);
        assert!(!names(&path).contains(&DiagnosticFile::Incomplete.name().to_owned()));
        let expected = [
            FileKind::Inputs,
            FileKind::HostBaseline,
            FileKind::Distribution,
            FileKind::CustodyJobCapacity,
            FileKind::ActivityReserves,
            FileKind::WhpxQmpEndpoint,
            FileKind::NetworkRestrictedTest,
            FileKind::RecipeUbuntu,
            FileKind::RecipeWindows,
            FileKind::Cleanup,
        ];
        let evidence = records::read_native(
            marker.as_slice(),
            marker.len() as u64,
            |kind| Ok(std::io::Cursor::new(read(&path, &kind.name()))),
            &Selection {
                seal_sha256: records::digest(&marker),
                scope: dir.scope(),
            },
            &Fresh {
                now: TEST_BOOT.on_boot(NOW).unwrap(),
                boot_marker: TEST_BOOT.marker,
                dependencies: &deps,
            },
        )
        .unwrap();
        assert_eq!(evidence.recipes, recipes);
        assert_eq!(evidence.seal.binding.dependencies, deps.key(dir.scope()));
        assert_eq!(evidence.seal.binding.issued_utc, ISSUE.utc);
        assert_eq!(evidence.seal.finished_utc, NOW.utc);
        assert_eq!(
            evidence
                .seal
                .manifest
                .iter()
                .map(|row| row.file)
                .collect::<Vec<_>>(),
            expected
        );
        let mut actual_rows: Vec<Record> = evidence
            .seal
            .manifest
            .iter()
            .flat_map(|row| records::decode::<Vec<Record>>(&read(&path, &row.file.name())).unwrap())
            .collect();
        actual_rows.sort_by_key(|row| row.started_ms);
        assert_eq!(
            actual_rows.iter().map(|row| row.stage).collect::<Vec<_>>(),
            records::stages(records::Class::Test)
        );
        assert_eq!(
            dir.publish(&"test", &deps, ISSUE.utc, &entries(&[]), &body())
                .unwrap_err(),
            SinkError::Closed
        );
    }

    #[test]
    fn native_marker_rename_refuses_cancelled_stale_expired_drifted_and_unsettled_runs() {
        for failure in [
            "cancelled",
            "stale",
            "expired",
            "utc rollback",
            "drift",
            "unsettled",
        ] {
            let work = tempfile::tempdir().unwrap();
            let (path, dir) = claimed(work.path(), &[]);
            let (deps, _, bundle) = complete_test_fixture(&dir);
            let mut changed = deps.clone();
            changed.firmware = records::Measured::Measured([39; 32]);
            let cancel = crate::service::Shutdown::default();
            let calls = Cell::new(0);
            let clock = Cell::new(NOW);
            let result = dir.publish_complete(
                &bundle,
                ISSUE,
                &cancel,
                || clock.get(),
                || {
                    let next = calls.get() + 1;
                    calls.set(next);
                    let at_rename = next == 3;
                    if at_rename {
                        assert!(names(&path).contains(&temporary_name(COMPLETE)));
                        assert!(!names(&path).contains(&COMPLETE.to_owned()));
                        match failure {
                            "cancelled" => cancel.cancel(),
                            "expired" => clock.set(Instant2 {
                                utc: ISSUE.utc + records::MAX_TRIAL_MS / 1000 + 1,
                                mono_ms: records::MAX_TRIAL_MS + 1000,
                            }),
                            "utc rollback" => clock.set(Instant2 {
                                utc: ISSUE.utc - 1,
                                mono_ms: NOW.mono_ms,
                            }),
                            "stale" => clock.set(Instant2 {
                                utc: NOW.utc + 6,
                                mono_ms: NOW.mono_ms + 6000,
                            }),
                            _ => {}
                        }
                    }
                    Ok(PublishSample {
                        dependencies: if at_rename && failure == "drift" {
                            &changed
                        } else {
                            &deps
                        },
                        live_captured_ms: if failure == "stale" {
                            NOW.mono_ms
                        } else {
                            clock.get().mono_ms
                        },
                        settled: !(at_rename && failure == "unsettled"),
                    })
                },
            );
            assert_eq!(calls.get(), 3, "{failure}");
            assert_eq!(result, Err(SinkError::Io), "{failure}");
            assert!(!names(&path).contains(&COMPLETE.to_owned()), "{failure}");
            assert!(
                !names(&path).contains(&temporary_name(COMPLETE)),
                "{failure}"
            );
            assert_eq!(
                dir.publish(&"test", &deps, ISSUE.utc, &entries(&[]), &body())
                    .unwrap_err(),
                SinkError::Closed
            );
        }
    }

    #[test]
    fn publication_derives_the_binding_and_chain_inside_the_boundary() {
        use serde_json::json;
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (path, dir) = claimed(root.path(), &[r"D:\Private\Canary-Root"]);
        // A concurrent runner on the same directory loses the claim.
        let (owner, _) = user();
        assert_eq!(
            EvidenceDir::claim(
                PathPin::open(&path, true, false).unwrap(),
                sid(&owner),
                &[],
                TEST_BOOT
            )
            .err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
        let results = [
            ("inputs", raw(INPUTS)),
            ("host_baseline", raw(r#"{"failed":"unsupported_platform"}"#)),
            ("disposable_store", raw(r#""unimplemented""#)),
            ("cleanup", raw(r#"{"inconclusive":"teardown_uncertain"}"#)),
        ];
        let dependencies = ("projection", 7u32);
        let unwritten = dir
            .publish(
                &"test",
                &dependencies,
                1_800_000_000,
                &entries(&results),
                &body(),
            )
            .unwrap();
        assert!(unwritten.is_empty());
        assert_eq!(
            names(&path),
            [
                "claim.json",
                "cleanup.json",
                "custody-job-capacity.json",
                "host-baseline.json",
                "incomplete.json",
                "inputs.json"
            ]
        );
        let inputs = &value(&path, "inputs.json")[0];
        let binding = &inputs["binding"];
        let keyed: [u8; 32] =
            Sha256::digest(serde_json::to_vec(&(&dir.scope(), &dependencies)).unwrap()).into();
        assert_eq!(binding["class"], json!("test"));
        assert_eq!(binding["scope"], json!(dir.scope()));
        assert_eq!(binding["run"], json!(dir.run()));
        assert_eq!(binding["release"], json!(super::super::schema::release()));
        assert_eq!(binding["dependencies"], json!(keyed));
        assert_eq!(
            binding["expires_utc"],
            json!(1_800_000_000u64 + VALIDITY_SECS)
        );
        // The chain follows the caller's order across files.
        let mut prior = [0u8; 32];
        for name in [
            "inputs.json",
            "host-baseline.json",
            "custody-job-capacity.json",
            "cleanup.json",
        ] {
            assert_eq!(value(&path, name)[0]["prior"], json!(prior), "{name}");
            let file = read(&path, name);
            prior = Sha256::digest(&file[1..file.len() - 1]).into();
        }
        let summary = value(&path, "incomplete.json");
        assert_eq!(&summary["binding"], binding);
        assert_eq!(summary["unwritten"], json!([]));
        // Published once: the sink is closed.
        assert_eq!(
            dir.publish(&"test", &dependencies, 0, &entries(&results), &body()),
            Err(SinkError::Closed)
        );
    }

    #[test]
    fn a_caller_cannot_place_chosen_bytes_in_an_opaque_field() {
        use serde_json::json;
        let distribution = |receipt: [u8; 32]| {
            raw(&format!(
                r#"{{"passed":{{"kind":"distribution","receipt":{},"receipt_bytes":7438,"manifest":{},"manifest_bytes":11915,"payload_files":136,"payload_bytes":1}}}}"#,
                serde_json::to_string(&receipt).unwrap(),
                serde_json::to_string(&super::super::schema::stock_manifest()).unwrap()
            ))
        };
        for (receipt, published) in [
            ([7u8; 32], false),
            (crate::vm::public_egress::STOCK_RECEIPT_1113_SHA256, true),
        ] {
            let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
            let (path, dir) = claimed(root.path(), &[]);
            let results = [("distribution", distribution(receipt))];
            let unwritten = dir
                .publish(&"test", &(), 0, &entries(&results), &body())
                .unwrap();
            assert_eq!(unwritten.is_empty(), published, "{receipt:?}");
            assert_eq!(
                names(&path).contains(&"distribution.json".to_owned()),
                published
            );
            // The binding is the writer's own; no parameter supplies it.
            let summary = value(&path, "incomplete.json");
            assert_eq!(summary["binding"]["scope"], json!(dir.scope()));
            assert_eq!(summary["binding"]["run"], json!(dir.run()));
        }
    }

    #[test]
    fn an_interrupted_commit_leaves_no_final_entry_and_still_settles() {
        use serde_json::json;
        for (name, step) in [
            ("inputs.json", Step::Write),
            ("host-baseline.json", Step::Flush),
        ] {
            let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
            let (path, dir) = claimed(root.path(), &[]);
            let results = [
                ("inputs", raw(INPUTS)),
                ("host_baseline", raw(r#""unimplemented""#)),
            ];
            FAULT.with(|fault| fault.set(Some((name, step))));
            let published = dir.publish(&"test", &(), 0, &entries(&results), &body());
            FAULT.with(|fault| fault.set(None));
            let unwritten: Vec<&str> = published.unwrap().iter().map(|file| file.name()).collect();
            assert_eq!(unwritten, [name]);
            let listed = names(&path);
            assert!(
                !listed
                    .iter()
                    .any(|entry| entry == name || entry.ends_with(".partial")),
                "{listed:?}"
            );
            assert_eq!(value(&path, "incomplete.json")["unwritten"], json!([name]));
        }

        // An interrupted claim leaves the directory empty, so a retry claims it.
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, path) = private_dir(root.path(), "evidence");
        FAULT.with(|fault| fault.set(Some(("claim.json", Step::Write))));
        let refused = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            &[],
            TEST_BOOT,
        )
        .err();
        FAULT.with(|fault| fault.set(None));
        assert_eq!(refused, Some(NativeRefusal::PathCustodyUnverified));
        assert!(names(&path).is_empty());
        assert!(
            EvidenceDir::claim(
                PathPin::open(&path, true, true).unwrap(),
                sid(&owner),
                &[],
                TEST_BOOT
            )
            .is_ok()
        );
        assert_eq!(names(&path), ["claim.json"]);

        // A summary that cannot be committed is an error and leaves no entry.
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (path, dir) = claimed(root.path(), &[]);
        let results = [("inputs", raw(INPUTS))];
        FAULT.with(|fault| fault.set(Some(("incomplete.json", Step::Flush))));
        let published = dir.publish(&"test", &(), 0, &entries(&results), &body());
        FAULT.with(|fault| fault.set(None));
        assert_eq!(published, Err(SinkError::Io));
        assert_eq!(names(&path), ["claim.json", "inputs.json"]);
    }

    #[test]
    fn written_records_pass_the_closed_schema_and_the_private_value_scan() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (path, dir) = claimed(root.path(), &[]);
        let results = [
            ("inputs", raw(INPUTS)),
            // A synthetic endpoint in an unknown field of a caller's result.
            (
                "host_baseline",
                raw(r#"{"failed":"other_refusal","endpoint":"192.0.2.7:443"}"#),
            ),
        ];
        let unwritten = dir
            .publish(&"test", &(), 0, &entries(&results), &body())
            .unwrap();
        assert_eq!(unwritten, [DiagnosticFile::HostBaseline]);
        assert_eq!(
            names(&path),
            ["claim.json", "incomplete.json", "inputs.json"]
        );

        // The schema admits no free-form string, so this canary can only
        // match a closed spelling; it proves the byte scan still runs.
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (path, dir) = claimed(root.path(), &["native"]);
        assert_eq!(
            dir.publish(&"test", &(), 0, &entries(&results[..1]), &body()),
            Err(SinkError::Private)
        );
        assert_eq!(names(&path), ["claim.json"]);
    }

    #[test]
    fn emptiness_is_read_from_the_held_directory_not_its_name() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, named) = private_dir(root.path(), "named");
        let (_, held) = private_dir(root.path(), "held");
        std::fs::write(format!(r"{}\stale.json", held.as_str()), b"{}").unwrap();
        // A split: the pin's name is the empty directory, its held object is not.
        let mut pin = PathPin::open(&named, true, true).unwrap();
        pin.leaf = PathPin::open(&held, true, true).unwrap().leaf;
        assert_eq!(
            EvidenceDir::claim(pin, sid(&owner), &[], TEST_BOOT).err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
        assert_eq!(names(&held), ["stale.json"]);
        assert!(names(&named).is_empty());
    }

    /// Set by the parent test for the child process it spawns.
    const CHILD: &str = "ARX_SINK_INTERRUPTED_CLAIM";

    /// Runs only as a child process: claim the named directory and end the
    /// process after the claim bytes are written, before the rename.
    #[test]
    #[ignore = "child process of an_interrupted_child_claim_leaves_debris_a_retry_clears"]
    fn interrupted_claim_child() {
        let Some(path) = std::env::var_os(CHILD) else {
            return;
        };
        let path = LocalPath::parse(path.to_str().unwrap()).unwrap();
        let (owner, _) = user();
        EXIT.with(|exit| exit.set(Some((CLAIM, Step::Write))));
        let _ = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            &[],
            TEST_BOOT,
        );
        unreachable!("the claim ends the process before its rename");
    }

    #[test]
    fn an_interrupted_child_claim_leaves_debris_a_retry_clears() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, path) = private_dir(root.path(), "evidence");
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "vm::windows::qualification::sink::tests::interrupted_claim_child",
                "--exact",
                "--ignored",
                "--test-threads=1",
            ])
            .env(CHILD, path.as_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        // Exit code 3 comes only from the hook inside the claim's commit.
        assert_eq!(status.code(), Some(3));
        assert_eq!(names(&path), [".claim.json.partial"]);
        // The retry recognizes its own debris, deletes it through the held
        // handle, and claims the same directory.
        let dir = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            &[],
            TEST_BOOT,
        )
        .unwrap();
        assert_eq!(names(&path), ["claim.json"]);
        drop(dir);
    }

    #[test]
    fn only_the_sinks_own_debris_is_removed_before_a_claim() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, _) = user();
        let put = |dir: &LocalPath, name: &str| {
            std::fs::write(format!(r"{}\{name}", dir.as_str()), b"x").unwrap()
        };
        let claim = |dir: &LocalPath| {
            EvidenceDir::claim(
                PathPin::open(dir, true, true).unwrap(),
                sid(&owner),
                &[],
                TEST_BOOT,
            )
            .map(|_| ())
        };
        let refused = Err(NativeRefusal::PathCustodyUnverified);

        // Record and summary temporaries alone are debris.
        let (_, debris) = private_dir(root.path(), "debris");
        put(&debris, ".inputs.json.partial");
        put(&debris, ".incomplete.json.partial");
        assert_eq!(claim(&debris), Ok(()));
        assert_eq!(names(&debris), ["claim.json"]);

        // A foreign entry beside debris refuses, and nothing is removed.
        let (_, foreign) = private_dir(root.path(), "foreign");
        put(&foreign, ".claim.json.partial");
        put(&foreign, "notes.txt");
        assert_eq!(claim(&foreign), refused);
        assert_eq!(names(&foreign), [".claim.json.partial", "notes.txt"]);

        // A look-alike outside the fixed names is not debris.
        let (_, lookalike) = private_dir(root.path(), "lookalike");
        put(&lookalike, ".notes.json.partial");
        assert_eq!(claim(&lookalike), refused);
        assert_eq!(names(&lookalike), [".notes.json.partial"]);

        // A directory with a temporary name is not debris.
        let (_, nested) = private_dir(root.path(), "nested");
        std::fs::create_dir(format!(r"{}\.claim.json.partial", nested.as_str())).unwrap();
        assert_eq!(claim(&nested), refused);
        assert_eq!(names(&nested), [".claim.json.partial"]);

        // Once a claim exists the directory is consumed: debris beside it
        // still refuses, and the claim and debris both stay.
        let (_, consumed) = private_dir(root.path(), "consumed");
        put(&consumed, "claim.json");
        put(&consumed, ".incomplete.json.partial");
        assert_eq!(claim(&consumed), refused);
        assert_eq!(names(&consumed), [".incomplete.json.partial", "claim.json"]);
    }

    /// Set by the race test for its claimant-B child process.
    const RACE_CHILD: &str = "ARX_SINK_RACE_CLAIMANT";

    /// Runs only as a child process: claimant B claims the named directory,
    /// publishes one record, and ends the process while writing its summary
    /// temporary, before the rename. Filesystem work only.
    #[test]
    #[ignore = "child process of a_stale_debris_list_never_deletes_a_file_beside_a_valid_claim"]
    fn race_claimant_child() {
        let Some(path) = std::env::var_os(RACE_CHILD) else {
            return;
        };
        let path = LocalPath::parse(path.to_str().unwrap()).unwrap();
        let (owner, _) = user();
        let dir = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            &[],
            TEST_BOOT,
        )
        .unwrap();
        EXIT.with(|exit| exit.set(Some(("incomplete.json", Step::Write))));
        let results = [("inputs", raw(INPUTS))];
        let _ = dir.publish(&"test", &(), 0, &entries(&results), &body());
        unreachable!("claimant B ends during its summary write");
    }

    #[test]
    fn a_stale_debris_list_never_deletes_a_file_beside_a_valid_claim() {
        use std::{cell::RefCell, rc::Rc};
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, path) = private_dir(root.path(), "evidence");
        // Pre-claim debris from an earlier interrupted run.
        std::fs::write(
            format!(r"{}\.incomplete.json.partial", path.as_str()),
            b"old debris",
        )
        .unwrap();
        let target = path.as_str().to_owned();
        let after_b = Rc::new(RefCell::new(Vec::<String>::new()));
        let b_temporary = Rc::new(RefCell::new(Vec::<u8>::new()));
        let (seen, seen_bytes) = (after_b.clone(), b_temporary.clone());
        // Claimant A pauses after listing the debris; claimant B then claims
        // the directory and ends inside its summary commit.
        AFTER_LISTING.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let status = std::process::Command::new(std::env::current_exe().unwrap())
                    .args([
                        "vm::windows::qualification::sink::tests::race_claimant_child",
                        "--exact",
                        "--ignored",
                        "--test-threads=1",
                    ])
                    .env(RACE_CHILD, &target)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .unwrap();
                assert_eq!(status.code(), Some(3), "B ends inside its summary commit");
                let directory = LocalPath::parse(&target).unwrap();
                *seen.borrow_mut() = names(&directory);
                *seen_bytes.borrow_mut() = read(&directory, ".incomplete.json.partial");
            }));
        });
        let claimed_by_a = EvidenceDir::claim(
            PathPin::open(&path, true, true).unwrap(),
            sid(&owner),
            &[],
            TEST_BOOT,
        )
        .err();
        // B's run: its claim, its record and its summary temporary.
        assert_eq!(
            *after_b.borrow(),
            [".incomplete.json.partial", "claim.json", "inputs.json"]
        );
        assert!(b_temporary.borrow().starts_with(br#"{"contract""#));
        // A resumes with its stale list, finds B's claim under the held
        // directory, deletes nothing and refuses.
        assert_eq!(claimed_by_a, Some(NativeRefusal::PathCustodyUnverified));
        assert_eq!(
            names(&path),
            [".incomplete.json.partial", "claim.json", "inputs.json"]
        );
        assert_eq!(
            read(&path, ".incomplete.json.partial"),
            *b_temporary.borrow()
        );
    }

    #[test]
    fn a_live_temporary_entry_is_never_deleted() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (_, path) = private_dir(root.path(), "evidence");
        let pin = PathPin::open(&path, true, true).unwrap();
        let live = create_hidden(&pin.leaf, ".incomplete.json.partial").unwrap();
        let other = PathPin::open(&path, true, true).unwrap();
        assert!(clear_debris(&other.leaf).is_err());
        assert_eq!(names(&path), [".incomplete.json.partial"]);
        drop(live);
    }

    #[test]
    fn the_descriptor_decision_checks_owner_and_every_grant() {
        use windows_sys::Win32::Security::{GetSecurityDescriptorDacl, GetSecurityDescriptorOwner};
        let (data, me) = user();
        let decide = |sddl: &str| {
            let mut descriptor = null_mut();
            let wide_sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
            assert_ne!(
                unsafe {
                    ConvertStringSecurityDescriptorToSecurityDescriptorW(
                        wide_sddl.as_ptr(),
                        1,
                        &mut descriptor,
                        null_mut(),
                    )
                },
                0,
                "{sddl}"
            );
            let (mut owner, mut defaulted, mut present, mut dacl) = (null_mut(), 0, 0, null_mut());
            unsafe {
                assert_ne!(
                    GetSecurityDescriptorOwner(descriptor, &mut owner, &mut defaulted),
                    0
                );
                assert_ne!(
                    GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted),
                    0
                );
            }
            let decision = unsafe { private_descriptor(owner, dacl, sid(&data)) };
            unsafe { LocalFree(descriptor) };
            decision
        };
        for (name, sddl, private) in [
            ("owner only", format!("O:{me}D:P(A;OICI;FA;;;{me})"), true),
            (
                "owner, SYSTEM and Administrators",
                format!("O:{me}D:P(A;;FA;;;{me})(A;;FA;;;SY)(A;;FA;;;BA)"),
                true,
            ),
            (
                "deny entries grant nothing",
                format!("O:{me}D:P(D;;FA;;;WD)(A;;FA;;;{me})"),
                true,
            ),
            (
                "another owner",
                format!("O:S-1-5-21-1-2-3-1001D:P(A;;FA;;;{me})"),
                false,
            ),
            (
                "Administrators as owner",
                format!("O:BAD:P(A;;FA;;;{me})"),
                false,
            ),
            (
                "world readable",
                format!("O:{me}D:P(A;;FA;;;{me})(A;;FR;;;WD)"),
                false,
            ),
            (
                "authenticated users",
                format!("O:{me}D:P(A;;FA;;;{me})(A;;FR;;;AU)"),
                false,
            ),
            ("null DACL", format!("O:{me}D:NO_ACCESS_CONTROL"), false),
        ] {
            assert_eq!(decide(&sddl), private, "{name}");
        }
    }

    #[test]
    fn claim_refuses_nonprivate_nonempty_and_wrong_owner_directories() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let (owner, text) = user();
        let shared = directory(
            root.path(),
            "shared",
            &format!("O:{text}D:P(A;OICI;FA;;;{text})(A;OICI;FR;;;WD)"),
        );
        let (_, full) = private_dir(root.path(), "full");
        std::fs::write(format!(r"{}\stale.json", full.as_str()), b"{}").unwrap();
        for path in [&shared, &full] {
            assert_eq!(
                EvidenceDir::claim(
                    PathPin::open(path, true, false).unwrap(),
                    sid(&owner),
                    &[],
                    TEST_BOOT
                )
                .err(),
                Some(NativeRefusal::PathCustodyUnverified),
                "{}",
                path.as_str()
            );
        }
        // Ownership is compared against the expected owner, not just the DACL.
        let (_, private) = private_dir(root.path(), "private");
        let other = LocalSid::parse("S-1-5-21-1-2-3-1001").unwrap();
        assert_eq!(
            EvidenceDir::claim(
                PathPin::open(&private, true, false).unwrap(),
                other.0,
                &[],
                TEST_BOOT
            )
            .err(),
            Some(NativeRefusal::PathCustodyUnverified)
        );
        assert!(names(&private).is_empty());
        assert!(names(&shared).is_empty());
    }
}
