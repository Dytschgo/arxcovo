# M3 consensus append cancellation flake

Local hotfix by GPT-6.1 Sol, high effort, 2026-11-16. Fresh Opus review is required before publication. No push, PR or merge was performed.

## Identity and scope

Starting parent: `388573e`. The coordinator reported Ubuntu run `43767515928` at `765df2bd1df6271ff06a961f581029544b4835d1`: `cluster::consensus::tests::append_spawned_cancellation_panic_and_poison_complete_once`, `tests.rs:550:59`, `unwrap Err(Io)`, duration 0.015s. Public logs were verified by the coordinator; this child did not fetch them.

Verified locally with `git rev-parse REV:PATH` for HEAD, `0ab5038` and the reported failing commit:

- `crates/arxd/src/cluster/consensus/tests.rs`: identical blob `168873a8e60df47e1cba7dc1275523fa1c2eaa2f`.
- `crates/arxd/src/cluster/consensus/adapter.rs`: identical blob `f16f1c25283821686c8e8fd8a57ee86ab1b07545`.

Line 550 opens `VoteLogStore` inside `disk()`; the named test starts at line 681. Registry changes in the parent are unrelated to these identical blobs. Only consensus adapter/test source and this evidence file are changed. PROGRESS, registry, workflows, Cargo files, M2 native paths and host/VM setup were not changed.

## Feedback loop and reproduction

Read AGENTS.md, KICKOFF, dytschgo-debug and its active profile, and ADR 0005. Before hypotheses, built the real named test and ran a fixed 200-invocation loop. All targets, test temporary files and captured loop logs were placed under unique `D:\arxcovo-consensus-hotfix-ffefb74e02104b11bce68e56e36954b9`. Rust 1.95.0, Windows host, ordinary local tests.

Build command (TEMP and TMP also set to the unique root's `tmp` directory):

```powershell
cargo test -p arxd --lib cluster::consensus::tests::append_spawned_cancellation_panic_and_poison_complete_once --no-run
```

The bounded harness below was saved as `repeat.ps1` in that unique D: root, alongside `target` and `tmp`. It invokes the compiled test binary directly, preserves every exit code and per-invocation log, and never retries a failed invocation. `$Test` defaults to the original fully qualified test name; probe runs use `cluster::consensus::tests::cancelled_append_callback_releases_store_before_worker_returns`.

```powershell
param([int]$Count = 200, [string]$Label = 'before',
      [string]$Test = 'cluster::consensus::tests::append_spawned_cancellation_panic_and_poison_complete_once')
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$env:TEMP = "$root\tmp"
$env:TMP = $env:TEMP
$exe = (Get-ChildItem "$root\target\debug\deps\arxd-*.exe" | Select-Object -First 1).FullName
$failed = 0
$clock = [Diagnostics.Stopwatch]::StartNew()
for ($i = 1; $i -le $Count; $i++) {
    $output = & $exe $Test --exact --nocapture 2>&1
    $code = $LASTEXITCODE
    $output | Set-Content "$root\$Label-$i.log"
    if ($code -ne 0) { $failed++; Write-Output "FAIL iteration=$i code=$code"; $output | Select-Object -Last 22 }
}
Write-Output "RESULT label=$Label test=$Test count=$Count passed=$($Count-$failed) failed=$failed seconds=$($clock.Elapsed.TotalSeconds)"
if ($failed -gt 0) { exit 1 }
```

Invocations and exact aggregate results:

| Invocation | Passed | Failed | Seconds |
| --- | ---: | ---: | ---: |
| `./repeat.ps1 -Count 200 -Label before` (unchanged source) | 200 | 0 | 18.3573083 |
| `./repeat.ps1 -Count 20 -Label probe-before -Test cluster::consensus::tests::cancelled_append_callback_releases_store_before_worker_returns` (gate/test added, no fix) | 0 | 20 | 1.0449882 |
| `./repeat.ps1 -Count 200 -Label after` | 200 | 0 | 20.6883847 |
| `./repeat.ps1 -Count 20 -Label probe-after -Test cluster::consensus::tests::cancelled_append_callback_releases_store_before_worker_returns` | 20 | 0 | 3.3507143 |

A separate first probe invocation also failed before the fix, and a separate first probe invocation passed after it. Every pre-fix probe failed with exit 101 at `tests.rs:550:59`, `called Result::unwrap() on an Err value: Io`.

The natural flake did not reproduce locally. To obtain a deterministic signal, the regression uses a test-only, store-address-scoped channel gate immediately after callback delivery. It retains the real append, manually polls then cancels its future while the log mutex is held, releases that mutex, waits for the worker to park, checks callback success, drops caller-owned stores and reopens through the existing `disk()` helper. It cuts out the unrelated panic/poison/iterator cases. The worker remains parked through the reopen assertion; dropping the release sender unblocks it even if the assertion panics. No timing sleep or retry is involved. The existing two-second timeout is used only as a deadlock guard for gate entry.

## Diagnosis and change

Ranked hypotheses after reproducing the exact error: (1) callback precedes release of the worker's strong store reference; (2) destruction remains incomplete after ownership disappears (the earlier Weak observation); (3) unrelated filesystem/database state. Prediction: dropping worker ownership before callback delivery should turn the gated reopen green without changing storage or assertions.

The blocking append closure previously called `guard.finish(...)` while still owning `store`. Cancellation discards the join future, so callback receipt cannot establish worker termination. The test could drop its own handles and enter `disk()` while the detached worker still held the file lock. The deterministic gate reproduced that interleaving 20/20 with the exact reported error.

The production change explicitly drops the worker's Arc before publishing ordinary append completion. If it owns the last reference, this also completes store/file destruction before publication. This avoids treating Weak disappearance as a destructor-completion fence. No Weak logic or node shutdown behavior is changed. The callback result, exactly-once Completion guard, panic behavior, durability operation and original test assertions remain unchanged. One fix candidate; zero failed fix candidates. Removing the explicit `drop(store)` while leaving the regression/gate restores the deterministic failure.

## Validation and limits

- Original bounded loop after fix: 200/200 pass.
- Deterministic regression after fix: 20/20 pass.
- `cargo test -p arxd --lib cluster::consensus:: -- --test-threads=4`: 65 passed, 0 failed, 461 filtered out, 11.46s. Includes original cancellation/panic/poison test, OpenRaft storage suites and three-node tests; this parent includes registry tests, which were run but not edited.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy -p arxd --lib --tests -- -D warnings`: passed (35.12s).
- `git diff --check`: passed.
- No `[DEBUG-...]` markers remain in consensus source. The channel gate is retained as regression infrastructure and compiled only under `cfg(test)`.

This proves the callback/worker ownership race exists and is fixed under a controlled real-path interleaving on Windows. It does not prove no other cause contributed to the Ubuntu CI incident. Ubuntu/macOS reruns, exact-main transplant validation and fresh independent Opus review remain for the coordinator. No external network or real cluster hardware path was exercised; LAN/Tailscale and VM paths remain **unverified on real hardware**. No timeout increases, weakened assertions or retry-based workaround.

## Coordinator exact-main transplant

The hotfix is retained as `ebdecee` on main `0ab5038d89e86e5f726fbed7e5ef6d76e64fa306`, independently of the unmerged member-registry feature. On this main-based branch, `cargo nextest run -p arxd --locked --offline cluster::consensus::` ran 64 tests: 64 passed, 492 skipped, 8.876s. `cargo fmt --all --check` and `git diff --check origin/main` passed. These are local Windows results, not hosted Ubuntu reproduction or acceptance. Independent Opus review and fresh hosted gates remain pending.
