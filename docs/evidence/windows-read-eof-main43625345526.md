# Read-side broken pipe: targeted pure regression

Builder: GPT-6.1 Sol, high; 2026-11-14. Local base `4a025ed`.
Scope: `crates/arxd/src/development/windows.rs` plus this evidence.

User-supplied ordinary-main CI: run 43625345526, main
`7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`, Windows job 132539815010,
2026-11-14 18:17:03Z, test
`development::windows::tests::actual_job_tree_timeout_cancel_shutdown_overflow_and_handles`:

```text
c=overflow r=execution_failed s=final_out_read i=0 e=109 v=1 w=- ms=- o=12288 d=0 p=0 h=160 cw=258 cc=1 ja=0 jt=2 jk=0 jv=0
```

This input localizes error 109 to final stdout ReadFile. It was not fetched or
replayed here. It gives no cause credit for historical main71.

## Feedback loop and change

Extracted the existing drain algorithm into `drain_io`, called by production
`drain`, with peek/read/failure closures. Before the fix, it retained the old
semantics: broken pipe from peek means EOF; every failed read is fatal.
The regression supplies peek availability 4096, read error 109, prior total
12288 and limit 8192. It asserts `Ok(true)`, unchanged captured bytes/total and
4096 dropped bytes. The unsuccessful-read buffer is deliberately filled to
detect accidental appending.

Ran this exact command before fixing read semantics:

```powershell
cargo test --locked --offline -p arxd --lib --target x86_64-pc-windows-msvc --target-dir target/pipe-eof development::windows::tests::drain_read_broken_pipe_preserves_overflow -- --exact --test-threads=1
```

Result: exit 1; 0 passed, 1 failed, 384 filtered out; assertion:
`left: Err(ExecutionFailed)`, `right: Ok(true)`. Harness time 0.00s.
This is deterministic reproduction of the localized error-handling symptom,
not reproduction of Windows scheduling or the complete native test.

Ranked explanations after red: inconsistent read EOF handling; corrupted prior
byte accounting; overly broad classification of other pipe errors. The fix
adds only `Err(ERROR_BROKEN_PIPE)` as read EOF, preserving prior overflow.
Other errors still call failure capture once and return ExecutionFailed.
Native wrappers capture GetLastError immediately on failed API returns.
Test-only injected-read metadata now agrees with real-read EOF handling.

The same command passed after the fix: 1 passed, 386 filtered out, 0.00s.
Three additional exact-filter invocations used the identical command above
with only the fully qualified test name replaced by each of:

- `development::windows::tests::drain_eof_and_errors_are_distinct`
- `development::windows::tests::drain_success_retains_bounds_and_counts`
- `development::windows::diag::pure::injected_metadata_all_stages_and_broken_pipe`

Each passed: 1 passed, 386 filtered out, 0.00s. Coverage includes both peek and
read EOF at totals 0/8192/12288; errors 5/6/55 remain fatal; one read capped at
4096; capture capped at limit + 4096; dropped-byte accounting; successful empty
peek and zero-byte read. The existing bookkeeping test covers all 28 stages.
No extra mutant was needed: the pre-fix regression directly rejects the old
fatal-read behavior.

Callgraph inspected before execution: the three drain tests call only
`drain_io`, their mock closures, Captured arithmetic and ordinary Rust
collections/assertions. `drain_io` has no unsafe block or native call. These
tests never call `drain`, `run_inner`, diagnostic_snapshot or native_replay.
The selected existing diag test calls observe/Scope/Invocation, arm/stage,
capture_injected/capture, record and TLS bookkeeping; its error readers are
constant closures. It never calls the native diagnostic snapshot. Exact
filters with `--lib -- --exact` select one pure test per invocation; no broad
test discovery command or fixture was executed.

Checks passed:

```powershell
cargo fmt --all -- --check
git diff --check
cargo clippy --locked --offline -p arxd --lib --tests --target x86_64-pc-windows-msvc --target-dir target/pipe-eof -- -D warnings
```

Clippy exited 0 (30.23s); compilation/lint only, no test execution. The first
format check requested wrapping one new test array; rustfmt on windows.rs
corrected it and the subsequent format check passed.

## Limits and handoff

No changes to overflow thresholds, final-drain bounds, timeout/cancel/shutdown,
handles, cleanup, quarantine or native assertions. No harness/CI/workflow/Cargo,
policy or PROGRESS edits. No native tests, replay, helper/child fixtures, native
application API calls, service/account/owner operations, network/GitHub,
dispatch, subagents or babysitter operations. Old Lab Manager and its VM were
not accessed. D7b/PR86 authority and scope are unchanged.

Native behavior is **unverified on real hardware**. Fresh independent Grok
review and normal required gates remain pending; this local commit grants no
run authority. First targeted fix for this newly localized symptom; no failed
implementation fix attempts. No hosted-CI or historical-main71 fix claim.
