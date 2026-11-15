# M2 A4 native omitted-kill plan - independent Muse review

Scope: prepared head `d632863` (`docs(a4): plan fixed native omitted-kill negative`),
plan file `docs/reviews/m2-a4-native-omitted-kill-plan.md`. Sol source plan
`23b0f84` verified byte-identical (empty `git diff 23b0f84 d632863` on the plan
path; the commit-range stat delta is only `docs/PROGRESS.md` plus the retained
baseline assessment file). Read-only review: AGENTS.md, docs/KICKOFF.md, ADR0020
D9a/D10/Bounds, `exec/hierarchy/{a4,a5,setup,linux}.rs`, `exec/disposable/a4.rs`,
`tools/unix-exec-validation/driver.py` (TEST/argv/summary/harness sections),
`docs/evidence/linux-a4-baseline-run-43666413693.json`,
`docs/evidence/linux-a4-hosted-baseline-feedback.md`, and retained assessment
`e0067fd` (PASS, positive feedback only). No implementation, build, test,
dispatch, push, or native operation was performed. Baseline `8bd2e39` (PR111)
independent PASS / Windows gate pending and D7e-consumed status are retained
statements from PROGRESS and the assessment, not re-proven here.

## Standards - PASS, no findings

Docs-only review commit: one concern, small conventional commit. No production,
fixture, driver, workflow, or timeout byte is touched by this review. No product
approval flows, no publishing changes, no secrets or owner data. `git diff --check`
state is left to the final commit verification below.

## Spec - PASS conditional on one exactness fix and the clarification below

The mechanism as designed does meaningfully kill the native positive oracle
while preserving bounds:

- Positive oracle refresher: native A4 acceptance needs the real S7
  `write(runs, cgroup.kill, 1)`, a real 5 s populated wait draining to empty,
  one removal, and accept. Always-accept mutants (skipped kill, swallowed
  `KillTimeout`, second fake setup, early READY) would pass the positive
  fixture unchecked.
- Negative construction: `OmitS7` omits exactly that one kill write, returns
  `Ok`, and counts reached/omitted/forwarded. Setup then runs its unchanged
  real 5 s wait against real kernel population (`setup.rs` wait loop) and must
  yield exactly `Unavailable(KillTimeout)`. Returning `Ok` (not `Err`) is
  required and correct: `Err` would divert to `KillInterface` /
  `mechanism_unavailable` instead of the Janitor-quarantine `KillTimeout` path
  the negative is built to exercise.
- Oracle kill: the shared positive assertion runs under narrow `catch_unwind`
  after observation returns. Forwarded-kill, missing-assertion, and
  swallowed-timeout mutants produce no assertion panic, so the outer negative
  fails; only the genuine omitted-kill `KillTimeout` plus full attribution
  (one attempt / one omitted / zero forwarded, zero removals, retained literal
  owned `arx-0-0` leaf, real populated=1, live child with positive
  original-deadline remainder, no prior reap or READY) passes. Cleanup SIGKILL
  is failure-only, through the existing owned `Child` path with
  retained-identity reap, and can never earn S7 credit because the
  zero-forwarded counter and retained-leaf checks precede it.
- Bounds preserved: one real setup per entry, real monotonic clock, no second
  setup, no accelerated clock, no faked events; 64 MiB / swap 0 / pids 1
  fixture limits, absolute 15 s helper deadline with two >6 s remainder gates,
  4096-byte reads, 512-record / 64-child lists, fixed `--ignored --exact`
  one-thread argv, helper SHA/size pins, unit/profile, SHA/actor/ref fences,
  and independent finally cleanup all unchanged. Pure tests plus driver mocks
  cover both literals, unknown-name refusal, marker missing/duplicate, false
  summary/service success, and cleanup failure without launching helper or
  LinuxFs. This layering is proportionate: no optional frameworks, no manual
  overhead.

Blocking exactness fix (B1 - marker transport): the plan specifies the fixed
bounded rejection marker "via direct `std::io::Write`" to "bypass libtest
success capture". Rust `std::io::stdout` writes go through libtest output
capture and are discarded when the outer test passes, so as written the marker
may never reach `test-output` and the driver's exactly-one-marker check would
fail closed. Require one line of exactness before implementation: the marker
must be a raw file-descriptor write (e.g. `libc::write` to `STDOUT_FILENO` or
`STDERR_FILENO`, both dup2'd to `test-output` by `harness`), with exact marker
bytes fixed in the slice and the driver counting exactly one occurrence. Safe
direction (a lost marker fails the run, grants nothing), but the run cannot
pass until this is exact.

## Security - PASS, no findings

No new reachable surface. The wrapper forwards every port operation unchanged
except the single omitted kill write, rejects duplicate attempts and
other-target/other-byte writes, propagates wrapper-owned directory tags by
descriptor-relative traversal with no raw-FD export or path override, and
cannot signal or remove. Failure-only cleanup compares held/fresh dev+ino over
trusted descriptors with bounded empty observations before `rmdir`, with no
adoption. The marker carries no panic text, PIDs, or paths; the caught
assertion payload is dropped unread (`assert!(accepted)` on a copied bool, no
`#[should_panic]`, catch scope limited to that assertion). Driver fences
(exact TEST literals, SHA/actor/ref equality, unit/profile, 1 MiB bounds,
owned-only cleanup, finally-delete with 404 verification) are unchanged, and
in-fixture cleanup stays insufficient by itself.

## Pre-implementation requirements

1. Apply fix B1 (raw-fd marker exactness) to the plan.
2. Record this minimal exact ADR0020 D9a clarification before implementation
   (test-only negative, no new selector API):

   > D9a clarification (test-only omitted-kill negative): permits (i) one
   > additional literal `#[ignore]` test entry `disposable_linux_a4_omitted_kill`
   > sharing the existing spawn/seed/placement body with the positive entry and
   > fixing the negative path internally with no parameter; (ii) one test-only
   > `OmitS7` CgroupFs wrapper omitting exactly `write(runs, cgroup.kill, 1)`
   > (returning Ok, counting the attempt) plus test-only observed setup siblings
   > returning the real `SetupOutcome` with exactly one real setup per run;
   > (iii) adding the negative literal to the existing driver allowlist with
   > TEST fixed to exactly one literal at a time (implementation slice keeps
   > TEST=positive; only the separate one-run disposition candidate sets
   > TEST=negative). It does not permit arbitrary test selection, a second
   > setup per run, clock acceleration, faked events, or any production-code
   > behavior change.

3. Sequencing and pin conditions for the implementation slice: keep TEST on the
   positive literal (positive oracle intact); no `kill(`/`killpg(`/`setpriority(`
   strings in hierarchy files and no production behavior change (D10 denylist
   and production pins green); observed siblings `#[cfg(test)]` only with the
   negative path never reaping inside observation; separate reviewed one-run
   disposition binds the negative, a fresh host, bounded sanitized result, and
   finally-clear/404 with no retry or reuse.

## Verdict

The plan is sound and proportionate: one real setup under a fixed test-only
`OmitS7` wrapper, narrow assertion catch, real `KillTimeout` / population /
live-child attribution, and failure-only owned cleanup kills the native
positive oracle and preserves every stated bound. Implementation-eligible once
fix B1 and the clarification above are recorded. This review grants no run
authority and no acceptance: the native omitted-kill negative remains UNRUN
and NOT granted, and all hardware paths remain **unverified on real hardware**.
