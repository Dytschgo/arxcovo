//! Win32 adapter for the one stock paused recipe. No VM disks, network,
//! guest channel or production launch authority are available here.

use super::*;
use std::{cell::RefCell, io::Write};

const SLOT: &str = ".arx-paused-trial-charge";
const CWD: &str = "paused-cwd";
const IMAGE: &str = "qemu-system-x86_64.exe";
const FIRMWARE: &str = "bios-256k.bin";
const INVENTORY_TIME: Duration = Duration::from_secs(2);

/// Path-free outcome for the binary's diagnostic graph.
pub enum PausedVerdict {
    Verified,
    Refused(NativeRefusal),
    OwnerActive,
    Qmp,
    Timeout,
    Cancelled,
}

pub struct PausedTrial {
    pub verdict: PausedVerdict,
    pub release: StopDisposition,
}

pub(in crate::vm::windows::qualification) async fn run_native(
    baseline: &Baseline,
    cancel: &Shutdown,
) -> PausedTrial {
    let prepared = Prepared::new(baseline);
    let result = match prepared {
        Ok(mut prepared) => {
            let plan = PausedPlan {
                image: &prepared.image,
                data: &prepared.data,
                bios: &prepared.bios,
            };
            run(
                &mut prepared.host,
                &plan,
                cancel,
                Tick::now() + Duration::from_secs(15),
                Duration::from_secs(5),
            )
            .await
        }
        Err(reason) => TrialResult {
            paused: Err(reason.into()),
            release: StopDisposition::DirtyRelease,
        },
    };
    PausedTrial {
        verdict: match result.paused {
            Ok(()) => PausedVerdict::Verified,
            Err(Failure::Refused(reason)) => PausedVerdict::Refused(reason),
            Err(Failure::OwnerActive) => PausedVerdict::OwnerActive,
            Err(Failure::Qmp(_)) => PausedVerdict::Qmp,
            Err(Failure::Timeout) => PausedVerdict::Timeout,
            Err(Failure::Cancelled) => PausedVerdict::Cancelled,
        },
        release: result.release,
    }
}

struct Prepared<'a> {
    host: Windows<'a>,
    image: LocalPath,
    data: LocalPath,
    bios: LocalPath,
}

impl<'a> Prepared<'a> {
    fn new(baseline: &'a Baseline) -> Result<Self, NativeRefusal> {
        let held = &baseline._held;
        let reason = NativeRefusal::DistributionUnverified;
        let bundle = held.bundle.as_ref().filter(|_| held.paused).ok_or(reason)?;
        let limits = match &baseline.observations.limits {
            Taken::Observed(limits) => &limits.result,
            Taken::Refused { reason, .. } => return Err(*reason),
        };
        if let Taken::Refused { reason, .. } = &baseline.observations.host {
            return Err(*reason);
        }
        let reason = NativeRefusal::PathCustodyUnverified;
        let cwd = held.trial.path.join(CWD).map_err(|_| reason)?;
        let cwd = super::super::capacity::walk(
            &super::super::capacity::MountManager,
            &held.volumes[0],
            &cwd,
            true,
            false,
        )?;
        let bundle_root = &bundle.root.path;
        let image = bundle_root.join(IMAGE).map_err(|_| reason)?;
        let data = bundle_root.join("share").map_err(|_| reason)?;
        let bios = data.join(FIRMWARE).map_err(|_| reason)?;
        Ok(Self {
            host: Windows {
                baseline,
                bundle,
                limits,
                cwd,
            },
            image,
            data,
            bios,
        })
    }
}

struct Windows<'a> {
    baseline: &'a Baseline,
    bundle: &'a inventory::Bundle,
    limits: &'a Limits,
    cwd: PathPin,
}

struct Opens<'a> {
    bundle: &'a inventory::Bundle,
    image: FileIdentity,
    peak: Peak,
}

struct Slot {
    charge: File,
    state: RefCell<ChildState>,
}

#[derive(Default)]
struct ChildState {
    child: Option<Child>,
    parent: Option<Stdio>,
    workers: Option<WorkerSet>,
    uncertain: bool,
}

impl Windows<'_> {
    fn validate_search_and_bundle(&self, bundle: &inventory::Bundle) -> Result<(), NativeRefusal> {
        let deadline = Instant::now() + INVENTORY_TIME;
        let token = &self.baseline._held.token;
        if !safe_dll_search()
            || !inventory::protected_directory(&self.cwd.leaf, token.account.0)
            || !listing_bounded(&self.cwd.leaf, 0, Some(deadline))
                .is_ok_and(|entries| entries.is_empty())
        {
            return Err(NativeRefusal::PathCustodyUnverified);
        }
        inventory::binding(&self.cwd)?;
        bundle.protected(token, deadline)
    }

    fn live_limits(&self) -> Result<Limits, NativeRefusal> {
        use super::super::capacity::{MountManager, held_free, still_bound};

        let held = &self.baseline._held;
        let inner = held_free(&held.trial.leaf)?;
        let outer = held_free(&held.volumes[1].root)?;
        if inner.total != self.limits.inner_total_bytes
            || !held
                .volumes
                .iter()
                .all(|volume| still_bound(&MountManager, volume))
        {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        Ok(Limits {
            inner_caller_free_bytes: inner.caller,
            inner_total_free_bytes: inner.total_free,
            outer_caller_free_bytes: outer.caller,
            outer_total_free_bytes: outer.total_free,
            ..*self.limits
        })
    }
}

impl<'a> Host for Windows<'a> {
    type Opens = Opens<'a>;
    type Slot = Slot;
    type Reader = tokio::io::DuplexStream;
    type Writer = tokio::io::DuplexStream;

    fn open_contract(&mut self, plan: &PausedPlan<'_>) -> Result<(Opens<'a>, Peak), NativeRefusal> {
        let reason = NativeRefusal::DistributionUnverified;
        let root = &self.bundle.root.path;
        let expected_image = root.join(IMAGE).map_err(|_| reason)?;
        let expected_data = root.join("share").map_err(|_| reason)?;
        if *plan.image != expected_image
            || *plan.data != expected_data
            || *plan.bios != expected_data.join(FIRMWARE).map_err(|_| reason)?
        {
            return Err(reason);
        }
        let image = self.bundle.identity(IMAGE)?;
        self.bundle.identity("share/bios-256k.bin")?;
        self.validate_search_and_bundle(self.bundle)?;
        let peak = Peak {
            // Conservative admission headroom for the diskless paused recipe.
            // Live samples continue through the exchange. Owner qualification
            // still measures the actual native overhead before admission.
            outside_ram_bytes: GIB,
            outside_cpu_rate: 1250,
            inner_bytes: 1 << 20,
            outer_bytes: 1 << 20,
        };
        Ok((
            Opens {
                bundle: self.bundle,
                image,
                peak,
            },
            peak,
        ))
    }

    fn live(&self) -> Result<Live, NativeRefusal> {
        let captured = Tick::now();
        let idle = current_console_idle(&self.baseline._held.token)?;
        let (active_cpus, available_ram_bytes) = host_resources()?;
        let effective_cpus = cpu_scope()?;
        let whpx = hypervisor_present()?;
        let limits = self.live_limits()?;
        Ok(Live {
            captured,
            idle,
            active_cpus,
            effective_cpus,
            available_ram_bytes,
            whpx,
            limits,
            // A single exclusive filesystem claim serializes all runs on
            // this held trial root, including an unsettled previous run.
            outstanding_inner_bytes: 0,
            outstanding_outer_bytes: 0,
        })
    }

    fn claim(&mut self, _: &Live, _: Peak) -> Result<Slot, NativeRefusal> {
        Slot::claim(&self.baseline._held.trial.leaf, self.baseline.run())
    }

    async fn confirm_claim(&self, slot: &Slot) -> Result<(), NativeRefusal> {
        if slot.state.borrow().uncertain {
            return Err(NativeRefusal::DiskCeilingUnverified);
        }
        Ok(())
    }

    async fn create_suspended(
        &self,
        slot: &Slot,
        plan: &PausedPlan<'_>,
        opens: &Opens<'a>,
    ) -> Result<(), NativeRefusal> {
        let token = &self.baseline._held.token;
        let (stdio, parent) = token.stdio().map_err(|_| NativeRefusal::QmpUnverified)?;
        let job = Job::with_process_limit(JOB_MEMORY_BYTES, Some(JOB_CPU_RATE), Some(1))
            .map_err(|_| NativeRefusal::JobUnverified)?;
        let env = environment(&self.cwd.path)?;
        let args = plan
            .args()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let launch = Launch::Paused {
            token: &token.token,
            image: plan.image,
            args: &args,
        };
        // No child creation precedes the final held inventory and namespace
        // check. All custody stays in the borrowed Baseline through settlement.
        self.validate_search_and_bundle(opens.bundle)?;
        let live = self.live()?;
        live.check(opens.peak, Tick::now())
            .map_err(|error| match error {
                Failure::Refused(reason) => reason,
                _ => NativeRefusal::OwnerActivityUnverified,
            })?;
        let mut state = slot.state.borrow_mut();
        let (process, thread) = create_suspended(
            &launch,
            Path::new(self.cwd.path.as_str()),
            &env,
            Assignment::Job(&job),
            &stdio,
        )
        .map_err(|error| {
            state.uncertain = error.stage == SpawnStage::Verify || !matches!(job.active(), Ok(0));
            NativeRefusal::JobUnverified
        })?;
        state.child = Some(Child {
            job,
            process,
            suspended: Some(thread),
        });
        state.parent = Some(parent);
        let child = state.child.as_ref().expect("child just stored");
        verify_image(child, plan.image, opens.image)?;
        Ok(())
    }

    async fn verify_child(
        &self,
        slot: &Slot,
    ) -> Result<Pipes<Self::Reader, Self::Writer>, NativeRefusal> {
        let mut state = slot.state.borrow_mut();
        let child = state.child.as_ref().ok_or(NativeRefusal::JobUnverified)?;
        verify_child_policy(child, &self.baseline._held.token)?;
        let parent = state.parent.take().ok_or(NativeRefusal::QmpUnverified)?;
        let dispatch = Arc::new(DispatchGate::default());
        let (stdout, stdin, workers) = start_stdio(parent, dispatch.clone()).map_err(|_| {
            state.uncertain = true;
            NativeRefusal::QmpUnverified
        })?;
        state.workers = Some(workers);
        Ok(Pipes {
            stdout,
            stdin,
            dispatch,
        })
    }

    async fn resume(&self, slot: &Slot) -> Result<(), NativeRefusal> {
        slot.state
            .borrow_mut()
            .child
            .as_mut()
            .ok_or(NativeRefusal::JobUnverified)?
            .resume()
            .map_err(|_| NativeRefusal::JobUnverified)
    }

    async fn verify_opens(&self, slot: &Slot, opens: &Opens<'a>) -> Result<(), NativeRefusal> {
        require_live(slot)?;
        self.validate_search_and_bundle(opens.bundle)
    }

    async fn verify_whpx(
        &self,
        slot: &Slot,
        query: impl Future<Output = Result<(), Failure>>,
    ) -> Result<(), Failure> {
        require_live(slot)?;
        query.await?;
        require_live(slot)?;
        Ok(())
    }

    fn request_stop(&mut self, slot: &Slot) {
        let mut state = slot.state.borrow_mut();
        if let Some(child) = &state.child {
            let _ = child.job.terminate();
        }
        if let Some(workers) = &state.workers {
            workers.stop();
        }
        state.parent = None;
    }

    async fn observe_stop(&mut self, slot: &Slot, deadline: Tick) -> StopEvidence {
        let (child, workers, uncertain) = {
            let mut state = slot.state.borrow_mut();
            (state.child.take(), state.workers.take(), state.uncertain)
        };
        let reaped = match &child {
            Some(child) => child.reap(deadline).await.is_ok(),
            None => true,
        };
        let workers_joined = match workers {
            Some(workers) => workers.join(deadline).await.is_ok(),
            None => true,
        };
        slot.state.borrow_mut().child = child;
        StopEvidence {
            process_exited: reaped && !uncertain,
            job_empty: reaped && !uncertain,
            workers_joined: workers_joined && !uncertain,
            ..StopEvidence::default()
        }
    }

    async fn reopen(&mut self, _: &Slot, deadline: Tick) -> bool {
        // Diskless: the charge is the only writable file and its original
        // exclusive create handle never leaves the slot.
        self.bundle.check(deadline.into_std()).is_ok()
            && inventory::binding(&self.baseline._held.trial).is_ok()
    }

    fn settle(&mut self, slot: Slot, disposition: StopDisposition) -> StopDisposition {
        slot.settle(disposition)
    }
}

impl Slot {
    fn claim(root: &File, run: [u8; 16]) -> Result<Self, NativeRefusal> {
        let reason = NativeRefusal::DiskCeilingUnverified;
        // FILE_CREATE with no sharing is the cross-process arbitration.
        // Any existing generation, even an empty crash remnant, refuses.
        let mut charge = sink::create_hidden(root, SLOT).map_err(|_| reason)?;
        let state = RefCell::new(ChildState {
            uncertain: charge
                .write_all(&run)
                .and_then(|()| charge.sync_all())
                .is_err(),
            ..ChildState::default()
        });
        // The caller settles even if flush failed. No child can start
        // until confirm_claim accepts the persisted charge.
        Ok(Self { charge, state })
    }

    fn settle(self, disposition: StopDisposition) -> StopDisposition {
        if disposition == StopDisposition::ChargedQuarantine || sink::discard(&self.charge).is_err()
        {
            StopDisposition::ChargedQuarantine
        } else {
            disposition
        }
    }
}

fn require_live(slot: &Slot) -> Result<(), NativeRefusal> {
    slot.state
        .borrow()
        .child
        .as_ref()
        .ok_or(NativeRefusal::JobUnverified)?
        .verify_live()
        .map(|_| ())
        .map_err(|_| NativeRefusal::JobUnverified)
}

fn cpu_scope() -> Result<u32, NativeRefusal> {
    let process = unsafe { GetCurrentProcess() };
    let (mut in_job, mut current, mut system) = (0, 0, 0);
    // Refuse nesting and multiple processor groups until there is a measured
    // effective cap for those configurations.
    if unsafe {
        IsProcessInJob(process, null_mut(), &mut in_job) == 0
            || in_job != 0
            || GetActiveProcessorGroupCount() != 1
            || GetProcessAffinityMask(process, &mut current, &mut system) == 0
    } {
        return Err(NativeRefusal::ReservesUnverified);
    }
    Ok(current.count_ones())
}

fn safe_dll_search() -> bool {
    use windows_sys::Win32::System::Registry::{
        HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RegGetValueW,
    };
    let mut value = 0u32;
    let mut size = size_of_val(&value) as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            wide(r"SYSTEM\CurrentControlSet\Control\Session Manager").as_ptr(),
            wide("SafeDllSearchMode").as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            ptr_of(&mut value),
            &mut size,
        )
    };
    // Safe DLL search is on by default. An explicit value must enable it.
    status == ERROR_FILE_NOT_FOUND || (status == ERROR_SUCCESS && size == 4 && value == 1)
}

fn environment(cwd: &LocalPath) -> Result<Vec<u16>, NativeRefusal> {
    let mut root = [0u16; 260];
    let size = unsafe { GetSystemWindowsDirectoryW(root.as_mut_ptr(), root.len() as u32) } as usize;
    let reason = NativeRefusal::PathCustodyUnverified;
    if size == 0 || size >= root.len() {
        return Err(reason);
    }
    let root = String::from_utf16(&root[..size]).map_err(|_| reason)?;
    let mut env = Vec::new();
    for (key, value) in [
        ("SystemRoot", root.as_str()),
        ("TEMP", cwd.as_str()),
        ("TMP", cwd.as_str()),
        ("WINDIR", root.as_str()),
    ] {
        env.extend(format!("{key}={value}").encode_utf16().chain(Some(0)));
    }
    env.push(0);
    Ok(env)
}

fn verify_image(
    child: &Child,
    expected: &LocalPath,
    identity: FileIdentity,
) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::DistributionUnverified;
    let mut name = [0u16; 512];
    let mut size = name.len() as u32;
    succeeded(unsafe {
        QueryFullProcessImageNameW(
            child.process.as_raw_handle(),
            0,
            name.as_mut_ptr(),
            &mut size,
        )
    })
    .map_err(|_| reason)?;
    let name = String::from_utf16(name.get(..size as usize).ok_or(reason)?).map_err(|_| reason)?;
    if !name.eq_ignore_ascii_case(expected.as_str())
        || PathPin::open(expected, false, false)?.identity != identity
    {
        return Err(reason);
    }
    Ok(())
}

fn verify_child_policy(child: &Child, token: &VmToken) -> Result<(), NativeRefusal> {
    let reason = NativeRefusal::JobUnverified;
    child.verify_live().map_err(|_| reason)?;
    if !matches!(child.job.active(), Ok(1)) {
        return Err(reason);
    }
    let actual = process_token(child.process.as_raw_handle(), TOKEN_QUERY)
        .map_err(|_| NativeRefusal::IdentityUnverified)?;
    token
        .verify(&actual)
        .map_err(|_| NativeRefusal::IdentityUnverified)?;
    for (policy, flags) in [
        (ProcessChildProcessPolicy, 1u32),
        (ProcessImageLoadPolicy, 0b111),
    ] {
        let mut actual = 0u32;
        succeeded(unsafe {
            GetProcessMitigationPolicy(
                child.process.as_raw_handle(),
                policy,
                ptr_of(&mut actual),
                size_of_val(&actual),
            )
        })
        .map_err(|_| reason)?;
        if actual & flags != flags {
            return Err(reason);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_slot_releases_only_a_settled_generation() {
        let trial = tempfile::tempdir().unwrap();
        let root = open_directory(trial.path().to_str().unwrap()).unwrap();
        let first = Slot::claim(&root, [1; 16]).unwrap();
        assert!(!first.state.borrow().uncertain);
        assert!(Slot::claim(&root, [2; 16]).is_err());
        assert_eq!(
            first.settle(StopDisposition::DirtyRelease),
            StopDisposition::DirtyRelease
        );
        let second = Slot::claim(&root, [2; 16]).unwrap();
        assert_eq!(
            second.settle(StopDisposition::ChargedQuarantine),
            StopDisposition::ChargedQuarantine
        );
        assert_eq!(std::fs::read(trial.path().join(SLOT)).unwrap(), [2; 16]);
        assert!(Slot::claim(&root, [3; 16]).is_err());
    }
}
