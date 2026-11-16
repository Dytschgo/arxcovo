# Evidence: M3 Windows hosted diagnostic sample (implementation, local only)

Status: **implemented locally, not published** (Anthropic / Claude Opus 5.5 / high, implementation sub-agent, 2026-11-16).
Built on `50087fe`, which sits on main `87c9940`. It implements the Sol-rechecked brief `docs/briefs/m3-windows-hosted-diagnostic-sample.md` and the final recheck clarifications in `docs/reviews/m3-hosted-sample-sol-contract-review.md`.
Nothing was pushed, dispatched or rerun, and no PR exists. Independent Sol review is required before publication.
This is not a product fix and not a re-enable. The eight Windows ignores, their IDs and their guard are unchanged. M3 stays unchecked, and Windows three-node is unverified on hosted Windows and **unverified on real hardware**. M2 stays open.

## Commits and files

| Commit | Subject |
| --- | --- |
| `c0ae73e` | feat(ci): add bounded non-gating Windows three-node diagnostic sampler |
| `8a59468` | ci: anchor and run the Windows diagnostic sample as the last step |
| `4deb2f0` | fix(ci): pass --no-tests to the diagnostic sample run only |
| `1b5fa0d` | test(ci): make the absolute 90 s budget bind the sample run deadline |
| (this file) | docs(evidence): record local sampler evidence and limits |

Blob manifest at `1b5fa0d` (the five changed files, then the files this work only reads):

```
0c7af38b33f6c127029d8312a0c9ecd92f6361da .github/workflows/ci.yml
86bce968275ed7ff2ddfc6b6a17d992d503a85ee scripts/ci-workflow.test.mjs
f5b9f589f04d4e89bdfb1e6d4e365c94bef72917 tools/ci/windows-diag-sample.nextest.toml
b7b8a06c3137bf2532f1324fcf6f261e3796e104 tools/ci/windows_diag_sample.py
68fc3ebfda38f4e85396a5e8a0502e496aae1b15 tools/ci/test_windows_diag_sample.py
bb376fccf99e786bfbe1c84e712cab892dd58d05 tools/ci/test_windows_cluster_quarantine.py   (unchanged)
6139626427dbc4449a8923a3b2e3459e727e876f crates/arxd/src/cluster/consensus/diagnostics.rs (unchanged)
4613201cab02f3551a5d41daafb3273c813e1c62 crates/arxd/src/cluster/consensus/three_node.rs  (unchanged)
```

`git diff --stat 50087fe 1b5fa0d` touches exactly those five files (2036 insertions, 0 deletions). No PROGRESS, ADR, Cargo, Rust, quarantine-guard, other-workflow or gate change.

## What the workflow change does

- **First step**, before checkout, Windows only, `pwsh`: appends `ARX_DIAG_T0=<unix seconds>` to `GITHUB_ENV`.
- **Last step**, after Electron smoke, Windows only, `pwsh`, one line: `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py' && python -I -B tools/ci/windows_diag_sample.py`.
- The new `ci-workflow.test.mjs` case freezes the 28 step identifiers in order (anchor, the 26 post-C1 steps, then the sample), the exact text of both new steps, a single `ARX_DIAG_T0`, and no `continue-on-error`. The existing cases still pin one `timeout-minutes: 12`, one job-wide `env:` block, only `CARGO_PROFILE_DEV_DEBUG`, and both Playwright install lines. The C1 Windows install line is unchanged.

## Deviations from the brief (routine choices, for review)

1. **`--no-tests fail` is passed only on `run`.** The first real local wrapper run reported `guard reason=list_exit` after 203 ms. Running the fixed list argv by hand on nextest 0.9.151 gave `error: unexpected argument '--no-tests' found` (exit 2). The inventory gate already refuses an empty or partial match. This was implementation correction 1 of the allowed 2; no second correction was needed.
2. **Closed `outcome` enum gains `guard` and `skipped_event`.** On a gating failure the sample line is still printed, with `outcome=guard`, followed by the `guard` line. `ci.yml` also triggers on `workflow_dispatch`; any event other than `push` or `pull_request` prints `event=- outcome=skipped_event` and exits 0 without spawning anything. The anchor is still validated first.
3. **JUnit type table** also maps `test passed but leaked handles` (an `error` element, from `non_success_kind_and_type` in nextest-runner 0.129.0) to `leak`. Every other entry comes from the brief. A type sitting under the wrong element (`failure` versus `error`) gates `junit_type`.
4. **Target directory check.** `rust-build-meta.target-directory` in the list JSON must equal `<repo>/target`. Otherwise the run gates `inventory`, which covers a `CARGO_TARGET_DIR` or config override that would move the JUnit path.
5. **One Job Object per phase.** `list` and `run` each get their own kill-on-close job, terminated exactly once and closed. In the sample line, `cleanup_ms` and `orphans` are summed over both phases; `orphans` saturates at 255.
6. **EXPECTED import.** `importlib.util.spec_from_file_location` creates the module, but the code executed is exactly the bytes the wrapper read (at most 64 KiB) from the fixed guard path. That path is checked first: ordinary file, parent directories not reparse points, `realpath == abspath`. A cached `.pyc` or ambient module can never be used. The eight names must equal the wrapper's fixed ID table, or the run gates `inventory`.
7. **Child exit codes.** No go byte or EOF exits 2, the watchdog 3, bad arguments 6, a cargo spawn failure 7, and an out-of-range code 8. Any of them reaches the parent as a `list_exit` or `nextest_exit` gate. The child re-validates its private-directory argument (direct child of `RUNNER_TEMP`, an `arxdiag-` name, a real directory) only after the go byte. If `RUNNER_TEMP` is missing or not absolute, the run gates `platform`.
8. **`dropped_bytes`** counts the bytes of the re-rendered lines that were not released.

## Local evidence (dev host, current user, isolated clone `D:\arx-hosted-sample-opus-50087fe-15385`, temp `...-tmp`)

All of these ran on this Windows 11 dev host with Python 3.12.10, cargo-nextest 0.9.151 and `CARGO_PROFILE_DEV_DEBUG=0`. The target directory was the clone's own `target`, prebuilt with `cargo nextest run --workspace --locked --no-run` (43 s, exit 0).

- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py'`: `Ran 49 tests ... OK`. These are pure fakes; the files they read are the sampler, the guard and the D1 source pins.
- `python -I -m unittest discover -s tools/ci -p 'test_windows_cluster_quarantine.py'`: `OK`, unchanged.
- `node --test scripts/*.test.mjs`: 29 pass, 0 fail, including the 4 `ci-workflow` cases. **`npm test` was not fully run**: its `ui` half needs `vitest`, and this worktree has no `node_modules` (`'vitest' is not recognized`). Installs were out of scope. No `ui` file changed.
- Manual list (fixed argv minus `--no-tests`): exit 0 in 0.7 s, 92,612 bytes of JSON. Exactly eight matches, all binary `arxd`, kind `lib`, status `listed`, `ignored: true`.
- **Real wrapper run**, the exact CI one-liner in `pwsh`, `GITHUB_EVENT_NAME=push`, a fresh anchor, on `1b5fa0d`:
  ```
  ARXDIAG v1 sample event=push outcome=all_pass nextest=0 wall_ms=9578 list_ms=703 run_ms=8843 cleanup_ms=0 orphans=0 e_s=15
  ```
  Step wall time was 9.9 s. An earlier run of the same code (before the test-only commit) released 8 `test` lines, all `kind=pass`, and 9 real D1 `pass` lines (U6 prints two), with no `reject` or `trunc` line. Example: `ARXDIAG v1 diag id=R6c pass t=5303 lag=14@351 over=0 ticks=86 n1 term=1 ldr=1 seen=81 high=1 ... max Vote=14,Append=9,Apply=5015,Snapshot=0,Other=0 queue=0 leader=66:45883:45818 write=1:62:62 cand=Store`. After both runs `RUNNER_TEMP` held only the prebuild log, and `junit.xml` had been removed.
- Real budget skip (anchor 586 s old): `outcome=skipped_budget ... e_s=601`, exit 0, nothing spawned. Missing anchor: `outcome=guard ... e_s=-` and `guard reason=anchor`, exit 1.
- **Manual Job Object controls** (scratch script, real `WinJobApi` and `run_phase`, ordinary current-user Python processes). The control child uses the same go-byte handshake and then starts a grandchild that rewrites a heartbeat file every 100 ms for up to 600 s:
  ```
  control=orphan status=exit rc=0 phase_ms=1610 cleanup_ms=0 orphans=1 actives=[1, 0] job_final(total,active,terminated)=(2, 0, 0) child_rc=0 heartbeat=stopped
  control=hang status=killed rc=None phase_ms=3000 cleanup_ms=109 orphans=2 actives=[2, 0] job_final(total,active,terminated)=(2, 0, 0) child_rc=1 heartbeat=stopped
  control=assign_closed guard=job child_rc=1 reaped_s=0.11 stdin_closed=True list_json_exists=False
  ```
  In `orphan`, the grandchild outlives its exiting parent, is counted, and is reaped by `TerminateJobObject`: `ActiveProcesses` reaches 0 and the heartbeat stops. In `hang`, the 3 s deadline kills both processes. In `assign_closed`, the real wrapper child (blocked on the go byte) is assigned to a closed job handle. Assignment fails, the wrapper kills its own `Popen` child within 0.11 s, no go byte is written, and cargo never starts. No PID walk or name lookup was used; the verification relies only on job accounting and the heartbeat file.
- **Mutation check** (24 hand mutants applied in place one at a time, then restored; `git status` clean afterwards): 23 killed. The one survivor, removing the explicit `isascii` check in `parse_d1`, is equivalent because every literal, digit set and enum in the grammar is ASCII. Killed mutants included: no reap on assign failure; skipping `IsProcessInJob`; skipping the cancel check before go; writing go on cancel; a double terminate; no `ActiveProcesses` wait; an unbounded unassigned reap; leading zeros; a widened `u8`; ring > 128; lagev > 8; an unknown enum; a 17th line; an unknown JUnit type; exit 101 treated as diagnostic; parsing after the external kill; malformed JUnit gating; `not_run` at exit 0; E = 601 admitted; a future anchor; `ignored:false`; `deadline_list` turned diagnostic; the unclipped run deadline. The last of these survived the first pass, so the test was strengthened in `1b5fa0d`.

## Limits (local versus hosted)

- The local host is not a hosted runner. The eight tests passing here at `-j 4` says nothing about hosted Windows under load, and a green sample is not re-enable evidence.
- Hosted Job Object ownership, the 15 s pre-anchor and 30 s post-step allowances, the real `e_s` and the step's hosted wall time are all **unverified until the PR's own run**. Read the job-API timings of that run to check the allowances. The admission rule is conservative and measured, not a guarantee: if the allowances are exceeded, the unchanged 720 s cap still cancels the job.
- Cancellation was exercised only with fakes (a flag set between spawn and assign, and during the wait). No real runner cancel or Ctrl-Break was sent.
- No `diag_fail`, `diag_unparsed` or `diag_incomplete` path ran against real nextest. Those paths are covered by fixtures built from nextest-runner 0.129.0 and quick-junit 0.8.0 strings and element names. The real `all_pass` JUnit shape matched the parser.
- Python formatting: the repository configures no Python formatter, so none was run. No Rust changed, so `cargo fmt` and `clippy` were not rerun.

## Branch note

`work/m3-windows-hosted-sample` is checked out in the coordinator's main checkout at `50087fe` and was not touched. These commits sit on the implementation worktree branch `t3/hosted-diagnostic-sample`, a fast-forward of `50087fe`.

## Sol review corrections (2026-11-16)

Independent Sol review `fc6add5` (retained `a38ae7a`) blocked publication on B1 (late launch/finalization) and B2 (suppressed file cleanup errors). Coordinator Sol corrected both without changing Rust, quarantine, test selection, workflow scheduling or any existing timeout value.

The common deadline now bounds launch admission, the go decision, process cleanup polling and finalization. Validation is checked against its 2 s allowance. Expired synchronous operations cannot be interrupted by Python; once they return, subsequent work and success reporting are refused. Mandatory owned-child termination and owned-file cleanup are still attempted, including after expiry. The closed integrity reason `deadline` is added; a test run killed at its ordinary phase deadline remains diagnostic `diag_incomplete`.

Final cleanup attempts each fixed owned path, rejects unexpected path kinds without deleting them, and reports `cleanup` if any removal fails. Raw JUnit retention can therefore no longer accompany `all_pass`. No recursive deletion is used.

Before source correction, four new regression methods failed on the old implementation: delayed validation, expired inventory, expired launch and failed final cleanup. After correction, 55 pure sampler tests pass, including assignment expiry and cleanup polling under the common deadline; 29 Node tests and the unchanged quarantine guard pass. `git diff --check` passes. Logs: `D:/Temp/arx-sampler-red.log`, `D:/Temp/arx-sampler-corrected.log`, `D:/Temp/arx-sampler-node.log`, `D:/Temp/arx-sampler-guard.log`. These are coordinator runs, separate from the original Opus evidence. No new native wrapper/Job Object controls or full npm/UI test were run. Fresh complementary Opus review is required for these Sol changes. Hosted behavior remains unverified.

## Opus recheck R1/R2 correction (2026-11-16)

Opus recheck `a76c223` retained `81a37bf` closed B2, but found pre-go expiry misclassified as a diagnostic kill and UTF-8 damage in PROGRESS. Coordinator Sol adds the internal `expired` phase status, mapped to integrity guard `deadline`; `killed` is now reserved for a test process that actually received go. Ordinary test run timeout remains non-gating.

A new regression covers delayed run job creation, spawn, assignment and stale-JUnit deletion, each ending before the overall 90 s deadline. All four subcases fail before the fix (exit 0) and pass afterward (exit 1, deadline, no go). A second test pins rejection and non-removal of an unexpected cleanup path type. Final pure suite: 57 pass, Node: 29 pass, quarantine guard: 1 pass; diff check passes. Logs: `D:/Temp/arx-sampler-r1-red.log`, `D:/Temp/arx-sampler-r1-green.log`, `D:/Temp/arx-sampler-r1-node.log`, `D:/Temp/arx-sampler-r1-guard.log`.

PROGRESS was rebuilt from the known-good UTF-8 bytes at `a38ae7a`, retaining exactly the intended Current work replacement. A character-for-character assertion excluding that line proves historical text matches the original. The earlier red log covers four test methods; two additional methods were added after that run, as the Opus recheck correctly notes. No new native or hosted evidence is claimed. Fresh review remains required.
