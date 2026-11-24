//! Authority-free, read-only preflight for the separate owner-local
//! qualification runner. Nothing here can mint trial or launch authority:
//! the runner's private `TrialAuthority` lives in its own binary crate. The
//! only output is a held, claimed, diagnostic-only evidence directory whose
//! every file passes a closed schema at the held writer.
//!
//! Native observations here are **unverified on real hardware**.

mod capacity;
mod schema;
mod sink;

pub use schema::{RECORD_COLLECTOR, RECORD_CONTRACT};
pub use sink::{DiagnosticFile, Entry, SinkError};

use super::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    time::Instant,
};
use windows_sys::{
    Wdk::System::SystemServices::RtlGetVersion,
    Win32::System::{
        LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
        SystemServices::VER_NT_WORKSTATION,
    },
};

const GIB: u64 = 1 << 30;
const DISK_CEILING: u64 = 200 * GIB;
const OUTER_FLOOR: u64 = 50 * GIB;
const RAM_RESERVE: u64 = 16 * GIB;
const JOB_MEMORY_BYTES: usize = 2 * (1 << 30);
const JOB_CPU_RATE: u32 = 2500;
const MANIFEST_BYTES: usize = 11_915;
const RECEIPT_BYTES: usize = 7_438;
const PAYLOAD_ENTRIES: usize = 136;
const MAX_MANIFEST_BYTES: usize = 32_768;
const MAX_PAYLOAD_FILE: u64 = 512 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ENROLLMENT_BYTES: u64 = 8192;
/// Reviewed stock distribution manifest (ADR 0054).
pub const STOCK_MANIFEST_1113_SHA256: &str =
    "f1cf73c8ecae163e31467a3985fc13749aee1f12a8747756eb136108a1015759";
/// Five seconds in WTS/FILETIME 100 ns units.
const LIVE_TICKS: i64 = 50_000_000;
const IDLE_TICKS: i64 = 6_000_000_000;
/// Bound between the LSA logon record and the WTS session logon time.
const LOGON_SKEW_TICKS: i64 = 1_200_000_000;
/// Matches the interactive-only rule in `token_identity`.
const LOGON_INTERACTIVE: u32 = 2;

/// Raw local enrollment. It never leaves this module: no field is copied
/// into an observation, error or output.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    owner_sid: String,
    stock_bundle: String,
    trial_root: String,
    trial_volume: VolumePin,
    /// The mount manager's `\\?\Volume{GUID}\` name of the trial volume.
    trial_volume_guid: String,
    trial_identity: VolumePin,
    outer_root: String,
    backing_file: String,
    backing_identity: VolumePin,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VolumePin {
    volume: u64,
    file: [u8; 16],
}

impl VolumePin {
    fn matches(&self, id: FileIdentity) -> bool {
        self.volume == id.volume && self.file == id.file
    }
}

/// A result with monotonic milliseconds from `Baseline::origin`.
pub struct Observation<T> {
    pub started_ms: u64,
    pub finished_ms: u64,
    pub result: T,
}

/// A post-claim observation. A refusal here never ends the run early: the
/// runner settles it as a typed diagnostic and finishes the directory.
pub enum Taken<T> {
    Observed(Observation<T>),
    Refused { at_ms: u64, reason: NativeRefusal },
}

pub struct Inputs {
    pub held_roots: u8,
    pub enrollment_private: bool,
    pub evidence_private: bool,
    pub trial_root_private: bool,
    pub evidence_claimed: bool,
}

pub struct Host {
    pub windows_build: u32,
    pub windows_revision: u32,
    /// The installation's `MachineGuid`. Local only: the runner binds it into
    /// the keyed invalidation digest and never publishes it.
    pub machine: [u8; 16],
}

pub struct Distribution {
    pub receipt: [u8; 32],
    pub receipt_bytes: u64,
    pub manifest: [u8; 32],
    pub manifest_bytes: u64,
    pub payload_files: u16,
    pub payload_bytes: u64,
}

pub struct Limits {
    pub inner_caller_free_bytes: u64,
    pub inner_total_free_bytes: u64,
    pub inner_total_bytes: u64,
    pub inner_extent_bytes: u64,
    pub vhd_virtual_bytes: u64,
    pub outer_caller_free_bytes: u64,
    pub outer_total_free_bytes: u64,
    pub backing_allocated_bytes: u64,
    pub backing_length_bytes: u64,
    pub disk_ceiling_bytes: u64,
    pub outer_floor_bytes: u64,
    pub disk_budget_bytes: u64,
    pub empty_job_memory_limit_bytes: u64,
    pub empty_job_cpu_rate: u32,
}

pub struct Activity {
    pub console_idle: bool,
    pub active_cpus: u32,
    pub available_ram_bytes: u64,
    pub owner_ram_reserve_bytes: u64,
    pub owner_cpu_reserve: u32,
}

/// Stable identities of the held store objects, taken before the claim from
/// the GUID-anchored walks. They are published only inside a digest keyed
/// by the runner's random scope.
pub struct StoreIdentity {
    pub trial_volume: u64,
    pub trial_volume_root: [u8; 16],
    pub trial_volume_guid: [u8; 16],
    pub trial_root: [u8; 16],
    pub outer_volume: u64,
    pub outer_volume_root: [u8; 16],
    pub outer_volume_guid: [u8; 16],
    pub backing_file: [u8; 16],
}

pub struct Preflight {
    pub inputs: Observation<Inputs>,
    pub host: Taken<Host>,
    pub distribution: Taken<Distribution>,
    pub limits: Taken<Limits>,
    pub activity: Taken<Activity>,
    pub whpx_present: Taken<bool>,
    pub store: StoreIdentity,
}

/// Held custody for the whole run. No public constructor, no Clone, and no
/// authority: it carries only observations, pins and the diagnostic sink.
///
/// The runner's trial authority is not part of this library:
///
/// ```compile_fail,E0432
/// use arxd::vm::qualification::TrialAuthority;
/// ```
///
/// A baseline cannot be assembled outside the preflight. Every field is
/// supplied, so field privacy is the only refusal:
///
/// ```compile_fail,E0451
/// fn forge(p: arxd::vm::qualification::Preflight) -> arxd::vm::qualification::Baseline {
///     arxd::vm::qualification::Baseline {
///         observations: p,
///         origin: std::time::Instant::now(),
///         evidence: todo!(),
///         _held: todo!(),
///     }
/// }
/// ```
///
/// Nor converted into the daemon's launch authority, by `From`:
///
/// ```compile_fail,E0277
/// fn launch(b: arxd::vm::qualification::Baseline) -> arxd::vm::runtime::LaunchAuthority {
///     b.into()
/// }
/// ```
///
/// or by `TryFrom`:
///
/// ```compile_fail,E0277
/// use arxd::vm::runtime::LaunchAuthority;
/// fn launch(b: arxd::vm::qualification::Baseline) -> Option<LaunchAuthority> {
///     LaunchAuthority::try_from(b).ok()
/// }
/// ```
///
/// Nor can a completed qualification be written through the sink:
///
/// ```compile_fail,E0599
/// let _ = arxd::vm::qualification::DiagnosticFile::Qualification;
/// ```
///
/// There is no byte-taking writer, so caller bytes never reach an opaque
/// field:
///
/// ```compile_fail,E0599
/// fn write(b: &arxd::vm::qualification::Baseline) {
///     let _ = b.write(arxd::vm::qualification::DiagnosticFile::Inputs, b"[]");
/// }
/// ```
///
/// and a published entry has no field for the chain digest the writer
/// derives:
///
/// ```compile_fail,E0560
/// fn entry<'a>(
///     stage: &'a &'static str,
///     result: &'a (),
/// ) -> arxd::vm::qualification::Entry<'a, &'static str, ()> {
///     arxd::vm::qualification::Entry { stage, started_ms: 0, finished_ms: 0, result, prior: [0; 32] }
/// }
/// ```
///
/// Positive control: the authority-free surface these checks rely on
/// compiles, so each refusal above is the fence and not a missing path.
///
/// ```no_run
/// use arxd::vm::qualification::{Baseline, DiagnosticFile, Entry, Preflight, SinkError};
/// fn surface<'a>(b: &'a Baseline) -> Result<&'a Preflight, SinkError> {
///     let entry = Entry { stage: &"inputs", started_ms: 0, finished_ms: 1, result: &"unimplemented" };
///     let unwritten: Vec<DiagnosticFile> = b.publish(&"test", &(), 0, &[entry], &())?;
///     let _ = (unwritten, b.scope(), b.run());
///     Ok(b.observations())
/// }
/// let _ = surface;
/// let _ = arxd::vm::qualification::preflight;
/// let _: Option<arxd::vm::runtime::LaunchAuthority> = None;
/// ```
pub struct Baseline {
    observations: Preflight,
    origin: Instant,
    evidence: sink::EvidenceDir,
    _held: Held,
}

/// Every payload file and every directory under the distribution root, each
/// opened relative to its held parent.
type HeldTree = (Vec<File>, BTreeMap<String, File>);

struct Held {
    _bundle: Option<(PathPin, HeldTree)>,
    _pins: [PathPin; 3],
    _volumes: [capacity::AnchoredVolume; 2],
    _token: VmToken,
    _capacity: Option<(OwnedHandle, capacity::HeldDevice)>,
}

impl Baseline {
    pub fn observations(&self) -> &Preflight {
        &self.observations
    }

    /// The monotonic origin of every observation's milliseconds.
    pub fn origin(&self) -> Instant {
        self.origin
    }

    /// The run's opaque scope, drawn by the evidence writer's own random
    /// generator before the claim.
    pub fn scope(&self) -> [u8; 32] {
        self.evidence.scope()
    }

    /// The run's opaque identifier, drawn like the scope.
    pub fn run(&self) -> [u8; 16] {
        self.evidence.run()
    }

    /// Publish the run's diagnostics once: each entry's stage, times and
    /// result, in chain order, then the summary fields. The writer derives
    /// the binding (scope, run, release and the scope-keyed digest of
    /// `dependencies`) and the record chain itself, commits every file
    /// through a temporary entry, and writes the summary last. It returns the
    /// record files it could not commit, which the summary lists.
    pub fn publish<C, D, St, R, B>(
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
        self.evidence
            .publish(class, dependencies, issued_utc, entries, summary)
    }
}

/// The release digest the writer binds, as the runner derives it. Pure and
/// authority-free; the runner's tests check the two derivations agree.
pub fn release_digest() -> [u8; 32] {
    schema::release()
}

/// The closed-schema check the held writer applies to `file`. Pure and
/// authority-free; the runner's tests keep every encoding inside it.
pub fn schema_accepts(file: DiagnosticFile, bytes: &[u8]) -> bool {
    schema::check(file, bytes)
}

fn elapsed(origin: Instant) -> u64 {
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Take one post-claim observation and whatever it must keep held.
fn take_held<T, H>(
    origin: Instant,
    run: impl FnOnce() -> Result<(T, H), NativeRefusal>,
) -> (Taken<T>, Option<H>) {
    let started_ms = elapsed(origin);
    match run() {
        Ok((result, held)) => (
            Taken::Observed(Observation {
                started_ms,
                finished_ms: elapsed(origin),
                result,
            }),
            Some(held),
        ),
        Err(reason) => (
            Taken::Refused {
                at_ms: elapsed(origin),
                reason,
            },
            None,
        ),
    }
}

fn take<T>(origin: Instant, run: impl FnOnce() -> Result<T, NativeRefusal>) -> Taken<T> {
    take_held(origin, || run().map(|result| (result, ()))).0
}

/// Inspect only the explicitly enrolled roots, the current token and console,
/// aggregate counters and the exact stock payload. Behavioral trials are
/// later stages with their own collectors.
pub fn preflight(enrollment_file: &str, evidence_dir: &str) -> Result<Baseline, NativeRefusal> {
    preflight_with(&capacity::MountManager, enrollment_file, evidence_dir)
}

/// Every refusal before the claim leaves the evidence directory untouched,
/// so a corrected invocation can retry in it. After the claim nothing
/// returns early: each observation is a typed result the runner settles.
fn preflight_with(
    names: &impl capacity::VolumeNames,
    enrollment_file: &str,
    evidence_dir: &str,
) -> Result<Baseline, NativeRefusal> {
    let origin = Instant::now();
    let custody = NativeRefusal::PathCustodyUnverified;
    let user = current_user().map_err(|_| NativeRefusal::IdentityUnverified)?;
    let user_sid = user_sid(&user);
    let path = local(enrollment_file)?;
    let source = PathPin::open(&path, false, false)?;
    if !sink::held_private(&source.leaf, user_sid) {
        return Err(custody);
    }
    let bytes = read_bounded(&source.leaf, MAX_ENROLLMENT_BYTES)
        .map_err(|_| NativeRefusal::QualificationMissing)?;
    let enrollment: Enrollment =
        serde_json::from_slice(&bytes).map_err(|_| NativeRefusal::QualificationMissing)?;
    // Identity first, before any root is pinned or output claimed: the
    // enrolled owner must be this process's interactive, non-elevated,
    // non-impersonating console token. An elevated or mismatched invocation
    // is refused here and writes nothing.
    let owner = canonical_sid(&enrollment.owner_sid)?;
    let token =
        VmToken::derive_current_user(owner).map_err(|_| NativeRefusal::IdentityUnverified)?;

    let root = local(&enrollment.trial_root)?;
    let outer = local(&enrollment.outer_root)?;
    let backing = local(&enrollment.backing_file)?;
    let bundle = local(&enrollment.stock_bundle)?;
    let evidence_path = local(evidence_dir)?;
    if !roots_disjoint(&path, &root, &outer, &backing, &bundle, &evidence_path) {
        return Err(custody);
    }
    // Each drive letter is resolved once, to a GUID root held for the run.
    // Every leaf is walked from its held root, so all observations of a
    // volume come from the same proven objects.
    let outer_volume = capacity::anchor_volume(names, outer.as_str())?;
    let trial_volume = capacity::anchor_volume(names, &root.as_str()[..3])?;
    // The evidence directory shares write so the writer can rename its
    // committed entries relative to this held handle.
    let evidence_pin = capacity::walk(names, &outer_volume, &evidence_path, true, true)?;
    let trial_pin = capacity::walk(names, &trial_volume, &root, true, false)?;
    let backing_pin = capacity::walk(names, &outer_volume, &backing, false, true)?;
    if !sink::held_private(&trial_pin.leaf, user_sid)
        || !enrollment.trial_volume.matches(trial_volume.identity)
        || !trial_volume
            .guid
            .eq_ignore_ascii_case(&enrollment.trial_volume_guid)
        || !enrollment.trial_identity.matches(trial_pin.identity)
        || !enrollment.backing_identity.matches(backing_pin.identity)
        || trial_volume.guid_bytes == outer_volume.guid_bytes
        || trial_volume.identity.volume == outer_volume.identity.volume
    {
        return Err(custody);
    }
    let private_values = [
        enrollment.owner_sid.as_str(),
        enrollment.stock_bundle.as_str(),
        enrollment.trial_root.as_str(),
        enrollment.trial_volume_guid.as_str(),
        enrollment.outer_root.as_str(),
        enrollment.backing_file.as_str(),
        enrollment_file,
        evidence_dir,
    ];
    // Claim once identity and every input root are held and checked. A
    // concurrent runner loses here, before any observation is taken.
    let evidence = sink::EvidenceDir::claim(evidence_pin, user_sid, &private_values)?;

    let inputs = Observation {
        started_ms: 0,
        finished_ms: elapsed(origin),
        result: Inputs {
            held_roots: 5,
            enrollment_private: true,
            evidence_private: true,
            trial_root_private: true,
            evidence_claimed: true,
        },
    };
    let host = take(origin, || {
        Ok(Host {
            windows_build: platform()?,
            windows_revision: windows_revision()?,
            machine: machine_guid()?,
        })
    });
    let (distribution, bundle_pins) = take_held(origin, || collect_distribution(&bundle));
    let (limits, capacity_handles) = take_held(origin, || {
        let reason = NativeRefusal::DiskCeilingUnverified;
        let (vhd, facts) = capacity::fixed_vhd(names, &outer_volume, &backing_pin)?;
        let device = capacity::volume_extent(&trial_volume, facts.disk)?;
        let sample = capacity::CapacitySample {
            provider_subtype: facts.provider_subtype,
            vhd_virtual: facts.virtual_size,
            vhd_physical: facts.physical_size,
            sector: facts.sector,
            extent_start: device.extent_start,
            extent_length: device.extent_length,
            inner: capacity::held_free(&trial_pin.leaf)?,
            outer: capacity::held_free(&outer_volume.root)?,
            backing_length: backing_pin.leaf.metadata().map_err(|_| reason)?.len(),
            backing_allocated: backing_pin.allocated,
        };
        let limits = capacity::check_capacity(&sample)?;
        empty_job()?;
        // A drive letter rebound while the observation ran closes it.
        if !capacity::still_bound(names, &trial_volume)
            || !capacity::still_bound(names, &outer_volume)
        {
            return Err(reason);
        }
        Ok((limits, (vhd, device)))
    });
    let activity = take(origin, || {
        let console_idle = current_console_idle(&token)?;
        let (active_cpus, available_ram_bytes) = host_resources()?;
        Ok(Activity {
            console_idle,
            active_cpus,
            available_ram_bytes,
            owner_ram_reserve_bytes: RAM_RESERVE,
            owner_cpu_reserve: active_cpus.div_ceil(2),
        })
    });
    let whpx_present = take(origin, hypervisor_present);
    let store = StoreIdentity {
        trial_volume: trial_volume.identity.volume,
        trial_volume_root: trial_volume.identity.file,
        trial_volume_guid: trial_volume.guid_bytes,
        trial_root: trial_pin.identity.file,
        outer_volume: outer_volume.identity.volume,
        outer_volume_root: outer_volume.identity.file,
        outer_volume_guid: outer_volume.guid_bytes,
        backing_file: backing_pin.identity.file,
    };
    Ok(Baseline {
        observations: Preflight {
            inputs,
            host,
            distribution,
            limits,
            activity,
            whpx_present,
            store,
        },
        origin,
        evidence,
        _held: Held {
            _bundle: bundle_pins,
            _pins: [source, trial_pin, backing_pin],
            _volumes: [trial_volume, outer_volume],
            _token: token,
            _capacity: capacity_handles,
        },
    })
}

/// Containment rules for the enrolled roots and the evidence directory.
fn roots_disjoint(
    enrollment: &LocalPath,
    root: &LocalPath,
    outer: &LocalPath,
    backing: &LocalPath,
    bundle: &LocalPath,
    evidence: &LocalPath,
) -> bool {
    !(evidence.is_drive_root()
        || !outer.contains(evidence)
        || evidence.overlaps(root)
        || evidence.overlaps(bundle)
        || evidence.contains(enrollment)
        || evidence.overlaps(backing)
        || root.is_drive_root()
        || bundle.is_drive_root()
        || !outer.as_str().eq_ignore_ascii_case(r"D:\")
        || !outer.contains(backing)
        || backing.is_drive_root()
        || root.overlaps(outer)
        || bundle.overlaps(root)
        || bundle.overlaps(backing)
        || root.contains(enrollment))
}

fn empty_job() -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::JobUnverified;
    let job = Job::new(JOB_MEMORY_BYTES, Some(JOB_CPU_RATE)).map_err(|_| reason)?;
    let mut flags = 0;
    // Query only our own empty, unnamed Job. Assignment, nesting and
    // supervisor loss are later stages and cannot be inferred here.
    if unsafe { GetHandleInformation(job.0.as_raw_handle(), &mut flags) } == 0
        || flags & HANDLE_FLAG_INHERIT != 0
        || !matches!(job.active(), Ok(0))
    {
        return Err(reason);
    }
    Ok(())
}

fn current_user() -> io::Result<Vec<usize>> {
    let token = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY)?;
    token_data(&token, TokenUser)
}

fn user_sid(data: &[usize]) -> PSID {
    // TokenUser data begins with a TOKEN_USER that points into the buffer.
    unsafe { (*data.as_ptr().cast::<TOKEN_USER>()).User.Sid }
}

fn local(raw: &str) -> Result<LocalPath, NativeRefusal> {
    LocalPath::parse(raw).map_err(|_| NativeRefusal::PathCustodyUnverified)
}

fn canonical_sid(raw: &str) -> Result<LocalSid, NativeRefusal> {
    use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
    let reason = NativeRefusal::IdentityUnverified;
    if raw.len() > 184 || !raw.starts_with("S-1-") {
        return Err(reason);
    }
    let sid = LocalSid::parse(raw).map_err(|_| reason)?;
    let mut output = null_mut();
    // The OS round trip rejects aliases and noncanonical SID spellings.
    if unsafe { ConvertSidToStringSidW(sid.0, &mut output) } == 0 || output.is_null() {
        return Err(reason);
    }
    let expected = wide(raw);
    let mut matches = true;
    for (i, &unit) in expected.iter().enumerate() {
        // ConvertSidToStringSidW returned a terminated allocation. Stop at
        // the first mismatch, including an early terminator.
        if unsafe { *output.add(i) } != unit {
            matches = false;
            break;
        }
    }
    unsafe { LocalFree(output.cast()) };
    matches.then_some(sid).ok_or(reason)
}

/// The 16 bytes of an `8-4-4-4-12` hexadecimal GUID spelling.
fn parse_guid(text: &str) -> Option<[u8; 16]> {
    let bytes = text.as_bytes();
    if bytes.len() != 36 || [8, 13, 18, 23].iter().any(|&i| bytes[i] != b'-') {
        return None;
    }
    let digits: String = text.chars().filter(|&c| c != '-').collect();
    if digits.len() != 32 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0; 16];
    hex::decode_to_slice(&digits, &mut out).ok()?;
    Some(out)
}

fn platform() -> Result<u32, NativeRefusal> {
    let reason = NativeRefusal::UnsupportedPlatform;
    let (mut process, mut machine) = (0, 0);
    let mut version = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOEXW>() as u32,
        ..Default::default()
    };
    // The extended buffer is selected by its size. Check the actual machine
    // separately so an emulated process does not qualify an ARM64 host.
    if unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut machine) } == 0
        || process != IMAGE_FILE_MACHINE_UNKNOWN
        || machine != IMAGE_FILE_MACHINE_AMD64
        || unsafe { RtlGetVersion(ptr_of(&mut version).cast()) } != 0
        || version.dwMajorVersion != 10
        || version.dwMinorVersion != 0
        || version.dwBuildNumber < 22_000
        || u32::from(version.wProductType) != VER_NT_WORKSTATION
    {
        return Err(reason);
    }
    Ok(version.dwBuildNumber)
}

/// The update build revision, which build numbers alone omit.
fn windows_revision() -> Result<u32, NativeRefusal> {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    let mut value = 0u32;
    let mut size = size_of_val(&value) as u32;
    // A typed DWORD read of one fixed value into a fixed buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").as_ptr(),
            wide("UBR").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            ptr_of(&mut value),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS || size != 4 {
        return Err(NativeRefusal::UnsupportedPlatform);
    }
    Ok(value)
}

/// The installation's stable `MachineGuid`, for the local invalidation key.
fn machine_guid() -> Result<[u8; 16], NativeRefusal> {
    use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    let reason = NativeRefusal::UnsupportedPlatform;
    let mut value = [0u16; 40];
    let mut size = size_of_val(&value) as u32;
    // A typed, terminated string read of one fixed value into a fixed buffer.
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SOFTWARE\Microsoft\Cryptography").as_ptr(),
            wide("MachineGuid").as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            value.as_mut_ptr().cast(),
            &mut size,
        )
    };
    // Thirty-six UTF-16 units and the terminator.
    if status != ERROR_SUCCESS || size != 74 {
        return Err(reason);
    }
    String::from_utf16(&value[..36])
        .ok()
        .and_then(|text| parse_guid(&text))
        .ok_or(reason)
}

fn hypervisor_present() -> Result<bool, NativeRefusal> {
    let reason = NativeRefusal::WhpxUnverified;
    // Avoid adding a daemon load-time dependency on this optional platform DLL.
    let lib = unsafe {
        LoadLibraryExW(
            wide("WinHvPlatform.dll").as_ptr(),
            null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if lib.is_null() {
        return Err(reason);
    }
    // The function pointer is used only before this DLL reference is released.
    let entry = unsafe { GetProcAddress(lib, c"WHvGetCapability".as_ptr().cast()) };
    let result = entry.ok_or(reason).and_then(|entry| {
        type GetCapability =
            unsafe extern "system" fn(i32, *mut core::ffi::c_void, u32, *mut u32) -> i32;
        let query: GetCapability = unsafe { std::mem::transmute(entry) };
        let mut value = 0u64;
        let mut written = 0;
        // WHvCapabilityCodeHypervisorPresent is code zero and returns a BOOL.
        let status = unsafe {
            query(
                0,
                ptr_of(&mut value),
                size_of_val(&value) as u32,
                &mut written,
            )
        };
        match (status, written, value) {
            (0, 4, 0) => Ok(false),
            (0, 4, 1) => Ok(true),
            _ => Err(reason),
        }
    });
    unsafe { FreeLibrary(lib) };
    result
}

/// One read-only WTS sample of the token's session.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct WtsSample {
    session: u32,
    state: i32,
    flags: i32,
    logon: i64,
    input: i64,
    current: i64,
}

/// The account names a WTS sample reports for the session.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct WtsAccount {
    user: String,
    domain: String,
}

/// The LSA record for the token's logon LUID, read from the local logon
/// session itself. Its names are the record's own, never looked up by name.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LsaLogon {
    logon_type: u32,
    session: u32,
    logon_time: i64,
    owner: bool,
    user: String,
    domain: String,
}

struct ConsoleEvidence {
    token_session: u32,
    active_console: u32,
    /// The account names of both WTS samples; `None` when unreadable.
    accounts: [Option<WtsAccount>; 2],
    wts: [WtsSample; 2],
    lsa: [LsaLogon; 2],
    /// The token's session, LUID and logon SID did not change.
    token_stable: bool,
    now: i64,
}

/// Offline binding of the WTS session account to the LSA logon record that
/// is already pinned by the token's LUID and the enrolled SID: both must
/// name the same account. Nothing is resolved by name, so no lookup can
/// leave the host. Empty, qualified or unequal names are ambiguous and
/// close; WTS truncates long names, so a truncated name never matches.
fn account_bound(account: Option<&WtsAccount>, lsa: &LsaLogon) -> bool {
    let plain = |name: &str| !name.is_empty() && !name.contains('\\');
    account.is_some_and(|wts| {
        plain(&wts.user)
            && plain(&wts.domain)
            && plain(&lsa.user)
            && plain(&lsa.domain)
            && wts.user.eq_ignore_ascii_case(&lsa.user)
            && wts.domain.eq_ignore_ascii_case(&lsa.domain)
    })
}

/// Every comparison closes admission. A valid negative observation (recent
/// input) is returned as `Ok(false)` so the runner can record owner activity.
fn console_decision(e: &ConsoleEvidence) -> Result<bool, NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    let [first, second] = e.wts;
    let session = e.token_session;
    let lsa_ok = e.lsa.iter().all(|lsa| {
        lsa.owner
            && lsa.session == session
            && lsa.logon_type == LOGON_INTERACTIVE
            && lsa.logon_time > 0
            && lsa.logon_time.abs_diff(first.logon) <= LOGON_SKEW_TICKS as u64
    }) && e.lsa[0] == e.lsa[1];
    if e.accounts[0] != e.accounts[1]
        || !account_bound(e.accounts[0].as_ref(), &e.lsa[0])
        || !e.token_stable
        || !lsa_ok
        || e.active_console != session
        || first.session != session
        || second.session != session
        || first.state != WTSActive
        || second.state != WTSActive
        || first.flags != WTS_SESSIONSTATE_UNLOCK as i32
        || second.flags != WTS_SESSIONSTATE_UNLOCK as i32
        || first.logon <= 0
        || first.logon != second.logon
        || first.input < first.logon
        || second.input < first.input
        || second.current < first.current
        || second.current < second.input
        || !(0..=LIVE_TICKS).contains(&e.now.saturating_sub(second.current))
    {
        return Err(reason);
    }
    Ok(second.current - second.input >= IDLE_TICKS)
}

fn wts_sample(session: u32) -> Option<(WtsSample, Option<WtsAccount>)> {
    let raw = wts_session(session)?;
    let account = wts_text(&raw.UserName)
        .zip(wts_text(&raw.DomainName))
        .map(|(user, domain)| WtsAccount { user, domain });
    Some((
        WtsSample {
            session: raw.SessionId,
            state: raw.SessionState,
            flags: raw.SessionFlags,
            logon: raw.LogonTime,
            input: raw.LastInputTime,
            current: raw.CurrentTime,
        },
        account,
    ))
}

/// A counted LSA string; anything malformed reads as empty, which closes.
fn lsa_text(text: &LSA_UNICODE_STRING) -> String {
    if text.Buffer.is_null() || !text.Length.is_multiple_of(2) || text.Length > text.MaximumLength {
        return String::new();
    }
    // The LSA record owns this counted buffer until it is released.
    let units = unsafe { std::slice::from_raw_parts(text.Buffer, usize::from(text.Length / 2)) };
    String::from_utf16(units).unwrap_or_default()
}

fn lsa_logon(luid: &LUID, owner: PSID) -> Option<LsaLogon> {
    let mut data = null_mut();
    // The LSA allocates the record; it is released below.
    if unsafe { LsaGetLogonSessionData(luid, &mut data) } < 0 || data.is_null() {
        return None;
    }
    // A successful call returned one complete record.
    let record = unsafe { &*data };
    let logon = LsaLogon {
        logon_type: record.LogonType,
        session: record.Session,
        logon_time: record.LogonTime,
        owner: !record.Sid.is_null() && unsafe { EqualSid(record.Sid, owner) } != 0,
        user: lsa_text(&record.UserName),
        domain: lsa_text(&record.LogonDomain),
    };
    unsafe { LsaFreeReturnBuffer(data.cast()) };
    Some(logon)
}

fn current_console_idle(token: &VmToken) -> Result<bool, NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    let session = token.identity.session;
    let luid = token.identity.authentication;
    let lsa_first = lsa_logon(&luid, token.account.0).ok_or(reason)?;
    let (first, account) = wts_sample(session).ok_or(reason)?;
    let (second, account_again) = wts_sample(session).ok_or(reason)?;
    let lsa_second = lsa_logon(&luid, token.account.0).ok_or(reason)?;
    let actual = process_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY).map_err(|_| reason)?;
    let now_identity = token_identity(&actual, &token.account, false, TokenMode::CurrentUser)
        .map_err(|_| reason)?;
    let token_stable = now_identity.session == session
        && now_identity.authentication.LowPart == luid.LowPart
        && now_identity.authentication.HighPart == luid.HighPart
        && unsafe { EqualSid(now_identity.logon_sid(), token.identity.logon_sid()) } != 0;
    let mut time = FILETIME::default();
    unsafe { GetSystemTimeAsFileTime(&mut time) };
    let now = ((u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)) as i64;
    console_decision(&ConsoleEvidence {
        token_session: session,
        active_console: unsafe { WTSGetActiveConsoleSessionId() },
        accounts: [account, account_again],
        wts: [first, second],
        lsa: [lsa_first, lsa_second],
        token_stable,
        now,
    })
}

fn read_bounded(file: &File, max: u64) -> io::Result<Vec<u8>> {
    let declared = file.metadata()?.len();
    // Refuse an oversized declared length before allocating.
    if declared > max {
        return Err(io::Error::other("input byte limit exceeded"));
    }
    let mut bytes = Vec::with_capacity(declared as usize);
    file.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(io::Error::other("input byte limit exceeded"));
    }
    Ok(bytes)
}

fn collect_distribution(
    root: &LocalPath,
) -> Result<(Distribution, (PathPin, HeldTree)), NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    // The only walk by name. Every later open and listing goes through this
    // held root or a directory opened relative to it.
    let root_pin = PathPin::open(root, true, false).map_err(|_| reason)?;
    let (distribution, tree) = inspect_distribution(&root_pin.leaf)?;
    Ok((distribution, (root_pin, tree)))
}

fn inspect_distribution(root: &File) -> Result<(Distribution, HeldTree), NativeRefusal> {
    use crate::vm::public_egress::{STOCK_QEMU_1113, STOCK_RECEIPT_1113_SHA256};

    let reason = NativeRefusal::DistributionUnverified;
    let mut dirs = BTreeMap::new();
    let receipt = held_file(root, &mut dirs, "RECEIPT.txt").map_err(|_| reason)?;
    let manifest = held_file(root, &mut dirs, "MANIFEST.sha256").map_err(|_| reason)?;
    let receipt_bytes = read_bounded(&receipt, RECEIPT_BYTES as u64).map_err(|_| reason)?;
    let manifest_bytes = read_bounded(&manifest, MANIFEST_BYTES as u64).map_err(|_| reason)?;
    let receipt_hash: [u8; 32] = Sha256::digest(&receipt_bytes).into();
    let manifest_hash: [u8; 32] = Sha256::digest(&manifest_bytes).into();
    if receipt_bytes.len() != RECEIPT_BYTES
        || receipt_hash != STOCK_RECEIPT_1113_SHA256
        || manifest_bytes.len() != MANIFEST_BYTES
        || hex::encode(manifest_hash) != STOCK_MANIFEST_1113_SHA256
    {
        return Err(reason);
    }

    let payload = parse_manifest(&manifest_bytes, PAYLOAD_ENTRIES).ok_or(reason)?;
    let mut allowed_files =
        BTreeSet::from(["receipt.txt".to_owned(), "manifest.sha256".to_owned()]);
    let mut allowed_dirs = BTreeSet::new();
    let mut held = vec![receipt, manifest];
    let mut payload_bytes = 0u64;
    for (name, sha256) in payload {
        let file = held_file(root, &mut dirs, name).map_err(|_| reason)?;
        let size = file.metadata().map_err(|_| reason)?.len();
        payload_bytes = payload_bytes
            .checked_add(size)
            .filter(|&n| n <= MAX_PAYLOAD_BYTES)
            .ok_or(reason)?;
        if name == "qemu-system-x86_64.exe"
            && (size != STOCK_QEMU_1113.size || sha256 != STOCK_QEMU_1113.sha256)
            || name == "qemu-img.exe" && size != 2_520_350
        {
            return Err(reason);
        }
        // The held file itself is hashed, from its start.
        if !matches!(
            ArtifactPin { size, sha256 }.verify(&file, MAX_PAYLOAD_FILE),
            Ok(true)
        ) {
            return Err(reason);
        }
        let folded = name.to_ascii_lowercase();
        for (offset, _) in folded.match_indices('/') {
            allowed_dirs.insert(folded[..offset].to_owned());
        }
        allowed_files.insert(folded);
        held.push(file);
    }
    verify_inventory(root, &dirs, allowed_files, allowed_dirs)?;
    Ok((
        Distribution {
            receipt: receipt_hash,
            receipt_bytes: RECEIPT_BYTES as u64,
            manifest: manifest_hash,
            manifest_bytes: MANIFEST_BYTES as u64,
            payload_files: PAYLOAD_ENTRIES as u16,
            payload_bytes,
        },
        (held, dirs),
    ))
}

/// Open every directory of `path` (a `/`-separated relative name) relative
/// to its held parent, keyed by the lowercased relative name.
fn held_dir(root: &File, dirs: &mut BTreeMap<String, File>, path: &str) -> io::Result<()> {
    let mut key = String::new();
    for part in path.split('/') {
        let child = if key.is_empty() {
            part.to_ascii_lowercase()
        } else {
            format!("{key}/{}", part.to_ascii_lowercase())
        };
        if !dirs.contains_key(&child) {
            let parent = if key.is_empty() { root } else { &dirs[&key] };
            let dir = open_relative(parent, part, Node::Directory, false, false)?;
            file_identity(&dir, true)?;
            dirs.insert(child.clone(), dir);
        }
        key = child;
    }
    Ok(())
}

/// Open one file relative to its held parent directory, never by name from
/// a drive letter.
fn held_file(root: &File, dirs: &mut BTreeMap<String, File>, name: &str) -> io::Result<File> {
    let (key, leaf) = match name.rsplit_once('/') {
        Some((parent, leaf)) => {
            held_dir(root, dirs, parent)?;
            (parent.to_ascii_lowercase(), leaf)
        }
        None => (String::new(), name),
    };
    let parent = if key.is_empty() { root } else { &dirs[&key] };
    let file = open_relative(parent, leaf, Node::File, false, false)?;
    file_identity(&file, false)?;
    Ok(file)
}

/// The held root and every held directory list exactly the allowed entries,
/// read through their own handles.
fn verify_inventory(
    root: &File,
    dirs: &BTreeMap<String, File>,
    mut files: BTreeSet<String>,
    mut allowed: BTreeSet<String>,
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    let held = std::iter::once(("", root)).chain(dirs.iter().map(|(key, dir)| (key.as_str(), dir)));
    for (key, dir) in held {
        for (name, attributes) in listing(dir).map_err(|_| reason)? {
            let entry = if key.is_empty() {
                name.to_ascii_lowercase()
            } else {
                format!("{key}/{}", name.to_ascii_lowercase())
            };
            let listed = if attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                false
            } else if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
                allowed.remove(&entry)
            } else {
                files.remove(&entry)
            };
            if !listed {
                return Err(reason);
            }
        }
    }
    if files.is_empty() && allowed.is_empty() {
        Ok(())
    } else {
        Err(reason)
    }
}

/// Every entry of a held directory with its attributes, read through the
/// handle itself; the directory is never enumerated by name.
fn listing(dir: &File) -> io::Result<Vec<(String, u32)>> {
    const MAX_ENTRIES: usize = 4096;
    let mut buffer = vec![0u64; 8192];
    let capacity = buffer.len() * size_of::<u64>();
    let header = size_of::<FILE_FULL_DIR_INFO>();
    let name_offset = std::mem::offset_of!(FILE_FULL_DIR_INFO, FileName);
    let mut found = Vec::new();
    let mut class = FileFullDirectoryRestartInfo;
    loop {
        // The aligned buffer receives a chain of variable-length records.
        let ok = unsafe {
            GetFileInformationByHandleEx(
                dir.as_raw_handle(),
                class,
                buffer.as_mut_ptr().cast(),
                capacity as u32,
            )
        };
        if ok == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                return Ok(found);
            }
            return Err(error);
        }
        class = FileFullDirectoryInfo;
        let bytes = buffer.as_ptr().cast::<u8>();
        let mut offset = 0;
        loop {
            if offset + header > capacity {
                return Err(io::Error::other("directory record out of bounds"));
            }
            // The record starts inside the buffer at an 8-byte boundary.
            let entry = unsafe { &*bytes.add(offset).cast::<FILE_FULL_DIR_INFO>() };
            let length = entry.FileNameLength as usize;
            if offset + name_offset + length > capacity || !length.is_multiple_of(2) {
                return Err(io::Error::other("directory name out of bounds"));
            }
            // The name lies inside the buffer, as checked above.
            let units = unsafe {
                std::slice::from_raw_parts(
                    bytes.add(offset + name_offset).cast::<u16>(),
                    length / 2,
                )
            };
            let name = String::from_utf16(units).map_err(io::Error::other)?;
            if name != "." && name != ".." {
                if found.len() == MAX_ENTRIES {
                    return Err(io::Error::other("directory entry limit exceeded"));
                }
                found.push((name, entry.FileAttributes));
            }
            if entry.NextEntryOffset == 0 {
                break;
            }
            offset += entry.NextEntryOffset as usize;
        }
    }
}

fn parse_manifest(bytes: &[u8], count: usize) -> Option<Vec<(&str, [u8; 32])>> {
    if bytes.len() > MAX_MANIFEST_BYTES || count > PAYLOAD_ENTRIES {
        return None;
    }
    let raw = std::str::from_utf8(bytes).ok()?;
    let mut entries = Vec::with_capacity(count);
    let mut seen = BTreeSet::from(["manifest.sha256".to_owned(), "receipt.txt".to_owned()]);
    for line in raw.lines() {
        let (digest, name) = line.split_once("  ")?;
        if entries.len() == count
            || name.len() > 180
            || name.contains(['\\', ':'])
            || LocalPath::parse(&format!("D:\\{name}")).is_err()
            || !seen.insert(name.to_ascii_lowercase())
        {
            return None;
        }
        entries.push((name, sha256_spelling(digest)?));
    }
    (entries.len() == count).then_some(entries)
}

fn sha256_spelling(digest: &str) -> Option<[u8; 32]> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut bytes = [0; 32];
    hex::decode_to_slice(digest, &mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_bounded_and_refuses_aliases_and_duplicate_names() {
        let digest = "a".repeat(64);
        let valid = format!("{digest}  tools/a.exe\r\n{digest}  z.dll\r\n");
        assert_eq!(
            parse_manifest(valid.as_bytes(), 2).unwrap()[0].0,
            "tools/a.exe"
        );
        for name in [
            "../a.exe",
            "C:/a.exe",
            "/a.exe",
            r"a\z.dll",
            "tools/A.exe",
            "receipt.txt",
            "z.dll:ads",
        ] {
            let bad = format!("{digest}  tools/a.exe\n{digest}  {name}\n");
            assert!(parse_manifest(bad.as_bytes(), 2).is_none());
        }
        assert!(parse_manifest(valid.as_bytes(), 1).is_none());
        assert!(parse_manifest(&[b'a'; MAX_MANIFEST_BYTES + 1], 2).is_none());
    }

    #[test]
    fn canonical_sid_round_trips_only_exact_spellings() {
        assert!(canonical_sid("S-1-5-18").is_ok());
        assert!(canonical_sid("S-1-12-1-1111111111-2222222222-3333333333-4044444444").is_ok());
        for bad in ["s-1-5-18", "S-1-5-018", "S-1-0x5-18", "SY", "S-1-5-18 ", ""] {
            assert_eq!(
                canonical_sid(bad).err(),
                Some(NativeRefusal::IdentityUnverified),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn enrolled_roots_must_be_disjoint_and_contained() {
        let p = |raw: &str| LocalPath::parse(raw).unwrap();
        let (enrollment, root, outer, backing, bundle, evidence) = (
            p(r"C:\Owner\enrollment.json"),
            p(r"T:\trial"),
            p(r"D:\"),
            p(r"D:\Arx\trial.vhdx"),
            p(r"C:\Arx\qemu"),
            p(r"D:\Arx\evidence"),
        );
        assert!(roots_disjoint(
            &enrollment,
            &root,
            &outer,
            &backing,
            &bundle,
            &evidence
        ));
        for (name, enrollment, root, outer, backing, bundle, evidence) in [
            (
                "evidence is a drive root",
                &enrollment,
                &root,
                &outer,
                &backing,
                &bundle,
                &p(r"D:\"),
            ),
            (
                "evidence outside outer",
                &enrollment,
                &root,
                &outer,
                &backing,
                &bundle,
                &p(r"C:\e"),
            ),
            (
                "evidence holds backing",
                &enrollment,
                &root,
                &outer,
                &backing,
                &bundle,
                &p(r"D:\Arx"),
            ),
            (
                "trial root is a drive",
                &enrollment,
                &p(r"T:\"),
                &outer,
                &backing,
                &bundle,
                &evidence,
            ),
            (
                "outer is not D",
                &enrollment,
                &root,
                &p(r"E:\"),
                &backing,
                &bundle,
                &evidence,
            ),
            (
                "backing outside outer",
                &enrollment,
                &root,
                &outer,
                &p(r"C:\b.vhdx"),
                &bundle,
                &evidence,
            ),
            (
                "bundle in trial root",
                &enrollment,
                &root,
                &outer,
                &backing,
                &p(r"T:\trial\q"),
                &evidence,
            ),
            (
                "enrollment in trial root",
                &p(r"T:\trial\e.json"),
                &root,
                &outer,
                &backing,
                &bundle,
                &evidence,
            ),
            (
                "enrollment in evidence",
                &p(r"D:\Arx\evidence\e.json"),
                &root,
                &outer,
                &backing,
                &bundle,
                &evidence,
            ),
        ] {
            assert!(
                !roots_disjoint(enrollment, root, outer, backing, bundle, evidence),
                "{name}"
            );
        }
    }

    type Mutation = (&'static str, fn(&mut ConsoleEvidence));

    fn account(user: &str, domain: &str) -> Option<WtsAccount> {
        Some(WtsAccount {
            user: user.to_owned(),
            domain: domain.to_owned(),
        })
    }

    fn console() -> ConsoleEvidence {
        let wts = WtsSample {
            session: 2,
            state: WTSActive,
            flags: WTS_SESSIONSTATE_UNLOCK as i32,
            logon: 100,
            input: 100,
            current: 100 + IDLE_TICKS,
        };
        let lsa = LsaLogon {
            logon_type: LOGON_INTERACTIVE,
            session: 2,
            logon_time: 150,
            owner: true,
            user: "Owner".to_owned(),
            domain: "AzureAD".to_owned(),
        };
        ConsoleEvidence {
            token_session: 2,
            active_console: 2,
            accounts: [account("Owner", "AzureAD"), account("Owner", "AzureAD")],
            wts: [
                wts,
                WtsSample {
                    current: wts.current + 10,
                    ..wts
                },
            ],
            lsa: [lsa.clone(), lsa],
            token_stable: true,
            now: wts.current + 20,
        }
    }

    #[test]
    fn console_idle_requires_a_stable_correlated_owner_session() {
        assert_eq!(console_decision(&console()), Ok(true));
        let mut active = console();
        active.wts[1].input = active.wts[1].current - 1;
        assert_eq!(
            console_decision(&active),
            Ok(false),
            "recent input is owner activity"
        );
        // Account names compare case-insensitively on both sides.
        let mut cased = console();
        cased.accounts = [account("OWNER", "azuread"), account("OWNER", "azuread")];
        assert_eq!(console_decision(&cased), Ok(true));

        let cases: [Mutation; 25] = [
            ("unreadable WTS account", |e| e.accounts = [None, None]),
            ("LSA record names another account", |e| {
                e.lsa[0].user = "Other".to_owned();
                e.lsa[1].user = "Other".to_owned();
            }),
            ("LSA record names another domain", |e| {
                e.lsa[0].domain = "WORKGROUP".to_owned();
                e.lsa[1].domain = "WORKGROUP".to_owned();
            }),
            ("empty names on both sides", |e| {
                e.lsa[0].user = String::new();
                e.lsa[1].user = String::new();
                e.accounts = [account("", "AzureAD"), account("", "AzureAD")];
            }),
            ("qualified names on both sides", |e| {
                e.accounts = [
                    account(r"AzureAD\Owner", "AzureAD"),
                    account(r"AzureAD\Owner", "AzureAD"),
                ];
                e.lsa[0].user = r"AzureAD\Owner".to_owned();
                e.lsa[1].user = r"AzureAD\Owner".to_owned();
            }),
            ("truncated WTS name", |e| {
                e.accounts = [account("Own", "AzureAD"), account("Own", "AzureAD")]
            }),
            ("account name changed", |e| {
                e.accounts[1] = account("Owner2", "AzureAD")
            }),
            ("token incarnation changed", |e| e.token_stable = false),
            ("switched console", |e| e.active_console = 3),
            ("session mismatch", |e| e.wts[1].session = 3),
            ("disconnected", |e| e.wts[1].state = WTSDisconnected),
            ("locked", |e| e.wts[0].flags = WTS_SESSIONSTATE_LOCK as i32),
            ("logon incarnation changed", |e| e.wts[1].logon = 101),
            ("input before logon", |e| {
                e.wts[0].input = 99;
                e.wts[1].input = 99;
            }),
            ("WTS clock backwards", |e| {
                e.wts[1].current = e.wts[0].current - 1
            }),
            ("stale sample", |e| {
                e.now = e.wts[1].current + LIVE_TICKS + 1
            }),
            ("sample from the future", |e| e.now = e.wts[1].current - 1),
            ("remote LSA logon", |e| e.lsa[0].logon_type = 10),
            ("cached LSA logon", |e| e.lsa[1].logon_type = 11),
            ("LSA session differs", |e| e.lsa[1].session = 3),
            ("LSA SID differs", |e| e.lsa[0].owner = false),
            ("LSA record changed", |e| e.lsa[1].logon_time = 151),
            ("LSA names changed between samples", |e| {
                e.lsa[1].user = "Other".to_owned()
            }),
            ("LSA logon far from WTS logon", |e| {
                e.lsa[0].logon_time = 100 + LOGON_SKEW_TICKS + 1;
                e.lsa[1].logon_time = 100 + LOGON_SKEW_TICKS + 1;
            }),
            ("zero LSA logon time", |e| {
                e.lsa[0].logon_time = 0;
                e.lsa[1].logon_time = 0;
            }),
        ];
        for (name, mutate) in cases {
            let mut evidence = console();
            mutate(&mut evidence);
            assert_eq!(
                console_decision(&evidence),
                Err(NativeRefusal::OwnerActivityUnverified),
                "{name}"
            );
        }
    }

    #[test]
    fn guid_spellings_parse_to_exact_bytes() {
        assert_eq!(
            parse_guid("0123abcd-4567-89AB-cdef-0123456789ab"),
            Some([
                0x01, 0x23, 0xab, 0xcd, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67,
                0x89, 0xab
            ])
        );
        for bad in [
            "0123abcd-4567-89ab-cdef-0123456789a",
            "0123abcd-4567-89ab-cdef-0123456789abc",
            "0123abcd_4567-89ab-cdef-0123456789ab",
            "0123abcd-4567-89ab-cdef-0123456789ag",
            "0123abcd-4567-89ab-cdef--123456789ab",
            "{123abcd-4567-89ab-cdef-0123456789ab}",
        ] {
            assert_eq!(parse_guid(bad), None, "{bad}");
        }
    }

    /// A private file owned by the current user, as the owner enrolls it.
    fn private_file(dir: &Path, name: &str, bytes: &[u8]) -> String {
        use std::io::Write;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        };
        let user = current_user().unwrap();
        let mut text = null_mut();
        assert_ne!(
            unsafe { ConvertSidToStringSidW(user_sid(&user), &mut text) },
            0
        );
        let len = (0..).take_while(|&i| unsafe { *text.add(i) } != 0).count();
        let me = String::from_utf16(unsafe { std::slice::from_raw_parts(text, len) }).unwrap();
        unsafe { LocalFree(text.cast()) };
        let mut descriptor = null_mut();
        let sddl = wide(format!("O:{me}D:P(A;;FA;;;{me})"));
        assert_ne!(
            unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
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
        let path = dir.join(name);
        let handle = unsafe {
            CreateFileW(
                wide(&path).as_ptr(),
                FILE_GENERIC_WRITE,
                0,
                &attributes,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        unsafe { LocalFree(descriptor) };
        let mut file = File::from(unsafe { owned(handle) }.unwrap());
        file.write_all(bytes).unwrap();
        path.to_str().unwrap().to_owned()
    }

    #[test]
    fn identity_is_refused_before_any_root_is_pinned_or_output_claimed() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let evidence = root.path().join("evidence");
        std::fs::create_dir(&evidence).unwrap();
        let pin = r#"{"volume":1,"file":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0]}"#;
        // Every root is unusable, so only an identity check that runs first
        // can produce the identity refusal.
        for (index, owner) in ["S-1-5-21-1-2-3-1001", "S-1-5-018"].iter().enumerate() {
            let enrollment = format!(
                r#"{{"owner_sid":"{owner}","stock_bundle":"Q:\\missing\\qemu","trial_root":"T:\\missing","trial_volume":{pin},"trial_volume_guid":"none","trial_identity":{pin},"outer_root":"D:\\","backing_file":"D:\\missing\\trial.vhdx","backing_identity":{pin}}}"#
            );
            let file = private_file(
                root.path(),
                &format!("enrollment-{index}.json"),
                enrollment.as_bytes(),
            );
            assert_eq!(
                preflight(&file, evidence.to_str().unwrap()).err(),
                Some(NativeRefusal::IdentityUnverified),
                "{owner}"
            );
            assert_eq!(std::fs::read_dir(&evidence).unwrap().count(), 0);
        }
    }

    #[test]
    fn distribution_opens_and_lists_only_through_held_directories() {
        let root = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let named = root.path().join("named");
        let held = root.path().join("held");
        for dir in [&named, &held] {
            std::fs::create_dir_all(dir.join("sub")).unwrap();
            std::fs::write(dir.join("a.txt"), b"a").unwrap();
            std::fs::write(dir.join("sub").join("b.dll"), b"b").unwrap();
        }
        std::fs::write(named.join("only-named.txt"), b"n").unwrap();
        std::fs::write(held.join("only-held.txt"), b"h").unwrap();
        // A split: the inventory below matches the named directory, while
        // the held object is its sibling.
        let held_root = open_directory(held.to_str().unwrap()).unwrap();
        let mut dirs = BTreeMap::new();
        let set = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
        assert!(held_file(&held_root, &mut dirs, "only-named.txt").is_err());
        assert!(held_file(&held_root, &mut dirs, "sub/b.dll").is_ok());
        assert_eq!(
            verify_inventory(
                &held_root,
                &dirs,
                set(&["a.txt", "only-named.txt", "sub/b.dll"]),
                set(&["sub"])
            ),
            Err(NativeRefusal::DistributionUnverified)
        );
        // Control: the held object's own inventory, through held handles.
        assert_eq!(
            verify_inventory(
                &held_root,
                &dirs,
                set(&["a.txt", "only-held.txt", "sub/b.dll"]),
                set(&["sub"])
            ),
            Ok(())
        );
    }
}
