# A4 native omitted-S7-kill: minimal implementation plan

2026-11-15. Design sub-agent: OpenAI gpt-6.1-sol, high. Planning only;
no implementation, build, helper/native launch, network, dispatch or publication.

## Starting facts and scope

Read AGENTS.md, KICKOFF, ADR0020 D9a (all five bindings and bounds), ADR0019
D7e, brief A4/A6, actual disposable/hierarchy A4, A5, setup, LinuxFs, driver,
manual workflow, packaged unit/Standard profile and fixed helper source/README.
The README's historical “TEST stays A5”/“pending fixture” text is stale;
actual driver TEST selects A4 and the native fixture is implemented.
Baseline source `a328060000b8cb7bb389d39f8785f96f6bcb0c25`, tree
`48b92d548fb6ef5373f7f477ac8bda2f0cb04717`, ran as attempt1/job132666674807
in run43666413693: success in 73s, first/final null, cleanup confirmed;
reviewed-SHA variable deletion independently returned 404. See
[feedback and blob pins](../evidence/linux-a4-hosted-baseline-feedback.md) and
[bounded record](../evidence/linux-a4-baseline-run-43666413693.json).
Independent baseline assessment is pending: these facts grant no A4 acceptance.
D7e is consumed; native omitted-kill is **unrun and NOT granted**.

## One source shape

Add one ignored `disposable_linux_a4_omitted_kill` beside the positive fixture.
Extract its existing spawn/seed/placement body into a private shared function;
the two literal test entry points fix the positive or negative path internally.
No CLI/env/path/mutation parameter, caller command or product API is added.
One narrow explicit clarification is needed before implementation: D9a's “no
new selector API” must permit this private fixed case and its literal addition
to the existing driver allowlist; it must not permit arbitrary selection.

In `exec/hierarchy/a4.rs` add test-only `OmitS7<F: CgroupFs>` for the negative
path. Forward every port operation unchanged except `write(runs,cgroup.kill,1)`:
record one reached/omitted attempt and return Ok without forwarding that write.
Use wrapper-owned directory tags propagated by descriptor-relative traversal
of the validated unit/runs path, never a raw-FD export or path override; reject
duplicate attempts, other kill targets or bytes. All reads remain real LinuxFs
reads. Seed/place with the original LinuxFs before wrapping; wrap the held
runs/leaf handles for observations. The wrapper cannot signal or remove early.
LinuxFs production adapter, setup algorithm, helper image/source/pins, package,
hardening, deadlines and historical A1/A2/A5 assertions stay unchanged.

The current `a4::finish` returns only bool, and `a5::finish` hides SetupOutcome.
In hierarchy `a5.rs` extract its post-setup checks into a shared private body;
keep `finish` behavior, and add a test-only observed sibling returning the
actual SetupOutcome with the same acceptance bool. Both use exactly one setup.
In hierarchy `a4.rs` likewise retain `finish` and share an observed body that
records completed preconditions and that outcome; preserve short-circuit order,
all A5 evidence/S9/literal-absence checks and the SIGKILL-before-deadline check.
This is fixed observation data, not a pluggable mutation/restart framework.

## Native sequence and assertion kill

In disposable `a4.rs`, retain helper descriptor/hash/size checks, E4 fork/exec,
handshake FD discipline, exclusive seed and actual one-task membership check.
Keep 64 MiB/swap0/pids1 before placement, original absolute 15s deadline,
populated runs/leaf, empty runs procs, sole MainPID supervisor and >6s remaining
both before release and immediately before the single actual production setup.
The negative executes that setup through OmitS7 over LinuxFs with the real
monotonic clock: S7 reaches the port, its kernel write is omitted, real population
persists through the unchanged five-second wait, yielding exactly KillTimeout.
Never fake events, accelerate the native clock or use a second setup.

Run the positive `assert!(accepted)` at a dedicated small assertion function
under catch_unwind **after** observation returns. Catch only this assertion,
not spawn/preflight/setup/cleanup; do not use a broad #[should_panic]. The same
assertion function serves the positive entry. Require an assertion panic,
preconditions complete, exactly one S7 attempt/one omitted/zero forwarded kill,
Unavailable(KillTimeout), zero removals, retained literal owned leaf, real
populated=1 and child.live() with positive original-deadline remainder after
setup. Require no success reap/READY before rejection. A false bool alone, an
earlier refusal, unexpected Ready, expiry, early death or any other panic fails.
The retained child must still be unreaped and sole live member at attribution.
Emit a fixed bounded rejection marker via direct std::io::Write after checks
(bypass libtest success capture); no panic text/PIDs/paths. The assertion is killed
by the native omitted effect, rather than merely testing a model's false result.

Only after acceptance has failed, explicitly use Child's existing owned
SIGKILL/waitpid cleanup through a checked method; make Drop its fallback.
Require successful retained-identity reap, empty leaf/runs, then remove only
the still-matching exclusively owned arx-0-0 leaf and confirm literal absence.
Use a narrow cfg(test) LinuxFs cleanup method comparing held/fresh dev+ino,
trusted descriptors and bounded empty observations before rmdir; no adoption.
Cleanup failure fails the negative; never turn cleanup SIGKILL into S7 credit.
Only then send READY and hold identity-ack, so existing driver MainPID identity
checks still apply. Driver finally-stop/cgroup disappearance/helper/runtime
cleanup must independently confirm cleanup; in-fixture cleanup is insufficient.

## Closed driver and deterministic coverage

In `tools/unix-exec-validation/driver.py`, add only the literal negative name
to standard_profile/a4_selected and set fixed TEST to it for the later candidate.
Keep exact --ignored/--exact/one-thread argv, helper delivery, source/workflow
SHA/actor/ref fences, unit/profile, bounds and existing finally cleanup.
Require exactly one fixed rejection marker in negative mode as well as existing
exact test-ok, one-pass/zero-failure summary, real systemd success/zero exit,
identity acknowledgement and confirmed cleanup. Positive mode keeps its oracle.
The ignored outer negative passes only after witnessing the inner assertion
failure and checked cleanup; normal CI stays green and never runs native tests.
Do not accept FAILED summaries, expected nonzero service exits or continue-on-error.

Pure tests in hierarchy A4/A5 use Fake/Manual/MockChild with the same wrapper and
observed join: require KillTimeout/one attempt/no forwarded kill/no removal;
normal forwarding still accepts with existing evidence and SIGKILL checks.
Reject missed/duplicate/wrong-target attempts, earlier refusal, swallowed timeout,
Ready, empty events, expired/early-dead child, missing assertion panic and cleanup
failure. Driver mocks in test_driver.py and mutation_check.py cover both literals,
unknown-name refusal before mutation, negative marker missing/duplicate, false
summary/service success and cleanup failure. No pure test launches helper/LinuxFs.

## Delivery and remaining gap

Later: independent different-provider source review, green required checks and
coordinator merge; pin actual main squash/tree and exact changed execution blobs.
A separate reviewed one-run disposition must bind this fixed negative and fresh
hosted host, bounded sanitized result, finally-clear/404 and no reuse/retry.
Routine fresh hosted execution needs no extra owner approval overhead. This note
grants none. Baseline assessment and native negative assessment remain separate.
M2/A4/full F/G/real-Runner PA6 remain OPEN; installed/dedicated paths are
**unverified on real hardware**. Finish this bounded gap, then resume feature work.
