# F6 natural observation proposal - NOT APPLIED OR EXECUTED

Prepared: OpenAI / Sol 6.1 / high, isolated HEAD
1689a02cf00b183b3b3dc022a555b321ce8e6334; no children.
Authority: prepare two artifacts only. Root plus an independent Sol review of
this actual patch must precede a separate authorization for one future F6.
No application, build, compilation, engine, Cargo, nextest or test ran here.
No network, publication, PROGRESS edit, fix or attempt reset is authorized.

## Baseline and diagnostic state

Failed main: 09134085dce93742d8812821dbe11834dcee7a9c.
Hosted Windows: F6/F8 initial issuance Unavailable; F7 wrong refusal, actual
variant unknown. Prior ONE local F6/F7/F8 control passed; not reproduced.
See ../reviews/m3-main-founder-f6-f7-f8-sol-disposition.md for retained evidence.
Debug skill phases 2-5 are not satisfied; this explicitly bounded preparation
supersedes its loop/injection advice. No causal hypothesis or remedy selected.
Sol shared fixes 3/3 exhausted; historical Opus 0/2 unchanged.
PR176 held; main red; M2 open; M3 unchecked; **unverified on real hardware**.

Git blob controls (same as failed main):
- found.rs: 6ffd0edd253fcd1dd083019eac67d5a9106890d4
- found_tests.rs: c2dc569f7ed761cc42f949cec8cb8115d3da13c7
- consensus.rs: aba32c1bcd226f9f45a3b133ebaa48eef3f7239c
- issue.rs: e11d4f6a84d6b56684548c1b1f95538d94134979
- Cargo.lock: 6bb1f1c0840176b4411aa7b88a469c448f34d925
Pinned rust-toolchain.toml selects 1.95.0. Pin raw source, toolchain and config
hashes again before any later admission; record then-current crate aggregate.
Patch: ../evidence/m3-founder-f6-observation.patch
SHA256: 6fb49eba004936204ec45fb113f0f89b640fb2625686546222a6c67549b7d3cd

## Exact seam and expected signal

Patch touches only found.rs and F6's initial issue expression in found_tests.rs.
Test-only task-local RefCell owns [Option<Event>; 24], length and overflow flag.
Only that one F6 issue helper call scopes capture; no global/node-pointer match.
Other tests cannot inherit task-local context; Owners tasks do not inherit it.
No new spawn, yield, wait, sleep, lock, injection, async phase wrapper or helper.
Non-test macro expansions are the original expressions; observer module and
capture call exist only in cfg(test). Original signer clock calls and timing
predicates remain in their original order. Observer adds monotonic samples.
The original ? operators, guard order, signing, permits, Owners, cancellation,
20 s test wrapper, 5 s issue budget, assertions and resource drop order remain.
The test's .unwrap() still propagates the original returned error unchanged.
No unsafe, node fields, production observer, scheduler or ownership restructure.

Six phase expressions: preflight, initialize, leaderwait, proposal, guard1,
guard2. Each records enter/Pending and exit/Ok or mapped FoundError variant.
Finaldeadline enter surrounds the existing timeout match; its exit records the
actual final returned variant, after the original final elapsed predicate.
Events carry monotonic elapsed_us from deadline minus ISSUE_BUDGET, saturating
remaining_us, static phase/edge/result names and last unmatched pending phase.
Maximum normal trace is 14 events; cap 24 never grows. Overflow invalidates it.
Only allowlisted scalar fields are formatted. BadCsr/Rejected payloads are
collapsed to variant names; underlying OpenRaft error payloads are never logged.
No CSR, DER, keys, CA markers, code/proof, store data, paths or pointers recorded.
The buffer is formatted and emitted once with [DEBUG-F6-OBS], after found returns
and its issue permit/work have dropped, before the original unwrap. No timed
phase performs formatting or synchronous logging. Output stays inside the
existing whole-test wrapper; its overhead does not extend that deadline.

Expected useful red: original initial-issue Unavailable plus finaldeadline exit.
An unmatched phase entry at that refusal localizes an incomplete expression;
a preceding exit/Unavailable localizes a completed mapped refusal. All six
exit/Ok plus final Unavailable implicates a later synchronous gap/final gate,
not a specific cause. Remaining zero means observed budget exhausted, not that
the original predicate sampled zero. Observer samples can perturb timing.
Pending denotes caller expression, never continued worker/Owners completion.
Proposal includes argument preparation: its existing inner ? can skip exit;
thus unmatched entry alone does not prove an await was pending. Fixed F6 uses
a valid singleton FoundMembers input. Do not infer worker/storage causality.

## Future admission and command - requires review and authorization

Root and independent reviewer inspect actual patch, cfg production identities,
privacy allowlist, task scoping, ? placement, lifetimes and fixed-cap semantics.
Patch has NOT been compiled or formatted by Rust tools; those claims are pending.
Any objection, unsafe/restructure need or inability to preserve invariants stops
admission. A rejected patch is not permission to make a fix or execute a probe.
A future grant must explicitly include compilation and one natural invocation.
Use a fresh isolated checkout at pinned identical crates, private checked
non-reparse cache/target/evidence directories, no shared writable Cargo cache.
Admit only with no competing cargo/rustc/nextest/arxd workload; otherwise stop.
Reuse pinned tools; offline cached dependencies only. No install or network.
Set CARGO_HOME and CARGO_TARGET_DIR to those private directories and
CARGO_NET_OFFLINE=true. No profile, runtime, timeout, helper or assertion changes.
Check local config/env for retries or overrides before admission, without
printing credentials. Record sanitized command/tool versions/source/probe hashes.
Exactly one future command (output flags retain the one emission on green/red):

```text
cargo nextest run -p arxd --lib --locked --offline -E 'test(=cluster::consensus::issue::found::found_tests::f6_adapters)' --test-threads 1 --retries 0 --fail-fast --success-output final --failure-output final
```

No F7/F8, repeat, suite, mutation, delay, stress or follow-on injection. Preserve
original exit code in evidence; log capture must not mask failure. A compile,
config, missing dependency or admission failure consumes this execution grant
and stops; no mechanical repair/retry without new review and authorization.
Green is one instrumented natural local observation, not reproduction, hosted
stability, clearance of red main or credit toward exhausted repair attempts.
Exact red localizes only this observation; a different red is separately named.
Stop after any outcome. Preserve sanitized output, UTC bounds, exit code, source,
patch/log hashes, overflow/missing-output status and process cleanup evidence.
Panic/cancellation before capture completes may emit nothing: no Drop logger or
fallback run; report missing trace. The original panic/timeout propagates.

## Preparation checks actually run

Read-only git apply --check --whitespace=error-all passed; no patch application.
In-memory reconstruction verified non-test expression identity and F6-only seam.
All 130 tracked crate files match failed main byte for byte. Ordered path-NUL,
bytes-NUL aggregate SHA256:
026f868a360d527e742e6f84dbfb00b9931ce485455a0681ddbabeff02f342ac
ASCII/UTF8 LF and whitespace/scope checks passed. No Rust verification claimed.

## Restoration and coordinator proposal

After the one command exits, inspect only the invocation's owned processes;
record survivors and stop for coordinator disposition, without killing unrelated
workloads. Do not delete caches, evidence, static VMs or backups. Retain evidence
privately; never dump arbitrary store files or broad logs. No log upload here.
Reverse only the reviewed patch after verifying its expected source hashes;
if source drift exists, stop instead of overwriting it. Verify every crate byte
against failed main and verify DEBUG-F6-OBS absent from active crate source.
Do not commit temporary source. Evidence artifacts may contain the marker.
No rerun after restoration. If compile/review requires changes, seek a new grant.
Proposed coordinator PROGRESS entry: Sol 6.1/high prepared exact unapplied F6
observation patch/plan; static checks only; independent/root review pending;
no new runtime evidence, counters/statuses unchanged. PROGRESS is unmodified.
