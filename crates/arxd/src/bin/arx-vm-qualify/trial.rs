//! The closed trial graph. Authority is private to this binary crate: the
//! arxd library cannot name these types, and nothing here converts into the
//! daemon's `LaunchAuthority`.
//!
//! Every stage transition checks its prerequisites before the collector runs.
//! Effects additionally consume a disposable-store proof, a confinement plan
//! and a fresh live sample. Cleanup always runs. Only a trial whose every
//! stage passed can be sealed, and in this contract version no native
//! effect collector exists, so native trials are always incomplete.

use crate::frame::*;

/// What an evidence source supplies. The native source's store proof is
/// uninhabited in this contract version, so a native effect cannot dispatch.
pub(crate) trait Source {
    const ORIGIN: Origin;
    /// Opaque custody held for the whole trial (the native preflight pins).
    type Custody;
    /// The disposable-store proof every effect consumes.
    type Store;
}

pub(crate) struct Native;

/// The pinned stock distribution cannot prove actual opens. No native trial
/// can claim a slot until that pre-effect gate has a reviewed witness.
pub(crate) enum NoNativeStore {}

#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) type NativeCustody = arxd::vm::qualification::Baseline;
#[cfg(not(all(windows, target_arch = "x86_64")))]
pub(crate) enum NativeCustody {}

impl Source for Native {
    const ORIGIN: Origin = Origin::Native;
    type Custody = NativeCustody;
    type Store = NoNativeStore;
}

#[cfg(test)]
pub(crate) struct Fixture;

#[cfg(test)]
pub(crate) struct FixtureStore;

#[cfg(test)]
impl Source for Fixture {
    const ORIGIN: Origin = Origin::Fixture;
    type Custody = ();
    type Store = FixtureStore;
}

/// Exclusive trial authority. Not Clone, Default, Serialize or Deserialize,
/// and its fields are private to this module.
pub(crate) struct TrialAuthority<S: Source> {
    custody: S::Custody,
    binding: Binding,
    dependencies: Dependencies,
    issued: Instant2,
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl TrialAuthority<Native> {
    /// The only native constructor. It takes ownership of the held preflight
    /// custody and binds the scope and run the evidence writer drew from its
    /// own random generator before the claim, so the trial's binding is the
    /// one the writer publishes. It is infallible once the output is claimed.
    pub(crate) fn issue(
        custody: NativeCustody,
        class: Class,
        dependencies: Dependencies,
        issued: Instant2,
    ) -> Self {
        let (scope, run) = (custody.scope(), custody.run());
        Self::bind(custody, class, scope, run, dependencies, issued)
    }
}

/// Test-only authority for sources without custody. The real native source
/// holds preflight custody, so this constructor can never produce one.
#[cfg(test)]
impl<S: Source<Custody = ()>> TrialAuthority<S> {
    pub(crate) fn probe(
        class: Class,
        scope: Hash,
        run: [u8; 16],
        dependencies: Dependencies,
        issued: Instant2,
    ) -> Self {
        Self::bind((), class, scope, run, dependencies, issued)
    }
}

impl<S: Source> TrialAuthority<S> {
    fn bind(
        custody: S::Custody,
        class: Class,
        scope: Hash,
        run: [u8; 16],
        dependencies: Dependencies,
        issued: Instant2,
    ) -> Self {
        Self {
            custody,
            binding: Binding {
                class,
                scope,
                release: release(),
                run,
                dependencies: dependencies.key(scope),
                issued_utc: issued.utc,
                expires_utc: issued.utc.saturating_add(MAX_VALIDITY),
            },
            dependencies,
            issued,
        }
    }
}

pub(crate) struct Collected {
    pub started_ms: u64,
    pub finished_ms: u64,
    pub result: StageResult,
}

impl Collected {
    pub(crate) fn at(ms: u64, result: StageResult) -> Self {
        Self {
            started_ms: ms,
            finished_ms: ms,
            result,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Teardown {
    Released,
    Uncertain,
}

pub(crate) trait Clock {
    fn now(&self) -> Instant2;
    fn cancelled(&self) -> bool;
}

/// Stage collectors. The driver, never a collector, decides whether a stage
/// may run, and it counts every effect it dispatches.
pub(crate) trait Collectors<S: Source> {
    fn baseline(&mut self, stage: Stage) -> Collected;
    /// The store stage's record, and the proof only when it was produced.
    fn store(&mut self) -> (Collected, Option<S::Store>);
    fn plan(&mut self, stage: Stage) -> EffectPlan;
    /// Monotonic capture time of the live owner/reserve sample, if any.
    fn live(&mut self) -> Option<u64>;
    fn effect(&mut self, stage: Stage, plan: EffectPlan, store: &S::Store) -> Collected;
    fn cleanup(&mut self, dispatched: u16) -> Teardown;
}

/// A running trial: the held custody stays owned for the whole run while
/// the closed-graph state records results.
pub(crate) struct Trial<S: Source> {
    custody: S::Custody,
    state: State<S>,
}

impl<S: Source> Trial<S> {
    pub(crate) fn begin(authority: TrialAuthority<S>) -> Self {
        let TrialAuthority {
            custody,
            binding,
            dependencies,
            issued,
        } = authority;
        Self {
            custody,
            state: State {
                binding,
                dependencies,
                issued,
                records: Vec::new(),
                blocked_at: None,
                store: None,
                dispatched: 0,
                charge_retained: false,
                settled: false,
                late: None,
            },
        }
    }

    #[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
    pub(crate) fn custody(&self) -> &S::Custody {
        &self.custody
    }

    /// Run the closed graph to its first non-passing stage, then settle.
    pub(crate) fn drive(&mut self, collectors: &mut impl Collectors<S>, clock: &impl Clock) {
        self.state.drive(collectors, clock);
    }

    pub(crate) fn incomplete(&self) -> Incomplete {
        self.state.incomplete()
    }

    #[cfg(test)]
    pub(crate) fn records(&self) -> &[Record] {
        &self.state.records
    }

    #[cfg(test)]
    pub(crate) fn blocked_at(&self) -> Option<Stage> {
        self.state.blocked_at
    }

    #[cfg(test)]
    pub(crate) fn charge_retained(&self) -> bool {
        self.state.charge_retained
    }

    #[cfg(test)]
    pub(crate) fn late(&self) -> Option<Diagnostic> {
        self.state.late
    }

    #[cfg(test)]
    pub(crate) fn record_for_test(
        &mut self,
        stage: Stage,
        collected: Collected,
    ) -> Result<(), RunError> {
        self.state.record(stage, collected)
    }

    /// Seal a trial whose every stage passed against a complete, current
    /// dependency projection. Check cancellation, the trial budget, and
    /// binding expiry at the seal point. No writer accepts the result in
    /// this version.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn finish(self, clock: &impl Clock) -> Result<Complete<S>, RunError> {
        self.state.finish(clock)
    }
}

struct State<S: Source> {
    binding: Binding,
    dependencies: Dependencies,
    issued: Instant2,
    records: Vec<Record>,
    blocked_at: Option<Stage>,
    store: Option<S::Store>,
    dispatched: u16,
    charge_retained: bool,
    settled: bool,
    /// Latched when any completion lands after cancellation, the trial
    /// budget, expiry or a clock inconsistency. A latched trial never seals.
    late: Option<Diagnostic>,
}

impl<S: Source> State<S> {
    fn binding(&self) -> Binding {
        self.binding
    }

    fn last_finished(&self) -> u64 {
        self.records.last().map_or(0, |record| record.finished_ms)
    }

    /// A driver-owned record time. Past the trial budget the graph has
    /// already stopped; settlement is still recorded, at the budget.
    fn driver_ms(&self, now: Instant2) -> u64 {
        now.mono_ms.min(MAX_TRIAL_MS).max(self.last_finished())
    }

    /// Append one stage result in the closed order. Cleanup may follow a
    /// block; nothing else may. A failed timing check is recorded as an
    /// inconclusive result and blocks the trial.
    fn record(&mut self, stage: Stage, collected: Collected) -> Result<(), RunError> {
        let class = self.binding().class;
        let next = stages(class).get(self.records.len()).copied();
        let in_order = if stage == Stage::Cleanup {
            !self.settled
        } else {
            self.blocked_at.is_none() && next == Some(stage)
        };
        if !in_order {
            self.blocked_at.get_or_insert(stage);
            return Err(RunError::OutOfOrder);
        }
        let floor = self.last_finished();
        let timed = collected.started_ms <= collected.finished_ms
            && collected.finished_ms <= MAX_TRIAL_MS
            && collected.started_ms >= floor;
        let (started_ms, finished_ms, result) = if timed {
            (
                collected.started_ms,
                collected.finished_ms,
                collected.result,
            )
        } else {
            (
                floor,
                floor,
                StageResult::Inconclusive(Diagnostic::ClockInconsistent),
            )
        };
        let prior = self
            .records
            .last()
            .map(canonical)
            .transpose()?
            .map_or([0; 32], |bytes| digest(&bytes));
        if !result.passes(stage, S::ORIGIN) {
            self.blocked_at.get_or_insert(stage);
        }
        if stage == Stage::Cleanup {
            self.settled = true;
        }
        self.records.push(Record {
            contract: CONTRACT,
            collector: COLLECTOR,
            origin: S::ORIGIN,
            binding: self.binding(),
            stage,
            started_ms,
            finished_ms,
            prior,
            result,
        });
        if timed { Ok(()) } else { Err(RunError::Timing) }
    }

    fn gate(&self, now: Instant2, clock: &impl Clock) -> Option<Diagnostic> {
        let binding = self.binding();
        if clock.cancelled() {
            Some(Diagnostic::Cancelled)
        } else if !clocks_agree(self.issued, now) {
            Some(Diagnostic::ClockInconsistent)
        } else if now.mono_ms > MAX_TRIAL_MS || !binding.live_at(now.utc, binding.dependencies) {
            Some(Diagnostic::Expired)
        } else {
            None
        }
    }

    /// Admission for an effect: the store proof, an unblocked graph, the
    /// stage's confinement, and a live sample whose capture time the driver
    /// checks again immediately before dispatch.
    fn admit(
        &self,
        stage: Stage,
        collectors: &mut impl Collectors<S>,
    ) -> Result<(EffectPlan, u64), Diagnostic> {
        if self.store.is_none() || self.blocked_at.is_some() {
            return Err(Diagnostic::PrerequisiteMissing);
        }
        let plan = collectors.plan(stage);
        if stage.confinement().is_some_and(|limit| !plan.within(limit)) {
            return Err(Diagnostic::EffectUnconfined);
        }
        collectors
            .live()
            .map(|captured| (plan, captured))
            .ok_or(Diagnostic::StaleLiveSample)
    }

    /// The dispatch-time check, taken after planning and live collection:
    /// cancellation, clocks, budget and expiry, then live freshness.
    fn dispatchable(&self, captured: u64, at: Instant2, clock: &impl Clock) -> Option<Diagnostic> {
        self.gate(at, clock).or_else(|| {
            (captured > at.mono_ms || at.mono_ms - captured > LIVE_FRESHNESS_MS)
                .then_some(Diagnostic::StaleLiveSample)
        })
    }

    /// Run the closed graph to its first non-passing stage, then settle.
    fn drive(&mut self, collectors: &mut impl Collectors<S>, clock: &impl Clock) {
        let class = self.binding().class;
        for &stage in stages(class) {
            if stage == Stage::Cleanup || self.blocked_at.is_some() {
                break;
            }
            let now = clock.now();
            if let Some(reason) = self.gate(now, clock) {
                let _ = self.record(
                    stage,
                    Collected::at(self.driver_ms(now), StageResult::Failed(reason)),
                );
                break;
            }
            let collected = match stage.kind() {
                StageKind::Baseline => collectors.baseline(stage),
                StageKind::Store => match collectors.store() {
                    (collected, Some(proof)) if collected.result.passes(stage, S::ORIGIN) => {
                        self.store = Some(proof);
                        collected
                    }
                    // A passing record without its proof cannot admit effects.
                    (collected, None) if collected.result.passes(stage, S::ORIGIN) => Collected {
                        result: StageResult::Inconclusive(Diagnostic::PrerequisiteMissing),
                        ..collected
                    },
                    (collected, _) => collected,
                },
                StageKind::Effect => match self.admit(stage, collectors) {
                    Err(reason) => Collected::at(self.driver_ms(now), StageResult::Failed(reason)),
                    Ok((plan, captured)) => {
                        // Recheck immediately before dispatch: planning and
                        // live collection take time of their own.
                        let at = clock.now();
                        match self.dispatchable(captured, at, clock) {
                            Some(reason) => {
                                Collected::at(self.driver_ms(at), StageResult::Failed(reason))
                            }
                            None => {
                                self.dispatched = self.dispatched.saturating_add(1);
                                let store =
                                    self.store.as_ref().expect("admission requires the store");
                                collectors.effect(stage, plan, store)
                            }
                        }
                    }
                },
                StageKind::Cleanup => unreachable!("cleanup is settled below"),
            };
            // A completion after cancellation, the budget, expiry or a clock
            // inconsistency is latched: it never passes and the trial stops.
            let done = clock.now();
            let collected = match self.gate(done, clock) {
                Some(reason) => {
                    self.late.get_or_insert(reason);
                    Collected::at(self.driver_ms(done), StageResult::Failed(reason))
                }
                None => collected,
            };
            let _ = self.record(stage, collected);
        }
        self.settle(collectors, clock);
    }

    /// Teardown is always available, including after expiry, cancellation
    /// or a late completion. A settlement that ends late is latched.
    fn settle(&mut self, collectors: &mut impl Collectors<S>, clock: &impl Clock) {
        let started = self.driver_ms(clock.now());
        let teardown = collectors.cleanup(self.dispatched);
        // An uncertain teardown keeps every charge, whatever was dispatched.
        self.charge_retained = teardown != Teardown::Released;
        let end = clock.now();
        if let Some(reason) = self.gate(end, clock) {
            self.late.get_or_insert(reason);
        }
        let finished = self.driver_ms(end).max(started);
        let _ = self.record(
            Stage::Cleanup,
            Collected {
                started_ms: started,
                finished_ms: finished,
                result: StageResult::Passed(Observation::Cleanup {
                    dispatched_effects: self.dispatched,
                    released: !self.charge_retained,
                    charge_retained: self.charge_retained,
                    late: self.late.is_some(),
                }),
            },
        );
    }

    fn files(&self) -> Result<Vec<(FileKind, Vec<u8>)>, RunError> {
        group_files(&self.records)
    }

    fn complete(&self) -> bool {
        let sequence = stages(self.binding().class);
        self.settled
            && self.blocked_at.is_none()
            && self.late.is_none()
            && !self.charge_retained
            && self.records.len() == sequence.len()
            && self.records.iter().zip(sequence).all(|(record, stage)| {
                record.stage == *stage && record.result.passes(*stage, S::ORIGIN)
            })
    }

    fn incomplete(&self) -> Incomplete {
        let attempted: Vec<Stage> = self.records.iter().map(|record| record.stage).collect();
        Incomplete {
            contract: CONTRACT,
            origin: S::ORIGIN,
            binding: self.binding(),
            blocked_at: self.blocked_at,
            unattempted: stages(self.binding().class)
                .iter()
                .copied()
                .filter(|stage| !attempted.contains(stage))
                .collect(),
            unmeasured: self
                .dependencies
                .unmeasured()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            dispatched_effects: self.dispatched,
            charge_retained: self.charge_retained,
            late: self.late,
            unwritten: Vec::new(),
        }
    }

    fn finish(self, clock: &impl Clock) -> Result<Complete<S>, RunError> {
        let now = clock.now();
        let binding = self.binding();
        let dependencies = &self.dependencies;
        if !self.complete()
            || !dependencies.complete()
            || now.mono_ms > MAX_TRIAL_MS
            || now.mono_ms < self.last_finished()
            || !clocks_agree(self.issued, now)
            || !binding.live_at(now.utc, dependencies.key(binding.scope))
        {
            return Err(RunError::Incomplete);
        }
        let files = self.files()?;
        let seal = Seal {
            contract: CONTRACT,
            collector: COLLECTOR,
            origin: S::ORIGIN,
            binding,
            finished_utc: now.utc,
            manifest: files
                .iter()
                .map(|(file, bytes)| ManifestEntry {
                    file: *file,
                    length: bytes.len() as u64,
                    sha256: digest(bytes),
                })
                .collect(),
        };
        check_bundle(&seal, &files, S::ORIGIN)?;
        let seal = canonical(&seal)?;
        // Building, checking, and encoding the bundle consume trial time.
        // Recheck cancellation, budget and expiry after all of that work.
        let seal_point = clock.now();
        if seal_point.mono_ms < now.mono_ms
            || seal_point.utc < now.utc
            || self.gate(seal_point, clock).is_some()
        {
            return Err(RunError::Incomplete);
        }
        Ok(Complete {
            seal,
            files,
            _source: std::marker::PhantomData,
        })
    }
}

/// A sealed bundle. There is no route from this type to any writer.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct Complete<S: Source> {
    pub seal: Vec<u8>,
    pub files: Vec<(FileKind, Vec<u8>)>,
    _source: std::marker::PhantomData<S>,
}

/// Test-only label for one published file, used by recording sinks.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Output {
    Records(FileKind),
    Incomplete,
}

/// Each record file in manifest order with its canonical encoding.
pub(crate) fn group_files(records: &[Record]) -> Result<Vec<(FileKind, Vec<u8>)>, RunError> {
    FileKind::ALL
        .into_iter()
        .filter_map(|kind| {
            let group: Vec<&Record> = records
                .iter()
                .filter(|record| record.stage.file() == kind)
                .collect();
            (!group.is_empty()).then(|| canonical(&group).map(|bytes| (kind, bytes)))
        })
        .collect()
}

/// Where an incomplete native run is published. The sink receives the
/// records in chain order and the summary. The native sink passes only the
/// stages, times, results and summary fields to the evidence writer, which
/// derives every opaque value itself and writes the summary last.
pub(crate) trait DiagnosticSink {
    fn publish(
        &mut self,
        records: &[Record],
        dependencies: &Dependencies,
        summary: &Incomplete,
    ) -> Result<(), RunError>;
}

/// Publish an incomplete native run. Fixture trials and complete-looking
/// trials have no route here.
pub(crate) fn publish_incomplete<S: Source>(
    trial: &Trial<S>,
    sink: &mut impl DiagnosticSink,
) -> Result<(), RunError> {
    if S::ORIGIN != Origin::Native {
        return Err(RunError::Untrusted);
    }
    if !trial.state.settled || trial.state.complete() {
        return Err(RunError::Output);
    }
    sink.publish(
        &trial.state.records,
        &trial.state.dependencies,
        &trial.incomplete(),
    )
}
