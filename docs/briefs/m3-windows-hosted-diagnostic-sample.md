# Brief: M3 Windows hosted diagnostic sample (one non-gating step)

Status: **proposed, corrected** (Anthropic / Claude Opus 5.5 / high, design sub-agent, 2026-11-16) on `9f3ee35`, which sits on main `58d06c1` (PR #153 D1). This revision closes Sol review B1–B4 and the output-grammar requirement (`docs/reviews/m3-hosted-sample-sol-contract-review.md`).
This brief is docs only. Nothing was built, run or dispatched. Sol must recheck it before any sample workflow change.
It authorizes no fix and no re-enable. The eight sampled Windows ignores and their IDs stay unchanged. The accepted [whole-fixture contract](m3-windows-fixture-quarantine.md) pins 24 Windows ignores: 13 three_node fixture tests plus 11 issuance fixture tests. **Coverage loss: R1–R5 and all 11 issuance tests run in neither the Windows gate nor this sample; Ubuntu/macOS must execute all 24.** The bounded non-gating sample is an explicit exception for only the original eight, without Windows acceptance credit. The pure attribution unit remains required everywhere; three D1 controls stay globally ignored unchanged. M3 stays unchecked, and Windows three-node is unverified on hosted Windows and **unverified on real hardware**. M2 stays open: A7 is NO-GO, and A12 is consumed with no retry.

## Verified inputs

- **Windows job timing.** Job `133123407634` (run `43814521932`, PR153 head `53a7ff2`), read through the jobs API with whole-second timestamps: job `started_at` 06:06:27, `Set up job` 06:06:28–06:06:30, first user step starts 06:06:30 (3 s after job start). Clippy 87 s, nextest 281 s, `npx playwright install --with-deps --only-shell chromium` **151 s**, Electron smoke 10 s. The last regular step ends 06:16:21 (594 s); post steps and `Complete job` end 06:16:23, and the job `completed_at` is 06:16:25 (4 s after). Total 598 s against the 720 s cap. That is one job, not a distribution. The coordinator's 708 s figure for the last main Windows run was not re-read.
- **nextest 0.9.151.** Local `cargo-nextest 0.9.151` matches the CI pin. Read from `nextest-runner 0.129.0`, `nextest-metadata 0.15.0`, `cargo-nextest 0.9.151` and `quick-junit 0.8.0` source:
  - Flags and profile keys as before (`--config-file`, `-P`, `--run-ignored only`, `-E`, `--user-config-file none`, `--cargo-quiet`, `--no-tests fail`, `list -T json`; `global-timeout`, `slow-timeout`, `retries`, `fail-fast`, `test-threads`, `junit.*`). Default `leak-timeout` is 200 ms.
  - **Global timeout** (`dispatcher.rs`): cancel reason `GlobalTimeout`, broadcast as a shutdown with `ShutdownEvent::TERMINATE`, which on Windows is `Interrupt` (`signal.rs`). With a non-zero grace period, `windows.rs` waits for the grace period, then calls `TerminateJobObject`; a killed test is `Fail{Abort(JobObject)}`, JUnit type `test abort`. Tests not yet started emit no event and get **no** `<testcase>`. Final stats are `Cancelled` or `Failed`, so the exit code is **100**.
  - **Per-test slow timeout** on Windows ignores the grace period: `TerminateJobObject` is called at `period × terminate-after`; JUnit type `test timeout`.
  - **JUnit write** (`aggregator/junit.rs`): written once, on `RunFinished`, with `File::create` (truncate, not atomic) then `serialize`. A write error becomes `TestRunnerExecuteErrors`, which exits **110**. So a normal exit 0 or 100 means a complete file was written. If nextest is killed before or during `RunFinished`, the file is missing or truncated. `quick-junit` strips ANSI escapes and XML-invalid control characters from all strings, so test output cannot make a complete file malformed.
  - **JUnit types** (`non_success_kind_and_type`): `test failure with exit code N` (N ≥ 1; negative Windows codes become aborts), `test abort`, `test timeout`, `execution failure`, `test exited with code 0, but leaked handles so was marked failed`, plus the `(leaked handles)` suffix on failure/abort. A pass that leaks with the default `on-timeout` is a success with no failure element.
  - Exit codes: 0 OK; 100 test run failed or cancelled; 4, 94, 96, 101, 102, 104, 110 for no tests, filterset, setup, build/panic, metadata, list and write-output failures.
- **Workflow constraints.** `scripts/ci-workflow.test.mjs` pins one `timeout-minutes: 12`, one job-wide `env:` block, no other `CARGO_*` token and the exact playwright line. `tools/ci/test_windows_cluster_quarantine.py` pins eight sampled ignores, five unsampled three_node ignores and eleven issuance ignores.
- **D1 output.** `diagnostics.rs` at `58d06c1` prints one `[DIAG-3n] pass …` line on success and, from a drop guard, a dump of at most 8192 bytes on failure. Its line forms are the grammar below. Other test output (panics, `RPC counters`, `R6 …`) can contain paths and is never released.

## Budget and C1

A fixed +90 s step is **not robust**: 598 + 90 = 688 s, and 708 + 90 = 798 s exceeds 720 s. Raising the timeout or bypassing it is forbidden.
- **C1 comes first and is separately approved** for Sol to build: keep `--with-deps` off Windows only, pin both lines, keep every smoke and the 12 min cap. Its savings are measured on its PR run against the 598 s reference and on the next main run. Under 60 s abandons this sample; 60–100 s needs an explicit coordinator evidence decision; at least 100 s supports the next sample-contract review. C1 is not implemented by this brief, and **no sample workflow is implemented until Sol rechecks this brief**.
- **Admission is conservative and measured, not a guarantee.** The workflow exposes no job-start timestamp this design can rely on. Calling the jobs API from the step adds a network and token dependency to a required job; reading `Runner.Worker` creation time needs a walk of non-owned processes and undocumented timer semantics. Both are rejected.
- **Rule.** The anchor is the first user step, before checkout (measured 3 s after job start, ≤ 5 s with second rounding). E = now − T0 + 15 s. Run only if E + 90 s (step bound) + 30 s (measured 4 s of post steps) ≤ 720 s, i.e. E ≤ 600 s. Otherwise `outcome=skipped_budget`, exit 0; a skip is not a sample.
- **Residual risk.** If pre-anchor setup exceeds 15 s, post steps exceed 30 s, or the interpreter stalls past the 90 s design bound, the existing 720 s cap still cancels the job and the required job goes red. Kill-on-close ownership (below) still reaps the tree. `e_s` and the job API timings of each sampled run are recorded so the allowances can be checked.

## Sample contract (Opus implements after C1 and a passing Sol recheck)

**Selection.** All eight tests at `-j 4`, `retries 0`, one sample per run (the PR push, then each main CI); no rotation, `-j 1`, test groups, manual reruns or dispatches. Eight tests alone are less loaded than the full workspace, so a green sample proves nothing.

**Workflow: two added steps.** Every existing step, the cap, the env block, the quarantine guard and the five gates stay unchanged. No `continue-on-error`, step `timeout-minutes`, `env:` or `CARGO_` token.
- **First user step** `Anchor Windows diagnostic sample budget` (`if: runner.os == 'Windows'`, `shell: pwsh`), before checkout: appends `ARX_DIAG_T0=<unix seconds>` to `$env:GITHUB_ENV`.
- **Last step** after Electron smoke, `Windows three-node diagnostic sample` (Windows only, `shell: pwsh`), as **one line** (B1): `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py' && python -I -B tools/ci/windows_diag_sample.py`. In PowerShell 7, `&&` runs the right side only if the left native command exited 0, so a failing wrapper test leaves a non-zero `$LASTEXITCODE`, which the runner's appended exit check turns into a failed step. Two lines would mask it.

**Bounds.** One monotonic step deadline D = start + 90 s. Validation ≤ 2 s; `list` ≤ 20 s; `run` external deadline = min(run start + 60 s, D − 8 s); 8 s is reserved for kill, join and verify. nextest `global-timeout = "50s"`; `slow-timeout = {period="24s", terminate-after=2, grace-period="2s"}`: the per-test kill is at 48 s (grace ignored for timeouts), above D1's internal 45 s; the 2 s grace applies to global-timeout cancellation, so nextest's own worst case is about 52 s plus the 200 ms leak timeout and the JUnit write.

**`tools/ci/windows-diag-sample.nextest.toml`, profile `diag-sample`:** `retries=0`, `fail-fast=false`, `test-threads=4`, `success-output`/`failure-output="never"`, `status-level`/`final-status-level="none"`, the timeouts above, and `[profile.diag-sample.junit]` with `path="junit.xml"` and both store-output keys `true`.

**Commands.** Same cargo arguments as the gating step, so the dev/debug-0 build is reused, and `--workspace` avoids `-p` feature drift:

```
cargo nextest {list -T json | run} --workspace --locked --cargo-quiet --user-config-file none \
  --config-file tools/ci/windows-diag-sample.nextest.toml -P diag-sample --run-ignored only --no-tests fail \
  -E 'package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/) & not test(=cluster::consensus::three_node::r1_apply_wait_deadline) & not test(=cluster::consensus::three_node::r2_isolated_read_refusal) & not test(=cluster::consensus::three_node::r3_store_wait_deadline_and_heartbeat) & not test(=cluster::consensus::three_node::r4_read_admission_and_caller_abort) & not test(=cluster::consensus::three_node::r5_shutdown_read_owners)'
```

**Inventory (gates before `run`).** In the `list` JSON, exactly one suite (binary-id `arxd`, kind `lib`) has matches; its matched names equal `cluster::consensus::three_node::` + each `EXPECTED` name imported from `test_windows_cluster_quarantine.py`; all eight are `ignored: true`; nothing else matches. Each of R1–R5 is `ignored: true` with an `expression` mismatch, excluded by five fixed exact-name terms. Issuance is outside the namespace. The exclusion equals the guard's `WINDOWS_UNSAMPLED` inventory in tests only; runtime argv remains built from constants.

## Ownership and cancellation (Windows Job Object, stdlib `ctypes` only)

- **Job.** Created before any spawn: unnamed, non-inheritable handle, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, no `BREAKAWAY_OK`. Cargo's job and nextest's per-test jobs nest inside it.
- **Interpreter.** The child runs `sys.executable` and the wrapper refuses a venv (`sys.prefix != sys.base_prefix`), whose redirector would add an unowned process.
- **Handlers first.** `SIGINT`/`SIGBREAK` handlers that only set a flag are installed before the first spawn, so a cancel cannot interrupt the spawn-to-assign section.
- **Child handshake (B4).** Per phase, `Popen([exe, "-I", "-B", script, "--child", phase, privdir])` with stdin a pipe and stdout/stderr `DEVNULL`. Before the go byte the child has no descendants: it starts a 5 s watchdog (`threading.Timer` → `os._exit(3)`), then reads exactly one byte. EOF (parent died, since the non-inheritable write end closes) or any byte other than `G` exits without launching. Only after `G` does it cancel the watchdog and build argv from module constants.
- **Parent order.** Spawn, then `AssignProcessToJobObject`, then `IsProcessInJob(child, job)` must be true, then check the cancel flag, then write `G`. If assign or the check fails, or the flag is set, the parent calls `TerminateProcess` on its own `Popen` handle, waits ≤ 2 s, records the reason, and only then returns (`job` gate, or `cancelled`). An owned child that does not exit in 2 s gates `cleanup`. A hard-killed wrapper before assign leaves the child to EOF or its watchdog, and the runner's tree kill; after assign, `KILL_ON_JOB_CLOSE` reaps the tree.
- **Deadline, cancel or normal end.** `TerminateJobObject` once; wait ≤ 5 s for the child; poll `JobObjectBasicAccountingInformation.ActiveProcesses` every 100 ms for ≤ 3 s until 0; close the handle. `orphans=<n>` is the count alive before the terminate.
- **Forbidden:** `taskkill`, `os.kill`, psutil, any name or PID walk. Only the wrapper's own `Popen` handle and job are touched.
- **Paths and reads.** `privdir` = `mkdtemp` under `RUNNER_TEMP`. Refuse a reparse point on `target`, `target/nextest`, `target/nextest/diag-sample`, `junit.xml` or `privdir` (`lstat` + `FILE_ATTRIBUTE_REPARSE_POINT`). Delete a stale `junit.xml` before `run`; remove it and `privdir` afterwards. Reads are capped at 4 MiB each.

## Outcome split (B2): test behavior never gates

**Diagnostic (exit 0, no product credit):** everything the eight tests can cause.
- `all_pass`; `diag_fail` (at least one test not `pass`, including `not_run` at exit 100 and `abort` from global-timeout cancellation).
- `diag_unparsed`: exit 0/100 but JUnit over 4 MiB or not well-formed. Source says a complete file cannot be malformed by test output; it is still diagnostic so the parser can never redden the job on test content. No test lines are released.
- `diag_incomplete`: the wrapper's external deadline killed `run`. JUnit is then missing or truncated and is **not parsed**; only the `sample` line is released. This conflates a nextest stall with test-caused stalls; `run_ms` is the only evidence.
- `skipped_budget`; `cancelled` (signal, after verified cleanup).

**Gating (exit 1, `ARXDIAG v1 guard reason=<enum>`):** selection, ownership and wrapper integrity only.
- `anchor` (T0 missing, not an integer, in the future or older than 720 s), `platform` (not Windows, or venv), `reparse`.
- `job` (create, limit, assign or `IsProcessInJob` failed; child already reaped), `spawn`, `cleanup` (`ActiveProcesses` not 0, or owned child not exited).
- `list_exit`, `list_parse`, `inventory`, `deadline_list` (list runs no test bodies; an overrun means a rebuild or tool drift).
- `nextest_exit`: `run` exit outside {0, 100} when the wrapper did not kill it (101, 110, 4, …).
- `junit_missing`: exit 0/100 with no file, which source says cannot happen.
- `junit_type`: a type outside the closed table. `junit_inventory`: duplicate, extra or wrong-classname testcase, or an expected test absent at exit 0.
- `internal`: any uncaught exception, caught at top level; prints the enum only, never a traceback.

## Released output: closed typed grammar

Every released line starts `ARXDIAG v1 ` and is **re-rendered from parsed, typed values**, never echoed. Integers are ASCII decimal without leading zeros, range-checked (`u8`, `u16`, `u32`, `u64`); enums are closed sets; `-` means none.

```
sample event=<push|pull_request> outcome=<enum> nextest=<0|100|killed|none> wall_ms=<u32> list_ms=<u32> run_ms=<u32> cleanup_ms=<u32> orphans=<u8> e_s=<u16>
test id=<T1|T2|T3|T4|U6|R6c|R6s|R6p> kind=<pass|fail|abort|timeout|leak|exec_fail|not_run> ms=<u32>
diag id=<ID> <one D1 form below>
trunc id=<ID> dropped_lines=<u16> dropped_bytes=<u32>    reject id=<ID> n=<u16>    guard reason=<enum>
```

**D1 forms** (whole stored line after an exact `[DIAG-3n] ` prefix, anchored at both ends). `B` = `u32:u32:u32:u32:u32^u32`; `S` = `u32,u64,<L|F|C|l|S|?>,o,o,o,o,o` with `o` = `u64|-`; `cand` ∈ `None|Cpu|Store|Both`.
- `pass t= lag=@ over= ticks=` (u32), then for k = 1..3 ` nk` + (` term= ldr= seen=` u32 | ` ring=busy`) + ` high=`u32, then ` max Vote=,Append=,Apply=,Snapshot=,Other=` (u32) ` queue=`u32 ` leader=u32:u64:u32 write=u32:u32:u32 cand=`.
- `v1 fail t= lag=@ over= ticks=` (u32) ` cand=`; `lagev busy` | `lagev [` 0–8 × `(u32, u32)` joined by `, ` `]`.
- `helpers leader=u32:u64:u32 write=u32:u32:u32 converge=u32 stop=u32`.
- `attr busy` | `attr none` | `attr helper=<Leader|Write|Converge|ConvergeWait|Stop> fail=<Elapsed|NotLeader|NoQuorum|Unavailable|Busy|Stopped|Unknown|Orphaned|Wait> node=u8 t=u32 polls=u32 retries=u32`.
- `s3 before=S after=S class=<Churn|SameTermCandidate>`; `at n1=<S|-> n2=<S|-> n3=<S|->`.
- `report ShutdownReport { first: <Lock|Engine|Owners|StoreRelease>, engine: <Absent|Joined|JoinFailed|Deadline>, engine_ms: u16, owners_done_at_start: u8, owners_waited: u8, owners_left: u8, owners_panicked: u8, last_owner_join_ms: u16, propose_held: u8, read_held: u8, log_strong: u8, sm_strong: u8, total_ms: u16 }`.
- `n<1-3> ops Vote=B Append=B Apply=B Snapshot=B Other=B queue=B delay=u32` + (` seen= ovw= term= ldr= st=` u32 | ` ring=busy`); `n<1-3> ring` + (0–128 × ` S` | ` busy`); `end bytes=<u16 ≤ 8192> trunc=<0|1>`.

**Rules.** `kind` comes from the static type table; `not_run` only at exit 100. A line failing its form, or any field failing its type or range, is dropped privately and counted in `reject`. Indented or mid-line copies are ignored. Per test ≤ 16 `diag` lines, each ≤ 8192 bytes, and ≤ 8448 bytes in total (D1's 8 KiB limit plus prefixes); overflow is counted in `trunc`. Whole step ≤ 150 lines and 72 KiB. Never released: raw output, panics, paths, `list` JSON, failure exit codes, `GITHUB_*` values other than the event.

## Files and tests (exactly these files)

1. `.github/workflows/ci.yml`: the two steps.
2. `tools/ci/windows-diag-sample.nextest.toml`.
3. `tools/ci/windows_diag_sample.py`: stdlib only; native calls behind one `JobApi`/`Spawner` seam; budget, inventory, JUnit, grammar and outcome logic are pure functions.
4. `tools/ci/test_windows_diag_sample.py`: pure unittest with mocks and fixtures. **No real process, ctypes call or cargo.** Covers:
   - budget edges E = 600 s / 601 s and every `anchor` case;
   - inventory with real `nextest-metadata` field names, rejecting 7 names, 9 names, a control test, another binary and `ignored:false`;
   - JUnit with strings copied from `junit.rs`: every `kind`, an unknown type, a duplicate, `not_run` at exit 0 vs 100, oversize and malformed after exit 100 (both `diag_unparsed`), and missing at exit 100 (`junit_missing`);
   - external-deadline kill with a missing and with a truncated file (`diag_incomplete`, file not parsed, exit 0);
   - grammar: one valid fixture per D1 form, re-rendered byte-identical; rejects for a leading zero, `u8` overflow, an unknown enum, a ninth `lagev` pair, 129 ring samples, a path, non-ASCII, an indented line, a 17th line; exact `trunc`/`reject` counts; a source pin that each D1 format literal above still occurs in `diagnostics.rs`;
   - fake job and spawner: assign failure and `IsProcessInJob` false each kill the owned child before returning and write no go byte; a cancel flag set between spawn and assign; terminate exactly once; `ActiveProcesses` polled; `cleanup` gating;
   - exit 0/1 per outcome; `internal` without traceback; a source scan finding no `taskkill`, `psutil` or `os.kill`.
5. `scripts/ci-workflow.test.mjs`: one new test pinning anchor-first-before-checkout and sample-last (Windows-only, exact one-line text with `&&`), the ordered list of the 25 existing step identifiers, and no `continue-on-error`. Existing tests still pin one `timeout-minutes` and no other `CARGO_`.

**Original sample implementation scope (superseded by the accepted whole-fixture contract):** `three_node.rs`, `diagnostics.rs`, `test_windows_cluster_quarantine.py`, Cargo files, other workflows were untouched. The whole-fixture exception pins all 24 ignores and the separate inventories; diagnostics, Cargo files and workflows remain untouched. The coordinator owns PROGRESS and any ADR 0024 note.

**Local evidence Opus must report** (dev host, current user, unique target under D:): the unittest file and `npm test`; one real wrapper run with `outcome`, phase times and `orphans=0`; one manual Job Object control where a Python grandchild sleeping 600 s is reaped with `ActiveProcesses=0`; one manual assign-failure control (assign to a closed job handle) showing the blocked child killed. Hosted ownership stays unverified until the PR's own run.

## Risks

- **Admission is not a guarantee**: see the residual above. **Selection bias**: skips favor faster runners; `e_s` is recorded.
- **Gate exposure**: `deadline_list` and `cleanup` can redden the job on hosted slowness or an unkillable process, not on test logic. Sol should confirm that trade.
- **Conflation**: `abort` covers crashes and global-timeout kills; `diag_incomplete` covers nextest and test stalls. A leaked-but-passing test reads as `pass`.
- **D1 drift**: a D1 format change turns lines into `reject` counts; the source pin fails the unittest on that PR.
- **What results mean**: a green sample is not re-enable evidence; a red one names a candidate seam and authorizes no fix (recovery plan §5).
