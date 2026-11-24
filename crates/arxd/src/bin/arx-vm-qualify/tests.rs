//! Runner tests: the closed graph against injected synthetic collectors, the
//! one verifier against forged and tampered bundles, and the authority fence.

use crate::frame::*;
use crate::trial::*;
use std::{cell::Cell, rc::Rc};

const T0: Instant2 = Instant2 {
    utc: 1_800_000_000,
    mono_ms: 0,
};
/// A finish instant after every scripted record: scripted collectors stamp
/// one millisecond per call from zero, and no trial makes 1000 calls.
const DONE: Instant2 = Instant2 {
    utc: T0.utc,
    mono_ms: 1_000,
};
const SCOPE: Hash = [1; 32];
const RUN: [u8; 16] = [3; 16];

/// A named mutation applied to a known-good value.
type Mutation<T> = (&'static str, fn(&mut T));

type Files = Vec<(FileKind, Vec<u8>)>;

fn deps() -> Dependencies {
    Dependencies {
        release: release(),
        artifact: Measured::Measured([13; 32]),
        host: Measured::Measured(HostDependency {
            windows_build: 26_100,
            windows_revision: 4_000,
            machine: [14; 16],
        }),
        store: Measured::Measured(StoreDependency {
            trial_volume: 11,
            trial_volume_root: [5; 16],
            trial_volume_guid: [15; 16],
            trial_root: [6; 16],
            outer_volume: 12,
            outer_volume_root: [16; 16],
            outer_volume_guid: [17; 16],
            backing_file: [7; 16],
            vhd_virtual_bytes: 100 * GIB,
            inner_extent_bytes: 99 * GIB,
        }),
        owner_profile: Measured::Measured([8; 32]),
        distribution: Measured::Measured(DistributionDependency {
            receipt: arxd::vm::public_egress::STOCK_RECEIPT_1113_SHA256,
            manifest: stock_manifest(),
        }),
        install_media: Measured::Measured([9; 32]),
        recipes: Measured::Measured([10; 32]),
        firmware: Measured::Measured([11; 32]),
        guest_protocol: Measured::Measured(1),
        network_topology: Measured::Measured([12; 32]),
        policy: PolicyDependency::CURRENT,
    }
}

fn good(stage: Stage) -> Observation {
    match stage {
        Stage::Inputs => Observation::InputPins {
            held_roots: HELD_ROOTS,
            enrollment_private: true,
            evidence_private: true,
            trial_root_private: true,
            evidence_claimed: true,
        },
        Stage::HostBaseline => Observation::Host {
            windows_build: 26_100,
            windows_revision: 4_000,
        },
        Stage::Distribution => Observation::Distribution {
            receipt: arxd::vm::public_egress::STOCK_RECEIPT_1113_SHA256,
            receipt_bytes: STOCK_RECEIPT_BYTES,
            manifest: stock_manifest(),
            manifest_bytes: STOCK_MANIFEST_BYTES,
            payload_files: STOCK_PAYLOAD_FILES,
            payload_bytes: 300_000_000,
        },
        Stage::CustodyJobCapacityBaseline => Observation::Capacity {
            inner_caller_free_bytes: 80 * GIB,
            inner_total_free_bytes: 81 * GIB,
            inner_total_bytes: 98 * GIB,
            inner_extent_bytes: 99 * GIB,
            vhd_virtual_bytes: 100 * GIB,
            outer_caller_free_bytes: 60 * GIB,
            outer_total_free_bytes: 61 * GIB,
            backing_allocated_bytes: 100 * GIB + 8192,
            backing_length_bytes: 100 * GIB + 8192,
            disk_ceiling_bytes: DISK_CEILING,
            outer_floor_bytes: OUTER_FLOOR,
            disk_budget_bytes: 99 * GIB,
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
        stage => Observation::FixtureEffect { stage },
    }
}

/// A point inside a collector call where a test trips the clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Hook {
    /// After a stage's collector produced its result.
    Collect(Stage),
    /// While the live sample for an admitted effect is taken.
    Live(Stage),
    /// After an effect returned.
    Effect(Stage),
    /// Inside teardown.
    Cleanup,
}

/// Scripted collectors that record every call the driver makes.
struct Script {
    overrides: Vec<(Stage, StageResult)>,
    plans: Vec<(Stage, EffectPlan)>,
    live: Option<u64>,
    teardown: Teardown,
    calls: Vec<Stage>,
    effects: u16,
    cleanups: Vec<u16>,
    t: u64,
    trip: Option<(Hook, Rc<Cell<bool>>)>,
    planning: Option<Stage>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            overrides: Vec::new(),
            plans: Vec::new(),
            live: Some(0),
            teardown: Teardown::Released,
            calls: Vec::new(),
            effects: 0,
            cleanups: Vec::new(),
            t: 0,
            trip: None,
            planning: None,
        }
    }
}

impl Script {
    fn failing(stage: Stage, result: StageResult) -> Self {
        Self {
            overrides: vec![(stage, result)],
            ..Self::default()
        }
    }

    fn tripping(hook: Hook, switch: &Rc<Cell<bool>>) -> Self {
        Self {
            trip: Some((hook, switch.clone())),
            ..Self::default()
        }
    }

    fn fire(&self, hook: Hook) {
        if let Some((at, switch)) = &self.trip
            && *at == hook
        {
            switch.set(true);
        }
    }

    fn collect(&mut self, stage: Stage) -> Collected {
        self.calls.push(stage);
        let result = self
            .overrides
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, result)| result.clone())
            .unwrap_or_else(|| StageResult::Passed(good(stage)));
        self.t += 1;
        self.fire(Hook::Collect(stage));
        Collected {
            started_ms: self.t,
            finished_ms: self.t,
            result,
        }
    }
}

impl<S: Source<Store = FixtureStore>> Collectors<S> for Script {
    fn baseline(&mut self, stage: Stage) -> Collected {
        self.collect(stage)
    }

    fn store(&mut self) -> (Collected, Option<FixtureStore>) {
        (self.collect(Stage::DisposableStore), Some(FixtureStore))
    }

    fn plan(&mut self, stage: Stage) -> EffectPlan {
        self.planning = Some(stage);
        self.plans
            .iter()
            .find(|(s, _)| *s == stage)
            .map_or(EffectPlan::CONFINED, |(_, plan)| *plan)
    }

    fn live(&mut self) -> Option<u64> {
        if let Some(stage) = self.planning {
            self.fire(Hook::Live(stage));
        }
        self.live
    }

    fn effect(&mut self, stage: Stage, _plan: EffectPlan, _store: &FixtureStore) -> Collected {
        self.effects += 1;
        let collected = self.collect(stage);
        self.fire(Hook::Effect(stage));
        collected
    }

    fn cleanup(&mut self, dispatched: u16) -> Teardown {
        self.cleanups.push(dispatched);
        self.fire(Hook::Cleanup);
        self.teardown
    }
}

/// A clock that always reads one instant.
struct Script2(Instant2);

impl Script2 {
    fn fixed(now: Instant2) -> Self {
        Self(now)
    }
}

impl Clock for Script2 {
    fn now(&self) -> Instant2 {
        self.0
    }

    fn cancelled(&self) -> bool {
        false
    }
}

/// What a tripped `Switch` clock reports.
#[derive(Clone, Copy)]
enum Trip {
    /// Time jumps to this instant.
    Time(Instant2),
    /// Cancellation is requested.
    Cancel,
}

/// Reads `T0` until a collector hook trips it, independent of how many
/// times the driver reads the clock.
struct Switch {
    tripped: Rc<Cell<bool>>,
    trip: Trip,
}

impl Clock for Switch {
    fn now(&self) -> Instant2 {
        match (self.tripped.get(), self.trip) {
            (true, Trip::Time(later)) => later,
            _ => T0,
        }
    }

    fn cancelled(&self) -> bool {
        self.tripped.get() && matches!(self.trip, Trip::Cancel)
    }
}

const OVER_BUDGET: Instant2 = Instant2 {
    utc: T0.utc + MAX_TRIAL_MS / 1000 + 1,
    mono_ms: MAX_TRIAL_MS + 1000,
};

fn fixture(class: Class) -> Trial<Fixture> {
    Trial::begin(TrialAuthority::<Fixture>::probe(
        class,
        SCOPE,
        RUN,
        deps(),
        T0,
    ))
}

fn complete(class: Class) -> (Trial<Fixture>, Script) {
    let mut trial = fixture(class);
    let mut script = Script::default();
    trial.drive(&mut script, &Script2::fixed(T0));
    (trial, script)
}

fn sealed(class: Class) -> Complete<Fixture> {
    complete(class)
        .0
        .finish(&Script2::fixed(DONE))
        .expect("a complete fixture seals")
}

/// Rebuild a bundle from records: relink the chain, regroup the files and
/// recompute the manifest, as a forger with full write access would.
fn forge(mut records: Vec<Record>, origin: Origin) -> (Seal, Vec<(FileKind, Vec<u8>)>) {
    let binding = records[0].binding;
    let mut prior = [0; 32];
    for record in &mut records {
        record.origin = origin;
        record.prior = prior;
        prior = digest(&canonical(record).unwrap());
    }
    let files: Vec<(FileKind, Vec<u8>)> = FileKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let group: Vec<&Record> = records.iter().filter(|r| r.stage.file() == kind).collect();
            (!group.is_empty()).then(|| (kind, canonical(&group).unwrap()))
        })
        .collect();
    (seal_for(binding, origin, &files), files)
}

fn seal_for(binding: Binding, origin: Origin, files: &[(FileKind, Vec<u8>)]) -> Seal {
    Seal {
        contract: CONTRACT,
        collector: COLLECTOR,
        origin,
        binding,
        finished_utc: T0.utc,
        manifest: files
            .iter()
            .map(|(file, bytes)| ManifestEntry {
                file: *file,
                length: bytes.len() as u64,
                sha256: digest(bytes),
            })
            .collect(),
    }
}

fn fresh(dependencies: &Dependencies) -> Fresh<'_> {
    Fresh {
        now: T0,
        issued: T0,
        dependencies,
    }
}

#[test]
fn stage_tables_are_closed_and_class_specific() {
    let shared = &stages(Class::Test)[..15];
    for class in [Class::Test, Class::StaticOrigin, Class::Docker] {
        let sequence = stages(class);
        assert_eq!(&sequence[..15], shared);
        assert_eq!(sequence.last(), Some(&Stage::Cleanup));
        let endpoint = position(class, Stage::InstalledGuestEndpoint).unwrap();
        assert_eq!(
            endpoint,
            sequence.len() - 2,
            "endpoint follows the class boots"
        );
        for (index, stage) in sequence.iter().enumerate() {
            assert_eq!(position(class, *stage), Some(index), "no duplicate stages");
            assert!(stage.file().required(class));
        }
    }
    for (class, own, foreign) in [
        (
            Class::Test,
            Stage::RestrictedTestNetwork,
            Stage::PublicIpv4Network,
        ),
        (Class::Test, Stage::WindowsRecipe, Stage::StaticWebRecipe),
        (
            Class::StaticOrigin,
            Stage::RestrictedOriginNetwork,
            Stage::RestrictedTestNetwork,
        ),
        (
            Class::StaticOrigin,
            Stage::StaticWebRecipe,
            Stage::DockerRecipe,
        ),
        (
            Class::Docker,
            Stage::PublicIpv4Network,
            Stage::RestrictedOriginNetwork,
        ),
        (Class::Docker, Stage::DockerRecipe, Stage::UbuntuRecipe),
    ] {
        assert!(position(class, own).is_some());
        assert!(position(class, foreign).is_none());
        assert!(!foreign.file().required(class));
    }
    // The first trial is diskless and networkless, paused until consumer opens.
    for stage in [
        Stage::SuspendedChild,
        Stage::QemuPrelaunchPause,
        Stage::LoaderConsumerOpens,
    ] {
        assert_eq!(stage.confinement(), Some(EffectPlan::CONFINED));
    }
    for stage in [Stage::FirstGuestExecution, Stage::SupervisorLossRelease] {
        let limit = stage.confinement().unwrap();
        assert!(!limit.paused && limit.disks == 0 && !limit.network);
    }
    assert_eq!(Stage::DisposableStore.kind(), StageKind::Store);
    assert!(shared[..6].iter().all(|s| s.kind() == StageKind::Baseline));
    assert!(shared[7..].iter().all(|s| s.kind() == StageKind::Effect));
    assert_eq!(FileKind::Inputs.name(), "inputs.json");
    assert_eq!(
        FileKind::NetworkPublicIpv4.name(),
        "network-public-ipv4.json"
    );
}

#[test]
fn a_complete_fixture_seals_and_passes_its_structural_self_check() {
    for class in [Class::Test, Class::StaticOrigin, Class::Docker] {
        let (trial, script) = complete(class);
        assert_eq!(trial.blocked_at(), None);
        assert_eq!(trial.records().len(), stages(class).len());
        assert_eq!(script.cleanups, [stages(class).len() as u16 - 8]);
        let done = trial.finish(&Script2::fixed(DONE)).unwrap();
        let seal: Seal = decode(&done.seal).unwrap();
        assert_eq!(seal.origin, Origin::Fixture);
        for ((kind, bytes), entry) in done.files.iter().zip(&seal.manifest) {
            assert_eq!(
                (*kind, bytes.len() as u64, digest(bytes)),
                (entry.file, entry.length, entry.sha256)
            );
        }
        assert_eq!(check_bundle(&seal, &done.files, Origin::Fixture), Ok(()));
    }
}

#[test]
fn fixture_output_is_never_admissible_at_the_verifier() {
    let done = sealed(Class::Test);
    let dependencies = deps();
    let selection = Selection {
        seal_sha256: digest(&done.seal),
        scope: SCOPE,
    };
    assert_eq!(
        verify_native(&done.seal, &done.files, &selection, &fresh(&dependencies)),
        Err(RunError::Untrusted)
    );
}

#[test]
fn a_relabelled_native_forgery_fails_provenance_and_the_pinned_digest() {
    let (trial, _) = complete(Class::Test);
    let original = trial.finish(&Script2::fixed(DONE)).unwrap();
    let records: Vec<Record> = original
        .files
        .iter()
        .flat_map(|(_, bytes)| decode::<Vec<Record>>(bytes).unwrap())
        .collect();
    let mut ordered = records.clone();
    ordered.sort_by_key(|r| position(Class::Test, r.stage));
    let (seal, files) = forge(ordered, Origin::Native);
    let seal_bytes = canonical(&seal).unwrap();
    let dependencies = deps();
    // Against the independently pinned digest of the reviewed bundle.
    let pinned = Selection {
        seal_sha256: digest(&original.seal),
        scope: SCOPE,
    };
    assert_eq!(
        verify_native(&seal_bytes, &files, &pinned, &fresh(&dependencies)),
        Err(RunError::Untrusted)
    );
    // Even if the forger controls the selection, fixture-shaped effect
    // witnesses never validate under native origin.
    let forged = Selection {
        seal_sha256: digest(&seal_bytes),
        scope: SCOPE,
    };
    assert_eq!(
        verify_native(&seal_bytes, &files, &forged, &fresh(&dependencies)),
        Err(RunError::Incomplete)
    );
}

#[test]
fn no_native_observation_validates_any_store_or_effect_stage() {
    let samples: Vec<Observation> = stages(Class::Test)[..6]
        .iter()
        .map(|stage| good(*stage))
        .chain([Observation::Cleanup {
            dispatched_effects: 0,
            released: true,
            charge_retained: false,
            late: false,
        }])
        .collect();
    for class in [Class::Test, Class::StaticOrigin, Class::Docker] {
        for stage in stages(class)
            .iter()
            .filter(|s| matches!(s.kind(), StageKind::Store | StageKind::Effect))
        {
            for sample in &samples {
                assert!(
                    !sample.validates(*stage, Origin::Native),
                    "{stage:?} {sample:?}"
                );
            }
            assert!(
                !Observation::FixtureEffect { stage: *stage }.validates(*stage, Origin::Native)
            );
            assert!(
                Observation::FixtureEffect { stage: *stage }.validates(*stage, Origin::Fixture)
            );
            assert!(!StageResult::Unimplemented.passes(*stage, Origin::Fixture));
        }
    }
}

#[test]
fn every_non_passing_stage_blocks_later_work_but_cleanup_still_settles() {
    let sequence = stages(Class::Test);
    for (index, stage) in sequence[..sequence.len() - 1].iter().enumerate() {
        for result in [
            StageResult::Failed(Diagnostic::ObservationInvalid),
            StageResult::Inconclusive(Diagnostic::ObservationInvalid),
            StageResult::Unimplemented,
        ] {
            let mut trial = fixture(Class::Test);
            let mut script = Script::failing(*stage, result.clone());
            trial.drive(&mut script, &Script2::fixed(T0));
            assert_eq!(trial.blocked_at(), Some(*stage), "{stage:?} {result:?}");
            assert_eq!(script.calls, sequence[..=index], "no later work");
            let effects = sequence[..=index]
                .iter()
                .filter(|s| s.kind() == StageKind::Effect)
                .count() as u16;
            assert_eq!(script.effects, effects);
            assert_eq!(script.cleanups, [effects], "cleanup runs exactly once");
            assert_eq!(trial.records().last().unwrap().stage, Stage::Cleanup);
            let summary = trial.incomplete();
            assert_eq!(summary.blocked_at, Some(*stage));
            assert_eq!(summary.unattempted, sequence[index + 1..sequence.len() - 1]);
            assert!(matches!(
                trial.finish(&Script2::fixed(DONE)),
                Err(RunError::Incomplete)
            ));
        }
    }
}

#[test]
fn a_passed_but_invalid_observation_blocks_before_any_effect() {
    let mut trial = fixture(Class::Test);
    let mut script = Script::failing(
        Stage::ActivityReservesBaseline,
        StageResult::Passed(Observation::Activity {
            console_idle: false,
            active_cpus: 8,
            available_ram_bytes: 32 * GIB,
            owner_ram_reserve_bytes: RAM_RESERVE,
            owner_cpu_reserve: 4,
        }),
    );
    trial.drive(&mut script, &Script2::fixed(T0));
    assert_eq!(trial.blocked_at(), Some(Stage::ActivityReservesBaseline));
    assert_eq!(script.effects, 0);
    assert!(!script.calls.contains(&Stage::DisposableStore));
}

#[test]
fn effects_need_the_store_proof_before_dispatch() {
    // A failing store collector blocks every effect: zero dispatch.
    let mut trial = fixture(Class::Test);
    let mut script = Script::failing(Stage::DisposableStore, StageResult::Unimplemented);
    trial.drive(&mut script, &Script2::fixed(T0));
    assert_eq!(trial.blocked_at(), Some(Stage::DisposableStore));
    assert_eq!(script.effects, 0);
    assert_eq!(script.cleanups, [0]);
}

#[test]
fn confinement_is_checked_before_the_effect_dispatches() {
    for (stage, plan, dispatched_before) in [
        (
            Stage::SuspendedChild,
            EffectPlan {
                paused: false,
                ..EffectPlan::CONFINED
            },
            0,
        ),
        (
            Stage::SuspendedChild,
            EffectPlan {
                disks: 1,
                ..EffectPlan::CONFINED
            },
            0,
        ),
        (
            Stage::SuspendedChild,
            EffectPlan {
                network: true,
                ..EffectPlan::CONFINED
            },
            0,
        ),
        (
            Stage::LoaderConsumerOpens,
            EffectPlan {
                paused: false,
                ..EffectPlan::CONFINED
            },
            2,
        ),
        (
            Stage::FirstGuestExecution,
            EffectPlan {
                disks: 1,
                paused: false,
                network: false,
            },
            3,
        ),
        (
            Stage::SupervisorLossRelease,
            EffectPlan {
                network: true,
                paused: false,
                disks: 0,
            },
            4,
        ),
    ] {
        let mut trial = fixture(Class::Test);
        let mut script = Script {
            plans: vec![(stage, plan)],
            ..Script::default()
        };
        trial.drive(&mut script, &Script2::fixed(T0));
        assert_eq!(trial.blocked_at(), Some(stage), "{stage:?} {plan:?}");
        assert_eq!(script.effects, dispatched_before, "refused before dispatch");
        assert!(!script.calls.contains(&stage));
        let last_attempt = &trial.records()[trial.records().len() - 2];
        assert_eq!(
            last_attempt.result,
            StageResult::Failed(Diagnostic::EffectUnconfined)
        );
        assert_eq!(script.cleanups, [dispatched_before]);
    }
}

#[test]
fn a_stale_or_missing_live_sample_refuses_dispatch() {
    let later = Instant2 {
        utc: T0.utc + 10,
        mono_ms: 10_000,
    };
    for (live, now) in [
        (None, T0),
        (Some(10_000 - LIVE_FRESHNESS_MS - 1), later),
        (Some(10_001), later),
    ] {
        let mut trial = fixture(Class::Test);
        let mut script = Script {
            live,
            ..Script::default()
        };
        trial.drive(&mut script, &Script2::fixed(now));
        assert_eq!(trial.blocked_at(), Some(Stage::SuspendedChild), "{live:?}");
        assert_eq!(script.effects, 0);
        let attempt = &trial.records()[trial.records().len() - 2];
        assert_eq!(
            attempt.result,
            StageResult::Failed(Diagnostic::StaleLiveSample)
        );
    }
    // The freshness boundary is inclusive.
    let mut trial = fixture(Class::Test);
    let mut script = Script {
        live: Some(10_000 - LIVE_FRESHNESS_MS),
        ..Script::default()
    };
    trial.drive(&mut script, &Script2::fixed(later));
    assert_eq!(trial.blocked_at(), None);
}

#[test]
fn expiry_clock_rollback_and_cancellation_stop_the_graph_and_settle() {
    let expired = Instant2 {
        utc: T0.utc + MAX_VALIDITY,
        mono_ms: MAX_VALIDITY * 1000,
    };
    let backwards = Instant2 {
        utc: T0.utc - 1,
        mono_ms: 1000,
    };
    let jumped = Instant2 {
        utc: T0.utc + 60,
        mono_ms: 0,
    };
    let over_budget = Instant2 {
        utc: T0.utc + MAX_TRIAL_MS / 1000 + 1,
        mono_ms: MAX_TRIAL_MS + 1000,
    };
    for (now, reason) in [
        (expired, Diagnostic::Expired),
        (over_budget, Diagnostic::Expired),
        (backwards, Diagnostic::ClockInconsistent),
        (jumped, Diagnostic::ClockInconsistent),
    ] {
        let mut trial = fixture(Class::Test);
        let mut script = Script::default();
        trial.drive(&mut script, &Script2::fixed(now));
        assert_eq!(trial.records()[0].result, StageResult::Failed(reason));
        assert!(script.calls.is_empty());
        assert_eq!(script.cleanups, [0]);
        let cleanup = trial.records().last().unwrap();
        assert_eq!(cleanup.stage, Stage::Cleanup);
        assert!(matches!(
            cleanup.result,
            StageResult::Passed(Observation::Cleanup { .. })
        ));
    }
    // Expiry in the middle of the run, seen at the dispatch recheck: the
    // effect is refused, earlier effects are counted and still settled.
    let tripped = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::tripping(Hook::Live(Stage::LoaderConsumerOpens), &tripped);
    trial.drive(
        &mut script,
        &Switch {
            tripped,
            trip: Trip::Time(expired),
        },
    );
    assert_eq!(trial.blocked_at(), Some(Stage::LoaderConsumerOpens));
    assert_eq!(script.effects, 2);
    assert_eq!(script.cleanups, [2]);
    assert_eq!(trial.late(), Some(Diagnostic::Expired));
    // Cancellation during a baseline collector latches that stage.
    let tripped = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::tripping(Hook::Collect(Stage::CustodyJobCapacityBaseline), &tripped);
    trial.drive(
        &mut script,
        &Switch {
            tripped,
            trip: Trip::Cancel,
        },
    );
    assert_eq!(trial.blocked_at(), Some(Stage::CustodyJobCapacityBaseline));
    assert_eq!(
        trial.records()[3].result,
        StageResult::Failed(Diagnostic::Cancelled)
    );
    assert_eq!(script.cleanups, [0]);
    assert_eq!(trial.late(), Some(Diagnostic::Cancelled));
}

#[test]
fn cancellation_after_admission_refuses_the_dispatch() {
    let tripped = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::tripping(Hook::Live(Stage::QemuPrelaunchPause), &tripped);
    trial.drive(
        &mut script,
        &Switch {
            tripped,
            trip: Trip::Cancel,
        },
    );
    assert_eq!(trial.blocked_at(), Some(Stage::QemuPrelaunchPause));
    assert_eq!(script.effects, 1, "only the suspended child dispatched");
    assert!(!script.calls.contains(&Stage::QemuPrelaunchPause));
    let attempt = &trial.records()[trial.records().len() - 2];
    assert_eq!(attempt.stage, Stage::QemuPrelaunchPause);
    assert_eq!(attempt.result, StageResult::Failed(Diagnostic::Cancelled));
    assert_eq!(script.cleanups, [1], "teardown stays available");
    assert!(matches!(
        trial.finish(&Script2::fixed(DONE)),
        Err(RunError::Incomplete)
    ));
}

#[test]
fn an_effect_completing_past_the_budget_is_latched_and_still_torn_down() {
    let tripped = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::tripping(Hook::Effect(Stage::LoaderConsumerOpens), &tripped);
    trial.drive(
        &mut script,
        &Switch {
            tripped,
            trip: Trip::Time(OVER_BUDGET),
        },
    );
    assert_eq!(trial.blocked_at(), Some(Stage::LoaderConsumerOpens));
    assert_eq!(script.effects, 3);
    let attempt = &trial.records()[trial.records().len() - 2];
    assert_eq!(attempt.stage, Stage::LoaderConsumerOpens);
    assert_eq!(attempt.result, StageResult::Failed(Diagnostic::Expired));
    assert_eq!(script.cleanups, [3]);
    assert_eq!(trial.late(), Some(Diagnostic::Expired));
    assert!(matches!(
        trial.finish(&Script2::fixed(DONE)),
        Err(RunError::Incomplete)
    ));
}

#[test]
fn crossing_the_budget_during_cleanup_never_seals() {
    let tripped = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::tripping(Hook::Cleanup, &tripped);
    trial.drive(
        &mut script,
        &Switch {
            tripped,
            trip: Trip::Time(OVER_BUDGET),
        },
    );
    // Every stage passed and teardown released everything, but it ended
    // after the budget: the settlement is latched and cannot complete.
    assert_eq!(script.cleanups, [12]);
    assert_eq!(trial.late(), Some(Diagnostic::Expired));
    assert!(!trial.charge_retained());
    assert!(matches!(
        trial.records().last().unwrap().result,
        StageResult::Passed(Observation::Cleanup { late: true, .. })
    ));
    assert_eq!(trial.blocked_at(), Some(Stage::Cleanup));
    assert_eq!(trial.incomplete().late, Some(Diagnostic::Expired));
    for now in [DONE, OVER_BUDGET] {
        assert!(matches!(
            fixture_after_late_cleanup().finish(&Script2::fixed(now)),
            Err(RunError::Incomplete)
        ));
    }
    assert!(matches!(
        trial.finish(&Script2::fixed(OVER_BUDGET)),
        Err(RunError::Incomplete)
    ));
}

/// A fixed instant and a cancellation source the test controls.
struct Cancellable {
    now: Instant2,
    cancelled: Rc<Cell<bool>>,
}

impl Clock for Cancellable {
    fn now(&self) -> Instant2 {
        self.now
    }

    fn cancelled(&self) -> bool {
        self.cancelled.get()
    }
}

#[test]
fn a_cancellation_after_cleanup_and_before_finish_never_seals() {
    let cancelled = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::default();
    trial.drive(
        &mut script,
        &Cancellable {
            now: T0,
            cancelled: cancelled.clone(),
        },
    );
    // The graph ran and settled with no cancellation in sight.
    assert_eq!(script.cleanups, [12]);
    assert_eq!((trial.blocked_at(), trial.late()), (None, None));
    // Cancellation arrives after cleanup and before the seal.
    cancelled.set(true);
    assert!(matches!(
        trial.finish(&Cancellable {
            now: DONE,
            cancelled
        }),
        Err(RunError::Incomplete)
    ));
    // Control: the same settled trial seals when nothing cancels.
    let (trial, _) = complete(Class::Test);
    assert!(
        trial
            .finish(&Cancellable {
                now: DONE,
                cancelled: Rc::new(Cell::new(false)),
            })
            .is_ok()
    );
}

#[test]
fn a_complete_trial_rechecks_the_budget_at_the_seal_point() {
    struct SealClock {
        current: Cell<Instant2>,
        at_seal: Instant2,
    }

    impl Clock for SealClock {
        fn now(&self) -> Instant2 {
            self.current.replace(self.at_seal)
        }

        fn cancelled(&self) -> bool {
            false
        }
    }

    let at_ms = |mono_ms| Instant2 {
        utc: T0.utc + mono_ms / 1000,
        mono_ms,
    };
    for seal_ms in [MAX_TRIAL_MS - 1, MAX_TRIAL_MS, MAX_TRIAL_MS + 1] {
        let clock = SealClock {
            current: Cell::new(at_ms(MAX_TRIAL_MS - 1)),
            at_seal: at_ms(seal_ms),
        };
        let result = complete(Class::Test).0.finish(&clock);
        if seal_ms <= MAX_TRIAL_MS {
            assert!(result.is_ok(), "seal point: {seal_ms} ms");
        } else {
            assert!(matches!(result, Err(RunError::Incomplete)));
        }
    }
}

fn fixture_after_late_cleanup() -> Trial<Fixture> {
    let tripped = Rc::new(Cell::new(false));
    let mut trial = fixture(Class::Test);
    let mut script = Script::tripping(Hook::Cleanup, &tripped);
    trial.drive(
        &mut script,
        &Switch {
            tripped,
            trip: Trip::Time(OVER_BUDGET),
        },
    );
    trial
}

#[test]
fn a_seal_needs_a_finish_time_inside_the_budget_and_after_the_last_record() {
    // A complete fixture still refuses a finish time past the budget, even
    // with clocks that agree, or one before its last record.
    assert!(matches!(
        complete(Class::Test).0.finish(&Script2::fixed(OVER_BUDGET)),
        Err(RunError::Incomplete)
    ));
    let mut trial = fixture(Class::Test);
    let mut script = Script {
        t: 1_000,
        ..Script::default()
    };
    trial.drive(&mut script, &Script2::fixed(T0));
    assert_eq!(trial.blocked_at(), None);
    assert!(matches!(
        trial.finish(&Script2::fixed(DONE)),
        Err(RunError::Incomplete)
    ));
}

#[test]
fn an_uncertain_teardown_retains_charge_and_blocks_sealing() {
    let mut trial = fixture(Class::Test);
    let mut script = Script {
        teardown: Teardown::Uncertain,
        ..Script::default()
    };
    trial.drive(&mut script, &Script2::fixed(T0));
    assert!(trial.charge_retained());
    assert_eq!(trial.blocked_at(), Some(Stage::Cleanup));
    let summary = trial.incomplete();
    assert!(summary.charge_retained);
    assert_eq!(summary.dispatched_effects, 12);
    assert!(matches!(
        trial.finish(&Script2::fixed(DONE)),
        Err(RunError::Incomplete)
    ));
}

#[test]
fn order_class_and_timing_are_enforced_at_each_record() {
    let mut trial = fixture(Class::Test);
    let at = |ms| Collected::at(ms, StageResult::Passed(good(Stage::UbuntuRecipe)));
    assert_eq!(
        trial.record_for_test(Stage::UbuntuRecipe, at(1)),
        Err(RunError::OutOfOrder)
    );
    // Once blocked, only cleanup may follow.
    assert_eq!(
        trial.record_for_test(
            Stage::Inputs,
            Collected::at(1, StageResult::Passed(good(Stage::Inputs)))
        ),
        Err(RunError::OutOfOrder)
    );
    // A static-origin stage is out of order in a TEST trial.
    let mut trial = fixture(Class::Test);
    for stage in &stages(Class::Test)[..15] {
        trial
            .record_for_test(*stage, Collected::at(1, StageResult::Passed(good(*stage))))
            .unwrap();
    }
    assert_eq!(
        trial.record_for_test(
            Stage::RestrictedOriginNetwork,
            Collected::at(2, StageResult::Passed(good(Stage::RestrictedOriginNetwork)))
        ),
        Err(RunError::OutOfOrder)
    );
    // Backwards or reversed timing is recorded as inconclusive and blocks.
    let mut trial = fixture(Class::Test);
    let reversed = Collected {
        started_ms: 2,
        finished_ms: 1,
        result: StageResult::Passed(good(Stage::Inputs)),
    };
    assert_eq!(
        trial.record_for_test(Stage::Inputs, reversed),
        Err(RunError::Timing)
    );
    assert_eq!(trial.blocked_at(), Some(Stage::Inputs));
    assert_eq!(
        trial.records()[0].result,
        StageResult::Inconclusive(Diagnostic::ClockInconsistent)
    );
}

#[test]
fn strict_records_reject_duplicate_unknown_missing_and_noncanonical_fields() {
    let (trial, _) = complete(Class::Test);
    let record = trial.records()[1].clone();
    let encoded = canonical(&record).unwrap();
    assert_eq!(decode::<Record>(&encoded).unwrap(), record);
    let valid = String::from_utf8(encoded).unwrap();
    for changed in [
        valid.replacen("\"contract\":2", "\"contract\":2,\"contract\":2", 1),
        valid.replacen(
            "\"contract\":2",
            "\"contract\":2,\"raw_path\":\"private\"",
            1,
        ),
        valid.replacen("\"collector\":1,", "", 1),
        valid.replacen(
            "\"kind\":\"host\"",
            "\"kind\":\"host\",\"sid\":\"private\"",
            1,
        ),
        valid.replacen("{\"contract\"", "{ \"contract\"", 1),
        format!("{valid}\n"),
    ] {
        assert_ne!(changed, valid);
        assert_eq!(
            decode::<Record>(changed.as_bytes()),
            Err(RunError::Encoding),
            "{changed}"
        );
    }
    assert_eq!(
        decode::<Record>(&vec![b' '; RECORD_LIMIT + 1]),
        Err(RunError::Encoding)
    );
}

#[test]
fn every_bundle_tamper_fails_the_structural_check() {
    let done = sealed(Class::Test);
    let seal: Seal = decode(&done.seal).unwrap();
    assert_eq!(check_bundle(&seal, &done.files, Origin::Fixture), Ok(()));
    let records: Vec<Record> = {
        let mut all: Vec<Record> = done
            .files
            .iter()
            .flat_map(|(_, bytes)| decode::<Vec<Record>>(bytes).unwrap())
            .collect();
        all.sort_by_key(|r| position(Class::Test, r.stage));
        all
    };
    let rebuilt =
        |files: Vec<(FileKind, Vec<u8>)>| (seal_for(seal.binding, Origin::Fixture, &files), files);
    let mut cases: Vec<(&str, Seal, Files)> = Vec::new();

    let mut files = done.files.clone();
    files[0].1.push(b' ');
    cases.push(("byte changed without manifest", seal.clone(), files));
    let mut files = done.files.clone();
    files.push((FileKind::RecipeDocker, done.files[0].1.clone()));
    let (s, f) = rebuilt(files);
    cases.push(("surplus file", s, f));
    let mut files = done.files.clone();
    files.remove(3);
    let (s, f) = rebuilt(files);
    cases.push(("missing file", s, f));
    let mut files = done.files.clone();
    files[0].1 = vec![b' '; RECORD_LIMIT + 1];
    let (s, f) = rebuilt(files);
    cases.push(("oversize file", s, f));
    let mut files = done.files.clone();
    files.swap(1, 2);
    let (s, f) = rebuilt(files);
    cases.push(("manifest order", s, f));
    let mut s = seal.clone();
    s.manifest[2].length += 1;
    cases.push(("manifest length", s, done.files.clone()));
    let mut s = seal.clone();
    s.manifest[2].sha256[0] ^= 1;
    cases.push(("manifest hash", s, done.files.clone()));

    // A group whose records are reordered within one file, fully rehashed.
    let index = done
        .files
        .iter()
        .position(|(kind, _)| *kind == FileKind::CustodyJobCapacity)
        .unwrap();
    let mut group: Vec<Record> = decode(&done.files[index].1).unwrap();
    group.swap(0, 1);
    let mut files = done.files.clone();
    files[index].1 = canonical(&group).unwrap();
    let (s, f) = rebuilt(files);
    cases.push(("reordered group", s, f));
    // A duplicated stage inside a group.
    let mut group: Vec<Record> = decode(&done.files[index].1).unwrap();
    group.insert(1, group[0].clone());
    let mut files = done.files.clone();
    files[index].1 = canonical(&group).unwrap();
    let (s, f) = rebuilt(files);
    cases.push(("duplicate stage", s, f));
    // A noncanonical JSON spelling with a matching manifest entry.
    let mut files = done.files.clone();
    let spelled = String::from_utf8(files[1].1.clone())
        .unwrap()
        .replacen(':', ": ", 1);
    files[1].1 = spelled.into_bytes();
    let (s, f) = rebuilt(files);
    cases.push(("noncanonical spelling", s, f));
    // A broken prior link, everything else rehashed.
    let mut broken = records.clone();
    broken[4].prior[0] ^= 1;
    let mut files = Vec::new();
    for kind in FileKind::ALL {
        let group: Vec<&Record> = broken.iter().filter(|r| r.stage.file() == kind).collect();
        if !group.is_empty() {
            files.push((kind, canonical(&group).unwrap()));
        }
    }
    let (s, f) = rebuilt(files);
    cases.push(("broken prior", s, f));
    // A record moved to another scope, chain relinked.
    let mut swapped = records.clone();
    swapped[2].binding.scope = [2; 32];
    let (_, f) = forge(swapped, Origin::Fixture);
    cases.push((
        "swapped binding",
        seal_for(seal.binding, Origin::Fixture, &f),
        f,
    ));
    // A stub stage carrying another stage's witness.
    let mut stub = records.clone();
    stub[9].result = StageResult::Passed(Observation::FixtureEffect {
        stage: Stage::SuspendedChild,
    });
    let (s, f) = forge(stub, Origin::Fixture);
    cases.push(("stub stage", s, f));
    // Seal for another class.
    let mut s = seal.clone();
    s.binding.class = Class::Docker;
    cases.push(("class swapped", s, done.files.clone()));

    for (name, seal, files) in cases {
        assert!(
            check_bundle(&seal, &files, Origin::Fixture).is_err(),
            "{name}"
        );
    }
}

#[test]
fn the_verifier_checks_selection_scope_dependencies_and_time_independently() {
    let done = sealed(Class::Test);
    let records: Vec<Record> = {
        let mut all: Vec<Record> = done
            .files
            .iter()
            .flat_map(|(_, bytes)| decode::<Vec<Record>>(bytes).unwrap())
            .collect();
        all.sort_by_key(|r| position(Class::Test, r.stage));
        all
    };
    let (seal, files) = forge(records, Origin::Native);
    let bytes = canonical(&seal).unwrap();
    let dependencies = deps();
    let selection = Selection {
        seal_sha256: digest(&bytes),
        scope: SCOPE,
    };
    // Baseline: every pre-structural check passes, and the structural check
    // then refuses the fixture-shaped effect witnesses.
    assert_eq!(
        verify_native(&bytes, &files, &selection, &fresh(&dependencies)),
        Err(RunError::Incomplete)
    );
    let mut drifted = deps();
    drifted.host = Measured::Measured(HostDependency {
        windows_build: 26_100,
        windows_revision: 4_001,
        machine: [14; 16],
    });
    let mut unmeasured = deps();
    unmeasured.install_media = Measured::Unmeasured;
    let late = Instant2 {
        utc: T0.utc + MAX_VALIDITY,
        mono_ms: MAX_VALIDITY * 1000,
    };
    let rolled_back = Instant2 {
        utc: T0.utc + 10,
        mono_ms: 100_000,
    };
    let cases: Vec<(&str, Selection, Dependencies, Instant2, Instant2)> = vec![
        (
            "other scope",
            Selection {
                scope: [2; 32],
                ..selection
            },
            deps(),
            T0,
            T0,
        ),
        ("dependency drift", selection, drifted, T0, T0),
        ("unmeasured dependency", selection, unmeasured, T0, T0),
        (
            "expired at the exclusive boundary",
            selection,
            deps(),
            late,
            T0,
        ),
        ("UTC behind monotonic", selection, deps(), rolled_back, T0),
        (
            "other issue time",
            selection,
            deps(),
            T0,
            Instant2 {
                utc: T0.utc - 1,
                mono_ms: 0,
            },
        ),
    ];
    for (name, selection, dependencies, now, issued) in cases {
        let fresh = Fresh {
            now,
            issued,
            dependencies: &dependencies,
        };
        assert_eq!(
            verify_native(&bytes, &files, &selection, &fresh),
            Err(RunError::Untrusted),
            "{name}"
        );
    }
    let mut wrong_release = seal.clone();
    wrong_release.binding.release[0] ^= 1;
    let bytes = canonical(&wrong_release).unwrap();
    let selection = Selection {
        seal_sha256: digest(&bytes),
        scope: SCOPE,
    };
    assert_eq!(
        verify_native(&bytes, &files, &selection, &fresh(&dependencies)),
        Err(RunError::Untrusted)
    );
}

#[test]
fn every_dependency_participates_in_the_invalidation_key() {
    let base = deps();
    assert!(base.complete());
    let key = base.key(SCOPE);
    assert_ne!(base.key([2; 32]), key, "the key is scoped");
    let drifts: [Mutation<Dependencies>; 14] = [
        ("release", |d| d.release[0] ^= 1),
        ("artifact", |d| d.artifact = Measured::Measured([0; 32])),
        ("host build", |d| {
            d.host = Measured::Measured(HostDependency {
                windows_build: 26_200,
                windows_revision: 4_000,
                machine: [14; 16],
            })
        }),
        ("host revision", |d| {
            d.host = Measured::Measured(HostDependency {
                windows_build: 26_100,
                windows_revision: 4_001,
                machine: [14; 16],
            })
        }),
        ("host machine", |d| {
            d.host = Measured::Measured(HostDependency {
                windows_build: 26_100,
                windows_revision: 4_000,
                machine: [0; 16],
            })
        }),
        ("store", |d| {
            if let Measured::Measured(store) = &mut d.store {
                store.backing_file[0] ^= 1;
            }
        }),
        ("owner profile", |d| {
            d.owner_profile = Measured::Measured([0; 32])
        }),
        ("distribution", |d| {
            d.distribution = Measured::Measured(DistributionDependency {
                receipt: [0; 32],
                manifest: stock_manifest(),
            })
        }),
        ("install media", |d| {
            d.install_media = Measured::Measured([0; 32])
        }),
        ("recipes", |d| d.recipes = Measured::Measured([0; 32])),
        ("firmware", |d| d.firmware = Measured::Measured([0; 32])),
        ("guest protocol", |d| {
            d.guest_protocol = Measured::Measured(2)
        }),
        ("network topology", |d| {
            d.network_topology = Measured::Measured([0; 32])
        }),
        ("policy", |d| d.policy.outer_floor_bytes += 1),
    ];
    for (name, drift) in drifts {
        let mut changed = deps();
        drift(&mut changed);
        assert_ne!(changed.key(SCOPE), key, "{name}");
        let binding = Binding {
            class: Class::Test,
            scope: SCOPE,
            release: release(),
            run: RUN,
            dependencies: key,
            issued_utc: T0.utc,
            expires_utc: T0.utc + MAX_VALIDITY,
        };
        assert!(!binding.live_at(T0.utc, changed.key(SCOPE)), "{name}");
    }
    let mut store_fields = 0;
    let base_store = match &base.store {
        Measured::Measured(store) => store.clone(),
        Measured::Unmeasured => unreachable!(),
    };
    let store_drifts: [fn(&mut StoreDependency); 10] = [
        |s| s.trial_volume += 1,
        |s| s.trial_volume_root[0] ^= 1,
        |s| s.trial_volume_guid[0] ^= 1,
        |s| s.trial_root[0] ^= 1,
        |s| s.outer_volume += 1,
        |s| s.outer_volume_root[0] ^= 1,
        |s| s.outer_volume_guid[0] ^= 1,
        |s| s.backing_file[0] ^= 1,
        |s| s.vhd_virtual_bytes += 1,
        |s| s.inner_extent_bytes += 1,
    ];
    for drift in store_drifts {
        let mut store = base_store.clone();
        drift(&mut store);
        let mut changed = deps();
        changed.store = Measured::Measured(store);
        assert_ne!(changed.key(SCOPE), key);
        store_fields += 1;
    }
    assert_eq!(store_fields, 10);
    // Any unmeasured dependency keeps the projection incomplete, including
    // an explicitly absent reviewed artifact.
    let mut native_like = deps();
    native_like.owner_profile = Measured::Unmeasured;
    native_like.network_topology = Measured::Unmeasured;
    assert!(!native_like.complete());
    assert_eq!(
        native_like.unmeasured(),
        ["owner_profile", "network_topology"]
    );
    let mut no_artifact = deps();
    no_artifact.artifact = Measured::Unmeasured;
    assert!(!no_artifact.complete());
    assert_eq!(no_artifact.unmeasured(), ["artifact"]);
}

#[test]
fn validity_is_seven_days_exclusive_and_clocks_must_agree() {
    let binding = Binding {
        class: Class::Test,
        scope: SCOPE,
        release: release(),
        run: RUN,
        dependencies: [4; 32],
        issued_utc: 100,
        expires_utc: 100 + MAX_VALIDITY,
    };
    assert!(binding.live_at(100, [4; 32]));
    assert!(binding.live_at(99 + MAX_VALIDITY, [4; 32]));
    assert!(!binding.live_at(100 + MAX_VALIDITY, [4; 32]));
    assert!(!binding.live_at(99, [4; 32]));
    assert!(!binding.live_at(101, [5; 32]));
    assert!(
        !Binding {
            expires_utc: 101 + MAX_VALIDITY,
            ..binding
        }
        .live_at(101, [4; 32])
    );
    assert!(
        !Binding {
            release: [0; 32],
            ..binding
        }
        .live_at(101, [4; 32])
    );
    assert!(
        !Binding {
            run: [0; 16],
            ..binding
        }
        .live_at(101, [4; 32])
    );
    assert!(
        !Binding {
            scope: [0; 32],
            ..binding
        }
        .live_at(101, [4; 32])
    );

    let start = Instant2 {
        utc: 1_000,
        mono_ms: 5_000,
    };
    assert!(clocks_agree(start, start));
    assert!(clocks_agree(
        start,
        Instant2 {
            utc: 1_010,
            mono_ms: 15_000 + CLOCK_SKEW_MS
        }
    ));
    assert!(!clocks_agree(
        start,
        Instant2 {
            utc: 1_010,
            mono_ms: 15_001 + CLOCK_SKEW_MS
        }
    ));
    assert!(!clocks_agree(
        start,
        Instant2 {
            utc: 999,
            mono_ms: 5_000
        }
    ));
    assert!(!clocks_agree(
        start,
        Instant2 {
            utc: 1_000,
            mono_ms: 4_999
        }
    ));
    assert!(!clocks_agree(
        start,
        Instant2 {
            utc: 1_100,
            mono_ms: 5_000
        }
    ));
}

#[test]
fn bounded_reads_check_the_declared_length_before_allocating() {
    assert_eq!(read_bounded(&b"{}"[..], 2), Ok(b"{}".to_vec()));
    assert_eq!(read_bounded(&b"{}"[..], 0), Err(RunError::Encoding));
    assert_eq!(
        read_bounded(&b"{}"[..], RECORD_LIMIT as u64 + 1),
        Err(RunError::Encoding)
    );
    assert_eq!(
        read_bounded(&b"{} "[..], 2),
        Err(RunError::Encoding),
        "surplus bytes"
    );
    assert_eq!(
        read_bounded(&b"{"[..], 2),
        Err(RunError::Encoding),
        "short read"
    );
    // An endless reader cannot drive allocation past the declared bound.
    assert_eq!(
        read_bounded(std::io::repeat(b' '), 64),
        Err(RunError::Encoding)
    );
}

struct Recording {
    writes: Vec<(Output, Vec<u8>)>,
}

impl DiagnosticSink for Recording {
    fn publish(
        &mut self,
        records: &[Record],
        _dependencies: &Dependencies,
        summary: &Incomplete,
    ) -> Result<(), RunError> {
        for (kind, bytes) in group_files(records)? {
            self.writes.push((Output::Records(kind), bytes));
        }
        self.writes.push((Output::Incomplete, canonical(summary)?));
        Ok(())
    }
}

#[test]
fn fixture_trials_have_no_route_to_the_diagnostic_sink() {
    let mut trial = fixture(Class::Test);
    let mut script = Script::failing(Stage::QemuPrelaunchPause, StageResult::Unimplemented);
    trial.drive(&mut script, &Script2::fixed(T0));
    let mut sink = Recording { writes: Vec::new() };
    assert_eq!(
        publish_incomplete(&trial, &mut sink),
        Err(RunError::Untrusted)
    );
    assert!(sink.writes.is_empty());
}

/// A test-only source with native origin and the uninhabited native store,
/// for exercising the publication path without host custody.
struct NativeProbe;

impl Source for NativeProbe {
    const ORIGIN: Origin = Origin::Native;
    type Custody = ();
    type Store = NoNativeStore;
}

/// Baseline-only collectors over the uninhabited native store, mirroring the
/// native glue: the store is unimplemented and no effect can dispatch.
struct BaselineOnly(Script);

impl<S: Source<Store = NoNativeStore>> Collectors<S> for BaselineOnly {
    fn baseline(&mut self, stage: Stage) -> Collected {
        self.0.collect(stage)
    }
    fn store(&mut self) -> (Collected, Option<NoNativeStore>) {
        self.0.calls.push(Stage::DisposableStore);
        (Collected::at(100, StageResult::Unimplemented), None)
    }
    fn plan(&mut self, _stage: Stage) -> EffectPlan {
        EffectPlan::CONFINED
    }
    fn live(&mut self) -> Option<u64> {
        Some(0)
    }
    fn effect(&mut self, _stage: Stage, _plan: EffectPlan, store: &NoNativeStore) -> Collected {
        match *store {}
    }
    fn cleanup(&mut self, dispatched: u16) -> Teardown {
        self.0.cleanups.push(dispatched);
        Teardown::Released
    }
}

fn assert_path_free(bytes: &[u8]) {
    let text = String::from_utf8(bytes.to_vec())
        .unwrap()
        .to_ascii_lowercase();
    for canary in ["s-1-", ":\\\\", ":/", "canary"] {
        assert!(!text.contains(canary), "{canary} in {text}");
    }
}

#[test]
fn a_failed_publication_reports_a_typed_path_free_retry_rule() {
    let line = crate::failure_line(&RunError::Output);
    assert!(line.starts_with("Output:"), "{line}");
    assert!(line.contains("fresh empty directory"), "{line}");
    assert_path_free(line.as_bytes());
    assert_eq!(crate::failure_line(&RunError::Incomplete), "Incomplete");
}

#[test]
fn a_blocked_native_run_publishes_only_incomplete_diagnostics_with_the_summary_last() {
    let mut unmeasured = deps();
    unmeasured.owner_profile = Measured::Unmeasured;
    let mut trial = Trial::begin(TrialAuthority::<NativeProbe>::probe(
        Class::Test,
        SCOPE,
        RUN,
        unmeasured,
        T0,
    ));
    let mut collectors = BaselineOnly(Script::default());
    trial.drive(&mut collectors, &Script2::fixed(T0));
    assert_eq!(trial.blocked_at(), Some(Stage::DisposableStore));
    assert_eq!(collectors.0.cleanups, [0]);
    let mut sink = Recording { writes: Vec::new() };
    publish_incomplete(&trial, &mut sink).unwrap();
    let outputs: Vec<Output> = sink.writes.iter().map(|(output, _)| *output).collect();
    assert_eq!(
        outputs,
        [
            Output::Records(FileKind::Inputs),
            Output::Records(FileKind::HostBaseline),
            Output::Records(FileKind::Distribution),
            Output::Records(FileKind::CustodyJobCapacity),
            Output::Records(FileKind::ActivityReserves),
            Output::Records(FileKind::WhpxQmpEndpoint),
            Output::Records(FileKind::Cleanup),
            Output::Incomplete,
        ]
    );
    for (_, bytes) in &sink.writes {
        assert!(bytes.len() <= RECORD_LIMIT);
        assert_path_free(bytes);
    }
    let summary: Incomplete = decode(&sink.writes.last().unwrap().1).unwrap();
    assert_eq!(summary.blocked_at, Some(Stage::DisposableStore));
    assert_eq!(summary.dispatched_effects, 0);
    assert!(!summary.charge_retained);
    assert_eq!(summary.unmeasured, ["owner_profile"]);
    assert_eq!(summary.unattempted, stages(Class::Test)[7..19]);
    assert!(matches!(
        trial.finish(&Script2::fixed(DONE)),
        Err(RunError::Incomplete)
    ));
}

// Trait-absence checks: the generic fallback is the only candidate unless
// the forbidden implementation exists, which makes inference ambiguous and
// fails to compile.
macro_rules! assert_not_impl {
    ($ty:ty: $($tr:path),+) => {{
        $({
            trait AmbiguousIfImpl<A> {
                fn check() {}
            }
            impl<T: ?Sized> AmbiguousIfImpl<()> for T {}
            #[allow(dead_code)]
            struct Invalid;
            impl<T: ?Sized + $tr> AmbiguousIfImpl<Invalid> for T {}
            <$ty as AmbiguousIfImpl<_>>::check();
        })+
    }};
}

trait AmbiguousIfFrom<T, Marker> {
    fn check() {}
}
impl<T, U> AmbiguousIfFrom<U, ()> for T {}
impl<T: From<U>, U> AmbiguousIfFrom<U, u8> for T {}

#[test]
fn trial_authority_is_private_inert_and_never_launch_authority() {
    use arxd::vm::runtime::LaunchAuthority;
    assert_not_impl!(TrialAuthority<Native>: Clone, Default, serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl!(Trial<Native>: Clone, Default, serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl!(Complete<Native>: Clone, Default, serde::Serialize, serde::de::DeserializeOwned);
    assert_not_impl!(NoNativeStore: Clone, Default, serde::de::DeserializeOwned);
    <LaunchAuthority as AmbiguousIfFrom<TrialAuthority<Native>, _>>::check();
    <TrialAuthority<Native> as AmbiguousIfFrom<LaunchAuthority, _>>::check();
    <LaunchAuthority as AmbiguousIfFrom<Complete<Native>, _>>::check();
    <TrialAuthority<Native> as AmbiguousIfFrom<Binding, _>>::check();
    <TrialAuthority<Native> as AmbiguousIfFrom<Dependencies, _>>::check();
    <Complete<Native> as AmbiguousIfFrom<Complete<Fixture>, _>>::check();
    <Trial<Native> as AmbiguousIfFrom<Trial<Fixture>, _>>::check();
    // The library's held baseline converts into neither authority.
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        use arxd::vm::qualification::Baseline;
        <LaunchAuthority as AmbiguousIfFrom<Baseline, _>>::check();
        <TrialAuthority<Native> as AmbiguousIfFrom<Baseline, _>>::check();
        assert_not_impl!(LaunchAuthority: TryFrom<Baseline>);
        assert_not_impl!(Baseline: Clone, Default, serde::Serialize, serde::de::DeserializeOwned);
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
mod native_glue {
    use super::*;
    use crate::native::{NativeCollectors, dependencies, diagnostic_file, project, refused};
    use arxd::vm::qualification::{
        Activity, DiagnosticFile, Distribution, Host, Inputs, Limits, Observation as Timed,
        Preflight, StoreIdentity, Taken,
    };
    use arxd::vm::runtime::NativeRefusal;

    fn timed<T>(started_ms: u64, result: T) -> Timed<T> {
        Timed {
            started_ms,
            finished_ms: started_ms + 1,
            result,
        }
    }

    fn taken<T>(started_ms: u64, result: T) -> Taken<T> {
        Taken::Observed(timed(started_ms, result))
    }

    fn capacity_facts() -> Limits {
        Limits {
            inner_caller_free_bytes: 80 * GIB,
            inner_total_free_bytes: 81 * GIB,
            inner_total_bytes: 98 * GIB,
            inner_extent_bytes: 99 * GIB,
            vhd_virtual_bytes: 100 * GIB,
            outer_caller_free_bytes: 60 * GIB,
            outer_total_free_bytes: 61 * GIB,
            backing_allocated_bytes: 100 * GIB + 8192,
            backing_length_bytes: 100 * GIB + 8192,
            disk_ceiling_bytes: DISK_CEILING,
            outer_floor_bytes: OUTER_FLOOR,
            disk_budget_bytes: 99 * GIB,
            empty_job_memory_limit_bytes: JOB_MEMORY_BYTES,
            empty_job_cpu_rate: JOB_CPU_RATE,
        }
    }

    fn preflight(console_idle: bool, available_ram_bytes: u64, whpx: bool) -> Preflight {
        collect_preflight(console_idle, available_ram_bytes, whpx, || {
            taken(6, capacity_facts())
        })
    }

    fn collect_preflight(
        console_idle: bool,
        available_ram_bytes: u64,
        whpx: bool,
        collect_limits: impl FnOnce() -> Taken<Limits>,
    ) -> Preflight {
        Preflight {
            inputs: timed(
                0,
                Inputs {
                    held_roots: 5,
                    enrollment_private: true,
                    evidence_private: true,
                    trial_root_private: true,
                    evidence_claimed: true,
                },
            ),
            host: taken(
                2,
                Host {
                    windows_build: 26_100,
                    windows_revision: 4_000,
                    machine: [14; 16],
                },
            ),
            distribution: taken(
                4,
                Distribution {
                    receipt: arxd::vm::public_egress::STOCK_RECEIPT_1113_SHA256,
                    receipt_bytes: STOCK_RECEIPT_BYTES,
                    manifest: stock_manifest(),
                    manifest_bytes: STOCK_MANIFEST_BYTES,
                    payload_files: STOCK_PAYLOAD_FILES,
                    payload_bytes: 300_000_000,
                },
            ),
            limits: collect_limits(),
            activity: taken(
                8,
                Activity {
                    console_idle,
                    active_cpus: 8,
                    available_ram_bytes,
                    owner_ram_reserve_bytes: RAM_RESERVE,
                    owner_cpu_reserve: 4,
                },
            ),
            whpx_present: taken(10, whpx),
            store: StoreIdentity {
                trial_volume: 11,
                trial_volume_root: [5; 16],
                trial_volume_guid: [15; 16],
                trial_root: [6; 16],
                outer_volume: 12,
                outer_volume_root: [16; 16],
                outer_volume_guid: [17; 16],
                backing_file: [7; 16],
            },
        }
    }

    fn run(p: &Preflight) -> (Trial<NativeProbe>, Recording) {
        let mut trial = Trial::begin(TrialAuthority::<NativeProbe>::probe(
            Class::Test,
            SCOPE,
            RUN,
            dependencies(p),
            T0,
        ));
        let mut collectors = NativeCollectors::new(p, std::time::Instant::now());
        trial.drive(&mut collectors, &Script2::fixed(T0));
        let mut sink = Recording { writes: Vec::new() };
        publish_incomplete(&trial, &mut sink).unwrap();
        (trial, sink)
    }

    #[test]
    fn the_native_glue_refuses_stock_before_claiming_a_slot_or_dispatching() {
        let p = preflight(true, 32 * GIB, true);
        let (trial, sink) = run(&p);
        assert_eq!(trial.blocked_at(), Some(Stage::DisposableStore));
        let records = trial.records();
        assert_eq!(records.len(), 8);
        for record in &records[..6] {
            assert!(
                record.result.passes(record.stage, Origin::Native),
                "{:?}",
                record.stage
            );
        }
        assert_eq!(
            records[6].result,
            StageResult::Failed(Diagnostic::DistributionUnverified)
        );
        assert_eq!(records[7].stage, Stage::Cleanup);
        let names: Vec<&str> = sink
            .writes
            .iter()
            .map(|(output, _)| diagnostic_file(*output).name())
            .collect();
        assert_eq!(names.last(), Some(&"incomplete.json"));
        assert!(!names.contains(&"qualification.json"));
        for (_, bytes) in &sink.writes {
            assert_path_free(bytes);
        }
        let summary: Incomplete = decode(&sink.writes.last().unwrap().1).unwrap();
        assert_eq!(
            summary.unmeasured,
            [
                "artifact",
                "owner_profile",
                "install_media",
                "recipes",
                "firmware",
                "guest_protocol",
                "network_topology"
            ]
        );
        assert_eq!(summary.dispatched_effects, 0);
        assert!(matches!(
            trial.finish(&Script2::fixed(DONE)),
            Err(RunError::Incomplete)
        ));
    }

    #[test]
    fn measured_stock_preflight_records_refusal_without_creating_any_job() {
        // Use the same collector that orders the native read and Job probe.
        // Supply raw capacity facts and a counting factory, then let the
        // collector produce the observation that goes through the graph.
        let reads = Cell::new(0);
        let jobs = Cell::new(0);
        let origin = std::time::Instant::now() - std::time::Duration::from_millis(6);
        let p = collect_preflight(true, 32 * GIB, true, || {
            arxd::vm::qualification::stock_capacity(
                origin,
                || {
                    reads.set(reads.get() + 1);
                    Ok((capacity_facts(), ()))
                },
                || {
                    jobs.set(jobs.get() + 1);
                    Ok(())
                },
            )
            .0
        });
        let (trial, sink) = run(&p);
        assert_eq!(reads.get(), 1);
        assert_eq!(jobs.get(), 0);
        assert_eq!(trial.incomplete().dispatched_effects, 0);
        assert_eq!(trial.blocked_at(), Some(Stage::CustodyJobCapacityBaseline));
        let (_, bytes) = sink
            .writes
            .iter()
            .find(|(output, _)| *output == Output::Records(FileKind::CustodyJobCapacity))
            .unwrap();
        let records: Vec<Record> = decode(bytes).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].result,
            StageResult::Failed(Diagnostic::DistributionUnverified)
        );
    }

    #[test]
    fn negative_native_observations_become_typed_failures() {
        for (p, stage, reason) in [
            (
                preflight(false, 32 * GIB, true),
                Stage::ActivityReservesBaseline,
                Diagnostic::OwnerActive,
            ),
            (
                preflight(true, RAM_RESERVE, true),
                Stage::ActivityReservesBaseline,
                Diagnostic::ReservesShort,
            ),
            (
                preflight(true, 32 * GIB, false),
                Stage::WhpxCapability,
                Diagnostic::WhpxAbsent,
            ),
        ] {
            let (trial, sink) = run(&p);
            assert_eq!(trial.blocked_at(), Some(stage));
            let attempt = &trial.records()[trial.records().len() - 2];
            assert_eq!(attempt.result, StageResult::Failed(reason));
            assert_eq!(sink.writes.last().unwrap().0, Output::Incomplete);
        }
    }

    #[test]
    fn the_writer_and_sink_share_one_file_contract() {
        for kind in FileKind::ALL {
            assert_eq!(diagnostic_file(Output::Records(kind)).name(), kind.name());
        }
        assert_eq!(
            diagnostic_file(Output::Incomplete),
            DiagnosticFile::Incomplete
        );
        assert_eq!(
            STOCK_MANIFEST_SHA256,
            arxd::vm::qualification::STOCK_MANIFEST_1113_SHA256
        );
        // The writer derives the same release the runner binds.
        assert_eq!(release(), arxd::vm::qualification::release_digest());
    }

    #[test]
    fn refused_post_claim_observations_settle_as_typed_failures() {
        type Refuse = fn(&mut Preflight);
        let cases: [(Refuse, Stage, Diagnostic, &str); 5] = [
            (
                |p| {
                    p.host = Taken::Refused {
                        at_ms: 3,
                        reason: NativeRefusal::UnsupportedPlatform,
                    }
                },
                Stage::HostBaseline,
                Diagnostic::UnsupportedPlatform,
                "host",
            ),
            (
                |p| {
                    p.distribution = Taken::Refused {
                        at_ms: 5,
                        reason: NativeRefusal::DistributionUnverified,
                    }
                },
                Stage::Distribution,
                Diagnostic::DistributionUnverified,
                "distribution",
            ),
            (
                |p| {
                    p.limits = Taken::Refused {
                        at_ms: 7,
                        reason: NativeRefusal::DiskCeilingUnverified,
                    }
                },
                Stage::CustodyJobCapacityBaseline,
                Diagnostic::DiskCeilingUnverified,
                "store",
            ),
            (
                |p| {
                    p.activity = Taken::Refused {
                        at_ms: 9,
                        reason: NativeRefusal::OwnerActivityUnverified,
                    }
                },
                Stage::ActivityReservesBaseline,
                Diagnostic::OwnerActivityUnverified,
                "",
            ),
            (
                |p| {
                    p.whpx_present = Taken::Refused {
                        at_ms: 11,
                        reason: NativeRefusal::WhpxUnverified,
                    }
                },
                Stage::WhpxCapability,
                Diagnostic::WhpxUnverified,
                "",
            ),
        ];
        for (refuse, stage, diagnostic, dependency) in cases {
            let mut p = preflight(true, 32 * GIB, true);
            refuse(&mut p);
            let (trial, sink) = run(&p);
            assert_eq!(trial.blocked_at(), Some(stage), "{stage:?}");
            let attempt = &trial.records()[trial.records().len() - 2];
            assert_eq!(attempt.stage, stage);
            assert_eq!(attempt.result, StageResult::Failed(diagnostic));
            assert_eq!(trial.records().last().unwrap().stage, Stage::Cleanup);
            let (last, bytes) = sink.writes.last().unwrap();
            assert_eq!(*last, Output::Incomplete, "{stage:?}");
            let summary: Incomplete = decode(bytes).unwrap();
            assert_eq!(summary.blocked_at, Some(stage));
            assert!(summary.unwritten.is_empty());
            if !dependency.is_empty() {
                assert!(
                    summary.unmeasured.iter().any(|name| name == dependency),
                    "{stage:?}"
                );
            }
        }
        // Refusals outside the preflight's observations map to one code.
        for reason in [
            NativeRefusal::Disabled,
            NativeRefusal::IdentityUnverified,
            NativeRefusal::InstallerDeadlineExpired,
        ] {
            assert_eq!(refused(reason), Diagnostic::OtherRefusal);
        }
    }

    #[test]
    fn the_native_projection_binds_exact_host_and_store_identities() {
        let p = preflight(true, 32 * GIB, true);
        let projected = dependencies(&p);
        // No reviewed artifact pin exists yet: explicitly absent.
        assert_eq!(projected.artifact, Measured::Unmeasured);
        assert_eq!(
            projected.host,
            Measured::Measured(HostDependency {
                windows_build: 26_100,
                windows_revision: 4_000,
                machine: [14; 16],
            })
        );
        assert_eq!(
            projected.store,
            Measured::Measured(StoreDependency {
                trial_volume: 11,
                trial_volume_root: [5; 16],
                trial_volume_guid: [15; 16],
                trial_root: [6; 16],
                outer_volume: 12,
                outer_volume_root: [16; 16],
                outer_volume_guid: [17; 16],
                backing_file: [7; 16],
                vhd_virtual_bytes: 100 * GIB,
                inner_extent_bytes: 99 * GIB,
            })
        );
        // The machine identity binds the key but never reaches a record.
        assert_eq!(
            project(&p, Stage::HostBaseline).result,
            StageResult::Passed(Observation::Host {
                windows_build: 26_100,
                windows_revision: 4_000,
            })
        );
    }

    /// Every diagnostic, by an exhaustive successor chain: a new variant
    /// fails to compile until it is placed here.
    fn all_diagnostics() -> Vec<Diagnostic> {
        use Diagnostic as D;
        let next = |d: D| match d {
            D::OwnerActive => Some(D::ReservesShort),
            D::ReservesShort => Some(D::WhpxAbsent),
            D::WhpxAbsent => Some(D::ObservationInvalid),
            D::ObservationInvalid => Some(D::Expired),
            D::Expired => Some(D::ClockInconsistent),
            D::ClockInconsistent => Some(D::Cancelled),
            D::Cancelled => Some(D::StaleLiveSample),
            D::StaleLiveSample => Some(D::PrerequisiteMissing),
            D::PrerequisiteMissing => Some(D::EffectUnconfined),
            D::EffectUnconfined => Some(D::TeardownUncertain),
            D::TeardownUncertain => Some(D::UnsupportedPlatform),
            D::UnsupportedPlatform => Some(D::DistributionUnverified),
            D::DistributionUnverified => Some(D::PathCustodyUnverified),
            D::PathCustodyUnverified => Some(D::JobUnverified),
            D::JobUnverified => Some(D::WhpxUnverified),
            D::WhpxUnverified => Some(D::DiskCeilingUnverified),
            D::DiskCeilingUnverified => Some(D::OwnerActivityUnverified),
            D::OwnerActivityUnverified => Some(D::ReservesUnverified),
            D::ReservesUnverified => Some(D::OtherRefusal),
            D::OtherRefusal => None,
        };
        std::iter::successors(Some(D::OwnerActive), |d| next(*d)).collect()
    }

    #[test]
    fn every_writer_encoding_passes_the_library_schema() {
        use arxd::vm::qualification::{RECORD_COLLECTOR, RECORD_CONTRACT, schema_accepts};
        assert_eq!((CONTRACT, COLLECTOR), (RECORD_CONTRACT, RECORD_COLLECTOR));
        let binding = Binding {
            class: Class::Test,
            scope: SCOPE,
            release: release(),
            run: RUN,
            dependencies: [4; 32],
            issued_utc: T0.utc,
            expires_utc: T0.utc + MAX_VALIDITY,
        };
        let encode = |stage: Stage, result: StageResult| {
            canonical(&vec![Record {
                contract: CONTRACT,
                collector: COLLECTOR,
                origin: Origin::Native,
                binding,
                stage,
                started_ms: 1,
                finished_ms: MAX_TRIAL_MS,
                prior: [9; 32],
                result,
            }])
            .unwrap()
        };
        let accepts = |kind: FileKind, bytes: &[u8]| {
            schema_accepts(diagnostic_file(Output::Records(kind)), bytes)
        };
        let diagnostics = all_diagnostics();
        assert_eq!(diagnostics.len(), 20);
        for class in [Class::Test, Class::StaticOrigin, Class::Docker] {
            for &stage in stages(class) {
                for &diagnostic in &diagnostics {
                    for result in [
                        StageResult::Failed(diagnostic),
                        StageResult::Inconclusive(diagnostic),
                    ] {
                        assert!(accepts(stage.file(), &encode(stage, result)), "{stage:?}");
                    }
                }
                let bytes = encode(stage, StageResult::Unimplemented);
                for kind in FileKind::ALL {
                    assert_eq!(accepts(kind, &bytes), kind == stage.file(), "{stage:?}");
                }
            }
        }
        let cleanup = |released, late| Observation::Cleanup {
            dispatched_effects: 12,
            released,
            charge_retained: !released,
            late,
        };
        for (stage, observation) in stages(Class::Test)[..6]
            .iter()
            .map(|stage| (*stage, good(*stage)))
            .chain([
                (Stage::Cleanup, cleanup(true, false)),
                (Stage::Cleanup, cleanup(false, true)),
            ])
        {
            let bytes = encode(stage, StageResult::Passed(observation));
            assert!(accepts(stage.file(), &bytes), "{stage:?}");
        }
        // A fixture witness never reaches the native writer.
        let witness = Observation::FixtureEffect {
            stage: Stage::SuspendedChild,
        };
        assert!(!accepts(
            FileKind::CustodyJobCapacity,
            &encode(Stage::SuspendedChild, StageResult::Passed(witness))
        ));
        // A synthetic endpoint smuggled into a writer record is refused.
        let mut smuggled = encode(Stage::Inputs, StageResult::Passed(good(Stage::Inputs)));
        let at = smuggled.len() - 2;
        smuggled.splice(at..at, br#","endpoint":"192.0.2.7:443""#.iter().copied());
        assert!(!accepts(FileKind::Inputs, &smuggled));

        let unmeasured: Vec<String> = Dependencies {
            artifact: Measured::Unmeasured,
            host: Measured::Unmeasured,
            store: Measured::Unmeasured,
            owner_profile: Measured::Unmeasured,
            distribution: Measured::Unmeasured,
            install_media: Measured::Unmeasured,
            recipes: Measured::Unmeasured,
            firmware: Measured::Unmeasured,
            guest_protocol: Measured::Unmeasured,
            network_topology: Measured::Unmeasured,
            ..deps()
        }
        .unmeasured()
        .into_iter()
        .map(str::to_owned)
        .collect();
        assert_eq!(unmeasured.len(), 10);
        let summary = |late, blocked_at, unwritten: Vec<FileKind>| Incomplete {
            contract: CONTRACT,
            origin: Origin::Native,
            binding,
            blocked_at,
            unattempted: stages(Class::Test)[7..].to_vec(),
            unmeasured: unmeasured.clone(),
            dispatched_effects: 0,
            charge_retained: true,
            late,
            unwritten,
        };
        let incomplete = |value: &Incomplete| {
            schema_accepts(DiagnosticFile::Incomplete, &canonical(value).unwrap())
        };
        assert!(incomplete(&summary(None, None, Vec::new())));
        assert!(incomplete(&summary(
            None,
            Some(Stage::Cleanup),
            FileKind::ALL.to_vec()
        )));
        for &late in &diagnostics {
            assert!(incomplete(&summary(
                Some(late),
                Some(Stage::DisposableStore),
                Vec::new()
            )));
        }
    }
}
