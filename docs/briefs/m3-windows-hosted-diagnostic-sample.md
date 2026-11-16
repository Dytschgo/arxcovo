# Brief: M3 Windows hosted diagnostic sample (one non-gating step)

Status: **proposed** (Anthropic / Claude Opus 5.5 / high, design sub-agent, 2026-11-16) on `5c680d6`, which sits on main `58d06c1` (PR #153 D1).
This brief is docs only. Nothing was built, run or dispatched. Sol must review it before any workflow change.
It authorizes no fix and no re-enable. The eight Windows ignores and their guard stay exactly as they are. M3 stays unchecked, and Windows three-node is unverified on hosted Windows and **unverified on real hardware**. M2 stays open: A7 is NO-GO, and A12 is consumed with no retry.

## Verified inputs

- **Windows job timing.** Job `133123407634` (run `43814521932`, PR153 head `53a7ff2`) took 598 s (06:06:27–06:16:25) against the 720 s cap. From one public-API read of its steps: clippy 87 s, `cargo nextest run --workspace --locked` 281 s, `npx playwright install --with-deps --only-shell chromium` **151 s** (06:13:36–06:16:07), Electron smoke 10 s, post steps plus Complete job 4 s. The last regular step ends at about 594 s elapsed. The 708 s figure for the last main Windows run came from the coordinator; I did not re-read it.
- **nextest.** Local `cargo-nextest 0.9.151` matches the CI pin. Checked against `--help` and its source in `nextest-runner 0.129.0` and `nextest-metadata 0.15.0`:
  - Flags: `--config-file`, `-P`, `--run-ignored only`, `-E`, `--user-config-file none`, `--cargo-quiet`, `--no-tests fail`, `list -T json`.
  - Profile keys: `global-timeout`, `slow-timeout{period,terminate-after,grace-period}`, `retries`, `fail-fast`, `test-threads`, `junit.{path,store-success-output,store-failure-output}`. JUnit is written under `target/nextest/<profile>/`.
  - Exit codes: 0 OK; 100 TEST_RUN_FAILED, which includes runs cancelled during tests; 4, 94, 96, 101, 102, 104 for no tests, filterset, setup, build, metadata and list failures.
  - On Windows, nextest puts each test process in its own job object and terminates that job on timeout.
  - JUnit failure `type` values: `test failure with exit code N`, `test abort`, `test timeout`, `execution failure`, plus the `(leaked handles)` and `passed but leaked handles` variants.
- **Workflow constraints.** `scripts/ci-workflow.test.mjs` pins one `timeout-minutes: 12`, one job-wide `env:` block, no other `CARGO_*` token, and the exact playwright line. `tools/ci/test_windows_cluster_quarantine.py` pins exactly the eight ignores.
- **D1 output.** A passing test prints one `[DIAG-3n] pass …` line through `eprintln!`. A failing test writes a dump of at most 8192 bytes to stderr from a drop guard. Other test output (panics, `RPC counters`, `R6 …` lines) can contain workspace or temp paths and must never be released.

## Budget decision: a ci-speed PR comes first

A fixed +90 s step is **not robust**: 598 + 90 = 688 s leaves 32 s, and 708 + 90 = 798 s exceeds 720 s, which turns the required job red. Raising the timeout or bypassing it is forbidden. Two rules follow:
1. The sample runs as the **last step, behind an adaptive gate**, so it can never push the job past the cap.
2. A ci-speed PR, **C1, lands first**. Without it the gate skips nearly every run: on the reference job E ≈ 606 s at the sample, above the 600 s limit.

**C1 (Sol builds, Opus reviews, its own concern).**
- **Measured cost:** 151 s on Windows.
- **Mechanism, read from source:** `playwright-core 1.65.0` `installDependenciesWindows` runs `bin/install_media_pack.ps1` for chromium. That script runs `Install-WindowsFeature Server-Media-Foundation` when `ProductType == 3`, and the runner is a Server SKU. How the 151 s splits between this and the download is unmeasured.
- **Change:** keep the existing line under `if: runner.os != 'Windows'`. Add a Windows-only step running `npx playwright install --only-shell chromium`. Update `ci-workflow.test.mjs` to pin both lines.
- **Acceptance:** Windows smoke and Electron smoke stay green; Linux and macOS are unchanged; the PR shows its own step-log split; its PR run and the next main run each show a Windows job reduction of **at least 100 s**.
- **Stop rule:** under 60 s saved, C1 is abandoned and the sample waits for a coordinator decision; between 60 and 100 s, the coordinator decides.
- **Out of scope:** caching, any other step, any broader backlog.

## Sample contract (Opus implements after C1 merges and Sol reviews this brief)

**Selection.** All eight tests at `-j 4` (nextest's default contention on the 4-vCPU runner), `retries 0`. No rotation, `-j 1` or test groups: removing inter-test CPU contention would mask a candidate upstream cause (S1/S2). Honest limit: eight tests alone are still less loaded than the original full-workspace run, so a green sample proves nothing. One sample per run: the PR push, then each main CI. No manual reruns or dispatches.

**Workflow: two added steps.** Every existing step, the cap, the env block, the quarantine guard and the five gates stay unchanged. No `continue-on-error`, step `timeout-minutes`, `env:` or `CARGO_` token.
- **First step**, `Anchor Windows diagnostic sample budget` (`if: runner.os == 'Windows'`, `shell: pwsh`): appends `ARX_DIAG_T0=<unix seconds>` to `$env:GITHUB_ENV`.
- **Last step**, after Electron smoke, `Windows three-node diagnostic sample` (Windows only): runs `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py'`, then `python -I -B tools/ci/windows_diag_sample.py`.

**Budget gate (pure function).** E = now − T0 + 15 s (allowance before the anchor). Run only if E + 90 s (step ceiling) + 30 s (post steps) ≤ 720 s, i.e. E ≤ 600 s. Otherwise print `outcome=skipped_budget` and exit 0; a skip is not a sample.

**Phase bounds.**
- Start and validation ≤ 2 s; `list` ≤ 20 s.
- `run`: 60 s external deadline; nextest `global-timeout` 50 s; per test `slow-timeout = {period="24s", terminate-after=2, grace-period="2s"}`, i.e. 48 s + 2 s, above D1's internal 45 s so internal timeouts still dump.
- Kill, join and verify ≤ 8 s. **Whole step ≤ 90 s.**

**`tools/ci/windows-diag-sample.nextest.toml`, profile `diag-sample`:** `retries=0`, `fail-fast=false`, `test-threads=4`, `success-output`/`failure-output="never"`, `status-level`/`final-status-level="none"`, the timeouts above, and `[profile.diag-sample.junit]` with `path="junit.xml"` and both store-output keys `true`.

**Commands.** Same cargo arguments as the gating step, so the dev/debug-0 build is reused without a rebuild, and `--workspace` avoids `-p` feature drift:

```
cargo nextest {list -T json | run} --workspace --locked --cargo-quiet --user-config-file none \
  --config-file tools/ci/windows-diag-sample.nextest.toml -P diag-sample --run-ignored only --no-tests fail \
  -E 'package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/)'
```

**Inventory (gates before `run`).** In the `list` JSON, exactly one suite (binary-id `arxd`, kind `lib`) has matches. Its matched names equal `cluster::consensus::three_node::` + each `EXPECTED` name, imported from `test_windows_cluster_quarantine.py` (single source). All eight have `ignored: true`, and nothing else matches. This excludes the three `diagnostics::controls` tests and every M2 fixture.

## Ownership and cancellation (Windows Job Object, stdlib `ctypes` only)

- **Job.** Unnamed, **non-inheritable**, `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, *no* `BREAKAWAY_OK`. Cargo's job and nextest's per-test jobs nest inside it.
- **Race-free launch.** Per phase, start `python -I -B windows_diag_sample.py --child <list|run> <privdir>` with stdin as a pipe and stdout/stderr as `DEVNULL`. The child blocks reading one byte. The parent calls `AssignProcessToJobObject`, then writes the byte. Only then does the child build argv from module constants (never from input) and launch nextest. `list` stdout goes to `privdir/list.json`; every other descendant stream is `DEVNULL`, so nothing inherits the step's console handles or writes raw output to the log.
- **Deadline or `SIGINT`/`SIGBREAK` (GitHub cancel):** `TerminateJobObject` once; wait ≤ 5 s for the child; poll `JobObjectBasicAccountingInformation.ActiveProcesses` every 100 ms for ≤ 3 s until 0; close the handle. A normal exit runs the same terminate-and-verify and records `orphans=<n>`, the count alive before the terminate. If the wrapper is hard-killed, `KILL_ON_JOB_CLOSE` reaps the tree.
- **Forbidden:** `taskkill` (by image or `/T`), `os.kill` on non-owned PIDs, psutil, any name- or PID-walk termination. Only processes in the wrapper's job are touched.
- **Paths and reads.** `privdir` = `mkdtemp` under `RUNNER_TEMP`. Before any read or write, refuse a reparse point on `target`, `target/nextest`, `target/nextest/diag-sample`, `junit.xml` or `privdir` (`lstat` + `FILE_ATTRIBUTE_REPARSE_POINT`). Delete a stale `junit.xml` before `run`; remove it and `privdir` afterwards. `list.json` and `junit.xml` reads are capped at 4 MiB each.

## Outcome split: gate vs diagnostic

**Diagnostic (exit 0, closed result, no product credit):** anything the eight tests can do — fail, hang, crash, leak, print paths, print too much. Outcomes: `all_pass`; `diag_fail` (at least one test not `pass`); `diag_unparsed` (JUnit over the cap, no test lines released); `skipped_budget`.

**Gating (exit 1, `ARXDIAG v1 guard reason=<enum>`):** a break in the wrapper's own contract.
- `anchor`: T0 missing, not an integer, in the future, or older than 720 s. `platform`: not Windows. `reparse`: reparse point on a checked path.
- `job`: job setup or assign failed. `spawn`: child failed to start.
- `list_exit`, `list_parse`, `inventory`, `deadline_list`.
- `deadline_run`: nextest broke its own bounds, or a rebuild happened. `nextest_exit`: `run` exit not in {0, 100}.
- `junit_missing`, `junit_parse`, `junit_type` (type outside the closed table), `junit_inventory` (duplicate, extra or wrong-classname case, or an expected test missing with exit 0).
- `cleanup`: `ActiveProcesses` never reached 0. `internal`: any uncaught exception, caught at top level; prints the enum only, never a traceback.

## Released output: closed grammar, nothing else reaches the log

Every line starts with `ARXDIAG v1 `.

```
sample event=<push|pull_request> outcome=<enum> wall_ms=<u32> list_ms=<u32> run_ms=<u32> cleanup_ms=<u32> orphans=<u8> e_s=<u16>
test id=<T1|T2|T3|T4|U6|R6c|R6s|R6p> kind=<pass|fail|abort|timeout|leak|exec_fail|not_run> ms=<u32>
diag id=<ID> <suffix of a stored line that began exactly with "[DIAG-3n] ">
trunc id=<ID> dropped_lines=<u16> dropped_bytes=<u32>    reject id=<ID> n=<u16>
guard reason=<enum>
```

- **Mappings.** `kind` comes from the static JUnit type table; `not_run` is valid only with exit 100. IDs are a static table: T1–T4 for the `t*` tests, U6 for `u6`, R6c/R6s/R6p for the cancelled, store-only and panic R6 tests.
- **`diag` filter.** Released only if ASCII, ≤ 3072 bytes, chars limited to `[A-Za-z0-9 _=:,@.()\[\]{}-]` (no `\`, `/`, quotes or `%`), and first token in `pass|v1|lagev|helpers|attr|s3|report|at|end|n[1-3]`. Other lines count as `reject`. Indented and mid-line copies are ignored.
- **Caps.** Per test ≤ 16 `diag` lines and 8448 bytes, overflow reported as `trunc`. Whole step ≤ ~150 lines / 72 KiB.
- **Never released:** raw stdout/stderr, panics, paths, the `list` JSON, `GITHUB_*` values other than the event.

## Files and tests (exactly these files)

1. `.github/workflows/ci.yml`: the two steps.
2. `tools/ci/windows-diag-sample.nextest.toml`.
3. `tools/ci/windows_diag_sample.py`: stdlib only. Native calls sit behind one small `JobApi`/`Spawner` seam; budget, inventory, JUnit, grammar and outcome logic are pure functions.
4. `tools/ci/test_windows_diag_sample.py`: pure unittest with mocks and fixtures. **No real process, ctypes call or cargo.** Covers:
   - budget edges E = 600 s / 601 s and every `anchor` case;
   - an inventory fixture with real `nextest-metadata` field names, rejecting 7 names, 9 names, a control test, a different binary and `ignored:false`;
   - a JUnit fixture with attribute and type strings copied from `nextest-runner` `junit.rs` (not guessed): every `kind`, an unknown type, a duplicate, `not_run` at exit 0 vs 100, the size cap;
   - redaction: `C:\…` and `/tmp` paths, non-ASCII, quotes, an indented DIAG line, a 17th line, a 3073-byte line, a panic with a path — none released, exact `trunc`/`reject` counts;
   - fake-job deadline and signal paths: terminate exactly once, `ActiveProcesses` polled, `cleanup` gating when it stays above 0, go byte only after assign;
   - exit 0/1 per outcome; `internal` with no traceback; a source scan finding no `taskkill`, `psutil` or `os.kill`.
5. `scripts/ci-workflow.test.mjs`: one new test pinning anchor-first and sample-last (Windows-only, exact text), the frozen ordered list of the 25 existing step identifiers, and no `continue-on-error`. The existing tests still pin one `timeout-minutes` and no other `CARGO_`.

**Untouched:** `three_node.rs`, `diagnostics.rs`, `test_windows_cluster_quarantine.py`, Cargo files, other workflows. The coordinator owns PROGRESS and decides whether ADR 0024 gets a one-line note.

**Local evidence Opus must report** (actual runs on the dev host as the current user, unique target under D:): the unittest file and `npm test`; one real wrapper run with its `outcome`, phase times and `orphans=0`; one manual Job Object control where a Python grandchild sleeping 600 s is reaped and `ActiveProcesses=0` (local only, not CI). Hosted ownership behavior stays unverified until the PR's own run.

## Risks

- **Selection bias:** the adaptive skip favors faster runners; record `e_s` on every sample.
- **Unmeasured overhead:** whether nextest skips listing other binaries under a binary-level filter is unconfirmed. `list_ms` measures it; an overrun gates via `deadline_list`.
- **Gating failures:** a guard can turn the required job red. That is intended for wrapper or tool drift; Sol must check that every test-produced behavior lands on the diagnostic side.
- **Partial runs:** a global-timeout cut-off records `not_run`; how JUnit represents terminated tests must be pinned from source.
- **C1 may fall short:** then the sample waits and nothing is widened.
- **What results mean:** a green sample is not re-enable evidence. A red one names a candidate seam only and authorizes no fix; fix attempts stay under recovery plan §5, after coordinator review.
