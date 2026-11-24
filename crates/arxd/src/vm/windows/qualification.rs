//! Authority-free, read-only preflight for the separate owner-local
//! qualification runner. Nothing here can mint trial or launch authority:
//! the runner's private `TrialAuthority` lives in its own binary crate. The
//! only output is a held, claimed, diagnostic-only evidence directory.
//!
//! Native observations here are **unverified on real hardware**.

mod capacity;
mod sink;

pub use sink::{DiagnosticFile, SinkError};

use super::*;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Read, time::Instant};
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

/// Stable identities for the runner's local invalidation projection. They
/// are published only inside a digest keyed by the runner's random scope.
pub struct StoreIdentity {
    pub trial_volume: u64,
    pub trial_volume_root: [u8; 16],
    pub trial_root: [u8; 16],
    pub backing_volume: u64,
    pub backing_file: [u8; 16],
    pub outer_volume: u64,
    pub vhd_virtual_bytes: u64,
    pub inner_extent_bytes: u64,
}

pub struct Preflight {
    pub inputs: Observation<Inputs>,
    pub host: Observation<Host>,
    pub distribution: Observation<Distribution>,
    pub limits: Observation<Limits>,
    pub activity: Observation<Activity>,
    pub whpx_present: Observation<bool>,
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
/// A baseline cannot be assembled outside the preflight:
///
/// ```compile_fail
/// fn forge(p: arxd::vm::qualification::Preflight) -> arxd::vm::qualification::Baseline {
///     arxd::vm::qualification::Baseline { observations: p }
/// }
/// ```
///
/// Nor can a completed qualification be written through the sink:
///
/// ```compile_fail,E0599
/// let _ = arxd::vm::qualification::DiagnosticFile::Qualification;
/// ```
pub struct Baseline {
    observations: Preflight,
    origin: Instant,
    evidence: sink::EvidenceDir,
    _held: Held,
}

struct Held {
    _bundle: Vec<PathPin>,
    _roots: [PathPin; 5],
    _token: VmToken,
    _vhd: OwnedHandle,
    _volume: capacity::HeldVolume,
}

impl Baseline {
    pub fn observations(&self) -> &Preflight {
        &self.observations
    }

    /// The monotonic origin of every observation's milliseconds.
    pub fn origin(&self) -> Instant {
        self.origin
    }

    /// Write one diagnostic file into the claimed directory.
    pub fn write(&self, file: DiagnosticFile, bytes: &[u8]) -> Result<(), SinkError> {
        self.evidence.write(file, bytes)
    }
}

fn elapsed(origin: Instant) -> u64 {
    u64::try_from(origin.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn observe<T>(
    origin: Instant,
    run: impl FnOnce() -> Result<T, NativeRefusal>,
) -> Result<Observation<T>, NativeRefusal> {
    let started_ms = elapsed(origin);
    let result = run()?;
    Ok(Observation {
        started_ms,
        finished_ms: elapsed(origin),
        result,
    })
}

/// Inspect only the explicitly enrolled roots, the current token and console,
/// aggregate counters and the exact stock payload. Behavioral trials are
/// later stages with their own collectors.
pub fn preflight(enrollment_file: &str, evidence_dir: &str) -> Result<Baseline, NativeRefusal> {
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
    let root = local(&enrollment.trial_root)?;
    let outer = local(&enrollment.outer_root)?;
    let backing = local(&enrollment.backing_file)?;
    let bundle = local(&enrollment.stock_bundle)?;
    let evidence_path = local(evidence_dir)?;
    if !roots_disjoint(&path, &root, &outer, &backing, &bundle, &evidence_path) {
        return Err(custody);
    }
    let evidence_pin = PathPin::open(&evidence_path, true, false)?;
    let trial_pin = PathPin::open(&root, true, false)?;
    let outer_pin = PathPin::open(&outer, true, false)?;
    let backing_pin = PathPin::open(&backing, false, true)?;
    let trial_volume = PathPin::open(&local(&root.as_str()[..3])?, true, false)?;
    if !sink::held_private(&trial_pin.leaf, user_sid)
        || !enrollment.trial_volume.matches(trial_volume.identity)
        || !enrollment.trial_identity.matches(trial_pin.identity)
        || !enrollment.backing_identity.matches(backing_pin.identity)
        || trial_pin.identity.volume == backing_pin.identity.volume
        || outer_pin.identity.volume != backing_pin.identity.volume
    {
        return Err(custody);
    }
    // Claim once every input root is held and checked. A concurrent runner
    // loses here, before any observation is taken.
    let private_values = [
        enrollment.owner_sid.as_str(),
        enrollment.stock_bundle.as_str(),
        enrollment.trial_root.as_str(),
        enrollment.outer_root.as_str(),
        enrollment.backing_file.as_str(),
        enrollment_file,
        evidence_dir,
    ];
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

    let host = observe(origin, || {
        let windows_build = platform()?;
        Ok(Host {
            windows_build,
            windows_revision: windows_revision()?,
        })
    })?;
    let owner = canonical_sid(&enrollment.owner_sid)?;
    let token =
        VmToken::derive_current_user(owner).map_err(|_| NativeRefusal::IdentityUnverified)?;
    let distribution_start = elapsed(origin);
    let (distribution, bundle_pins) = collect_distribution(&bundle)?;
    let distribution = Observation {
        started_ms: distribution_start,
        finished_ms: elapsed(origin),
        result: distribution,
    };

    let limits_start = elapsed(origin);
    let (vhd, facts) = capacity::fixed_vhd(&backing_pin)?;
    let volume = capacity::bind_volume(&trial_volume, facts.disk)?;
    let sample = capacity::CapacitySample {
        provider_subtype: facts.provider_subtype,
        vhd_virtual: facts.virtual_size,
        vhd_physical: facts.physical_size,
        sector: facts.sector,
        extent_start: volume.extent_start,
        extent_length: volume.extent_length,
        inner: capacity::held_free(&trial_pin.leaf)?,
        outer: capacity::held_free(&outer_pin.leaf)?,
        backing_length: backing_pin
            .leaf
            .metadata()
            .map_err(|_| NativeRefusal::DiskCeilingUnverified)?
            .len(),
        backing_allocated: backing_pin.allocated,
    };
    let limit_values = capacity::check_capacity(&sample)?;
    empty_job()?;
    let limits = Observation {
        started_ms: limits_start,
        finished_ms: elapsed(origin),
        result: limit_values,
    };

    let activity = observe(origin, || {
        let console_idle = current_console_idle(&token)?;
        let (active_cpus, available_ram_bytes) = host_resources()?;
        Ok(Activity {
            console_idle,
            active_cpus,
            available_ram_bytes,
            owner_ram_reserve_bytes: RAM_RESERVE,
            owner_cpu_reserve: active_cpus.div_ceil(2),
        })
    })?;
    let whpx_present = observe(origin, hypervisor_present)?;
    let store = StoreIdentity {
        trial_volume: trial_volume.identity.volume,
        trial_volume_root: trial_volume.identity.file,
        trial_root: trial_pin.identity.file,
        backing_volume: backing_pin.identity.volume,
        backing_file: backing_pin.identity.file,
        outer_volume: outer_pin.identity.volume,
        vhd_virtual_bytes: limits.result.vhd_virtual_bytes,
        inner_extent_bytes: limits.result.inner_extent_bytes,
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
            _roots: [source, trial_pin, outer_pin, backing_pin, trial_volume],
            _token: token,
            _vhd: vhd,
            _volume: volume,
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

/// The LSA record for the token's logon LUID.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct LsaLogon {
    logon_type: u32,
    session: u32,
    logon_time: i64,
    owner: bool,
}

struct ConsoleEvidence {
    token_session: u32,
    active_console: u32,
    /// The WTS account name resolved locally to the enrolled SID; `None`
    /// when no local mapping exists (including unknown cloud mappings).
    mapped: Option<bool>,
    same_account: bool,
    wts: [WtsSample; 2],
    lsa: [LsaLogon; 2],
    /// The token's session, LUID and logon SID did not change.
    token_stable: bool,
    now: i64,
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
    if e.mapped != Some(true)
        || !e.same_account
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

fn wts_sample(session: u32) -> Option<(WtsSample, [u16; 21], [u16; 18])> {
    let raw = wts_session(session)?;
    Some((
        WtsSample {
            session: raw.SessionId,
            state: raw.SessionState,
            flags: raw.SessionFlags,
            logon: raw.LogonTime,
            input: raw.LastInputTime,
            current: raw.CurrentTime,
        },
        raw.UserName,
        raw.DomainName,
    ))
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
    };
    unsafe { LsaFreeReturnBuffer(data.cast()) };
    Some(logon)
}

fn current_console_idle(token: &VmToken) -> Result<bool, NativeRefusal> {
    let reason = NativeRefusal::OwnerActivityUnverified;
    let session = token.identity.session;
    let luid = token.identity.authentication;
    let lsa_first = lsa_logon(&luid, token.account.0).ok_or(reason)?;
    let (first, user, domain) = wts_sample(session).ok_or(reason)?;
    let mapped = map_account(&domain, &user, &token.account);
    let (second, user_again, domain_again) = wts_sample(session).ok_or(reason)?;
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
        mapped,
        same_account: user == user_again && domain == domain_again,
        wts: [first, second],
        lsa: [lsa_first, lsa_second],
        token_stable,
        now,
    })
}

/// Resolve the WTS account name locally. `None` means no local mapping.
fn map_account(domain: &[u16], user: &[u16], account: &LocalSid) -> Option<bool> {
    let domain = wts_text(domain).filter(|name| !name.is_empty() && !name.contains('\\'))?;
    let user = wts_text(user).filter(|name| !name.is_empty() && !name.contains('\\'))?;
    let mut sid = [0usize; 16];
    let mut sid_size = size_of_val(&sid) as u32;
    let mut resolved_domain = [0u16; 256];
    let mut domain_size = resolved_domain.len() as u32;
    let mut kind = 0;
    // A fixed local lookup of the session's own qualified account name.
    let ok = unsafe {
        LookupAccountNameW(
            null(),
            wide(format!("{domain}\\{user}")).as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut sid_size,
            resolved_domain.as_mut_ptr(),
            &mut domain_size,
            &mut kind,
        )
    };
    if ok == 0 || kind != SidTypeUser {
        return None;
    }
    Some(unsafe { EqualSid(sid.as_mut_ptr().cast(), account.0) } != 0)
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

fn collect_distribution(root: &LocalPath) -> Result<(Distribution, Vec<PathPin>), NativeRefusal> {
    use crate::vm::public_egress::{STOCK_QEMU_1113, STOCK_RECEIPT_1113_SHA256};

    let reason = NativeRefusal::DistributionUnverified;
    let root_pin = PathPin::open(root, true, false).map_err(|_| reason)?;
    let receipt = distribution_pin(root, "RECEIPT.txt")?;
    let manifest = distribution_pin(root, "MANIFEST.sha256")?;
    let receipt_bytes = read_bounded(&receipt.leaf, RECEIPT_BYTES as u64).map_err(|_| reason)?;
    let manifest_bytes = read_bounded(&manifest.leaf, MANIFEST_BYTES as u64).map_err(|_| reason)?;
    let receipt_hash: [u8; 32] = Sha256::digest(&receipt_bytes).into();
    let manifest_hash: [u8; 32] = Sha256::digest(&manifest_bytes).into();
    if receipt_bytes.len() != RECEIPT_BYTES
        || receipt_hash != STOCK_RECEIPT_1113_SHA256
        || manifest_bytes.len() != MANIFEST_BYTES
        || hex::encode(manifest_hash) != STOCK_MANIFEST_1113_SHA256
    {
        return Err(reason);
    }

    let entries = parse_manifest(&manifest_bytes, PAYLOAD_ENTRIES).ok_or(reason)?;
    let mut allowed_files =
        BTreeSet::from(["receipt.txt".to_owned(), "manifest.sha256".to_owned()]);
    let mut allowed_dirs = BTreeSet::new();
    let mut pins = vec![root_pin, receipt, manifest];
    let mut payload_bytes = 0u64;
    for (name, sha256) in entries {
        let mut pin = distribution_pin(root, name)?;
        let size = pin.leaf.metadata().map_err(|_| reason)?.len();
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
        pin.verify_content(ArtifactPin { size, sha256 }, MAX_PAYLOAD_FILE)
            .map_err(|_| reason)?;
        let folded = name.to_ascii_lowercase();
        for (offset, _) in folded.match_indices('/') {
            allowed_dirs.insert(folded[..offset].to_owned());
        }
        allowed_files.insert(folded);
        pins.push(pin);
    }
    verify_inventory(root, allowed_files, allowed_dirs)?;
    Ok((
        Distribution {
            receipt: receipt_hash,
            receipt_bytes: RECEIPT_BYTES as u64,
            manifest: manifest_hash,
            manifest_bytes: MANIFEST_BYTES as u64,
            payload_files: PAYLOAD_ENTRIES as u16,
            payload_bytes,
        },
        pins,
    ))
}

fn distribution_pin(root: &LocalPath, name: &str) -> Result<PathPin, NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    let path = name
        .split('/')
        .try_fold(root.clone(), |dir, part| dir.join(part))
        .map_err(|_| reason)?;
    PathPin::open(&path, false, false).map_err(|_| reason)
}

fn verify_inventory(
    root: &LocalPath,
    mut files: BTreeSet<String>,
    mut dirs: BTreeSet<String>,
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    let mut pending = vec![(root.clone(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        let _held = PathPin::open(&dir, true, false).map_err(|_| reason)?;
        for entry in std::fs::read_dir(dir.as_str()).map_err(|_| reason)? {
            let entry = entry.map_err(|_| reason)?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(reason)?;
            let path = dir.join(name).map_err(|_| reason)?;
            let key = format!("{prefix}{}", name.to_ascii_lowercase());
            let kind = entry.file_type().map_err(|_| reason)?;
            if kind.is_dir() && dirs.remove(&key) {
                pending.push((path, format!("{key}/")));
            } else if !kind.is_file() || !files.remove(&key) {
                return Err(reason);
            }
        }
    }
    if files.is_empty() && dirs.is_empty() {
        Ok(())
    } else {
        Err(reason)
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
        };
        ConsoleEvidence {
            token_session: 2,
            active_console: 2,
            mapped: Some(true),
            same_account: true,
            wts: [
                wts,
                WtsSample {
                    current: wts.current + 10,
                    ..wts
                },
            ],
            lsa: [lsa, lsa],
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

        let cases: [Mutation; 19] = [
            ("unknown cloud mapping", |e| e.mapped = None),
            ("mapped to another SID", |e| e.mapped = Some(false)),
            ("account name changed", |e| e.same_account = false),
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
            ("LSA logon far from WTS logon", |e| {
                e.lsa[0].logon_time = 100 + LOGON_SKEW_TICKS + 1;
                e.lsa[1].logon_time = 100 + LOGON_SKEW_TICKS + 1;
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
}
