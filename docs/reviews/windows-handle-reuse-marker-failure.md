# Handle-reuse marker failure during executor integration

Codex / gpt-6.1-sol / medium observed this on the first coordinator full run
of service-executor candidate `8461a3a79609e53a4d5c6964d77891fa2ab54858`.

Command: `cargo nextest run --workspace --locked --status-level fail --final-status-level fail`.
Run ID: `a41ba4fd-714f-4da4-bd9b-2eb765934dfd`.

The unchanged test `development::windows::tests::handle_slot_reuse_is_not_inheritance`
failed at `windows.rs:774`:

```text
assertion failed: result.stdout.contains("ARX-HANDLE-REUSED-DISTINCT")
```

Earlier cleanup, identity, stop reason and exit-code assertions passed. The
parent's marker assertion did not include captured stdout/stderr, so the child's
exact reason is not established. Summary: 119/145 tests ran, 118 passed, one
failed, two skipped directly; fail-fast left 26 tests unrun. This is not a full
passing coordinator result, despite the builder's earlier 145-pass result.

`crates/arxd/src/development/windows.rs` is byte-identical to main `0ceb174`.
No change to its source or any handle guard was made. No fix, passing rerun or
cause is claimed. Diagnose using the existing real-process path and a focused
repeatable feedback loop. Keep object-identity, genuine leak, standard-handle,
Job, token and cleanup assertions intact. Do not hide the failure with retries,
skips, global serialization or a weakened marker assertion.

Separately, the new service executable validator panicked on `Cé:\tool.exe`
because its drive-prefix check sliced across a UTF-8 boundary. The regression
failed first, then passed after comparing the colon as a byte. All ten focused
service-executor tests, formatting and locked all-target Clippy passed after
that correction. It does not explain or fix the handle marker failure.

No privileged/service/owner/LSA/SCM/account/VM operation ran. Installed behavior
and dedicated-account M2 acceptance remain **unverified on real hardware**.

## Diagnosis

Anthropic / claude-opus-5-5, requested effort high, research sub-agent.
Base `da418e4b78547ebb396152a13c118112b3b525f3`. The committed result is this
document plus the candidate test-only patch
`docs/reviews/windows-handle-reuse-marker-failure.patch`. No native source is
changed in the commit; Sol applies the patch.

### Cause

The child's own `EtwRegistration` handles sometimes sit at the parent sentinel's
numeric value. Windows refuses to duplicate `EtwRegistration` objects
(`DuplicateHandle` fails with `ERROR_NOT_SUPPORTED`, 50). The fixture added in
#28 snapshots the slot with `DuplicateHandle` and reads any failure as "slot
absent". So it skips the identity comparison and the marker, and the child exits
0. This is a test-fixture defect. Production inheritance is not involved: the
object in the slot is a different type from the sentinel event, and the real
leak mutation below still fails.

Each child process held exactly 56 non-duplicable handles, all
`EtwRegistration`, at values from 56 to about 452 (6/6 scanned children). Parent
sentinel values fell between 200 and 556, so the slots overlap in about 7% of
runs.

### Feedback loop

A built copy of the unchanged unit-test binary was run by
`loop.sh <exe> <total> <parallel> <tag> [test]`. For each run it executes
`<exe> --exact development::windows::tests::handle_slot_reuse_is_not_inheritance`
through `xargs -P`, and counts each exit status as pass or fail. It keeps the logs
of failed runs. The real `run_inner` path, Job, token and handle list are used.
Nothing is mocked.

| Binary | Runs | Parallel | Failures | Symptom |
| --- | ---: | ---: | ---: | --- |
| base (unchanged) | 100 | 1 | 7 | `windows.rs:774` marker absent, every time |
| base (unchanged) | 200 | 8 | 17 | same |
| base (unchanged) | 1000 | 8 | 66 (6.6%) | same |
| instrumented | 200 | 8 | 9 | same |
| instrumented, `--nocapture` | 300 | 8 | 24 | same |

### Hypotheses and probes

The instrumentation was temporary, tagged `[DEBUG-h7c1]`, and wrote to stderr.
It printed only these fields: the numeric slot, whether the slot was occupied,
the number of events created, the kernel object type name, the result and error
code of `DuplicateHandle`, whether the slot was still valid, and whether the
parent captured the libtest banner. No owner data, paths or SIDs were printed.
The instrumentation is removed: `grep DEBUG-h7c1` finds nothing.

1. **Confirmed.** A valid slot that cannot be duplicated is read as absent.
   Prediction: failures occur exactly when the slot is valid and duplication
   fails, and forcing such a slot fails every time. Result over 300 runs: all 24
   failures had `pre_occupied=true type=EtwRegistration dup_ok=false dup_err=50
   valid_after_dup=true`. None of the 276 passes had that type. The passes
   either filled the slot with their own events (257) or found a duplicable
   foreign object there (Key 9, IRTimer 3, Thread 2, ALPC 2, IoCompletion 1,
   File 1, Event 1).
2. **Falsified.** A short-lived foreign handle closes between the validity check
   and the duplication. Every failure reported the slot still valid after the
   duplication failed.
3. **Falsified.** The parent loses the child's stdout. Every failing parent
   captured the libtest banner, and the child itself reported that duplication
   failed.
4. **Falsified.** The slot lies beyond the 4096-event fill range. Every failure
   had `created=0`: the slot was already occupied before the fill started, and
   the child's own assertion passed.
5. **Falsified.** The sentinel was really inherited in a form that cannot be
   duplicated. The slot held an `EtwRegistration`, not an event. Under a real
   leak, the inherited event duplicates normally and fails the comparison
   (mutation results below).

The review advisory raised two leads: a slot that is valid but not duplicable,
and a slot outside the 4096 range. The first is now proven. The second is
falsified for this failure.

### Controlled feedback and candidate fix

The patch changes test code only, under `cfg(test)`:

- In the child, `ARX_UNIT_UNDUPLICABLE` replaces the sentinel with the first
  slot that is valid but returns `ERROR_NOT_SUPPORTED` on duplication. It finds
  the slot through public APIs only. The new parent test
  `unduplicable_occupied_slot_is_compared_not_skipped` uses this to reproduce
  the case deterministically.
- When duplication fails with `ERROR_NOT_SUPPORTED` and the slot is still valid,
  the child calls `CompareObjectHandles` on the slot directly instead of
  skipping. It also asserts that the reference did not land on a vacated slot,
  which would fail closed. A leak still takes the duplicate path, because the
  sentinel is an event and events can be duplicated.
- The parent's marker assertion now also prints the bounded stdout and stderr.
- The snapshot is still taken before the reference is opened. The kernel
  identity comparison, standard-handle type checks, production
  `CreateProcessW`, the three-handle inheritance list, the Job, the token and
  the cleanup are all unchanged.

| Check | Runs | Parallel | Result |
| --- | ---: | ---: | --- |
| New test, seam only, without the fix | 50 | 8 | **50 failures**, the same `contains("ARX-HANDLE-REUSED-DISTINCT")` symptom |
| New test with the fix | 100 | 8 | 100 passes |
| Original test with the fix | 1000 | 8 | 1000 passes (base: 66 failures) |
| Original test with the fix, sequential | 300 | 1 | 300 passes (base: 7 of 100 failed) |
| Real leak mutation: inheritance list removed in production code, original test | 40 | 8 | **40 failures**: `unintended inherited sentinel object` |
| Real leak mutation, Job-tree handles test | 40 | 8 | **40 failures**: same message |
| Real leak mutation, new seam test | 40 | 8 | 40 passes. Expected: the seam swaps in a child-local slot, so it covers only the new branch |
| Earlier ordering guard mutation: reference opened before the snapshot, both handle tests | 80 | 8 | 80 passes. The fake-leak case is not reproduced locally, so this guard is not exercised |

With the candidate applied, these checks were run:

- `cargo fmt --all -- --check` passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `cargo nextest run --workspace --locked --status-level fail --final-status-level fail`
  run `3d61655d-9159-4f47-824e-5af61ff46ef6`: 147 run, 147 passed, 2 skipped.
  Ten more full runs gave 147 of 147 each.
- `cargo test --workspace --locked`: 147 passed, 2 ignored (the fixtures that
  are only run directly). The ignored fixtures were never run directly.

Both mutations were reverted, and the patch is the exact diff of the tested
candidate.

### Limits

- Only this Windows 11 Pro machine was used, with an unelevated current user.
  The hosted Windows runner was not re-run.
- The ETW-registration layout depends on the OS build and loader, so other
  hosts may see a different rate. The fix does not depend on the rate.
- The earlier ordering-guard scenario remains defensive and was not reproduced.
- Nothing ran through SCM, LSA, rights changes, an elevated or dedicated
  account, an installed service, a VM or the old Lab Manager. Full M2
  installed-service and dedicated-account paths remain **unverified on real
  hardware**.
- An independent review by a different provider and the hosted required checks
  are still needed before merge.
