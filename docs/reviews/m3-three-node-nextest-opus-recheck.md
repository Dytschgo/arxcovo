# Opus recheck: three-node nextest repair, B1 only

Provider/effort: Anthropic / Claude Opus 5.5 / high. This is a fresh, narrow, independent recheck of Sol's B1 correction. The prior Opus review `862fbcaa` is retained as `32dde4b`.

Target: head `15621369ebf8f824e543283ff33a8b92e1f8e72f` on branch `work/fix-three-node-nextest-contention`, base exact main `01a42d5adfedbdbd3d6e6d35648b8adb0d76cad9`. Correction commit `f63536ee2869e81b30f01e9eae433fc1f6925719`. Evidence commit `1562136` changes only `docs/PROGRESS.md` and `docs/evidence/m3-three-node-nextest-repair.md`.

**Verdict: PASS. B1 is resolved.** Standards PASS, Spec PASS, Security PASS. This clears the source recheck only. It is not publication, an exact-head review, the five hosted gates or green main.

## Scope (counted independently)

- `git diff 01a42d5 1562136` touches 9 paths. With this review file it touches 10.
- `git diff fb92e55 1562136` touches 4 paths: `.github/workflows/ci.yml`, `docs/PROGRESS.md`, the evidence file and the retained review `32dde4b`.
- `f63536e` changes only `.github/workflows/ci.yml`. It replaces the two-line `run: |` block in the existing `Pure Unix experiment guards` step with one `&&` chain: 1 insertion, 3 deletions. No other workflow line, step, job, matrix entry, timeout or permission changed.
- `git diff --quiet fb92e55 1562136 -- .config tools Cargo.toml Cargo.lock crates` reports no differences. `.config/nextest.toml` still has exactly one group with `max-threads = 1`, and its filter still covers only the eight `three_node` tests. Everything else keeps default scheduling. There is no Rust, dependency, deadline, retry, skip or quarantine change.
- `git diff --check 01a42d5 1562136`: pass.

## Standards: PASS

B1 is resolved. With `&&`, a failing first guard stops the chain under both of the shells the job uses.

What I ran myself: local Windows, pwsh 7.6.9 with `$PSNativeCommandUseErrorActionPreference = False`, GNU bash 5.3.15 and Python 3.12.10. These local runs are not hosted. A scratch harness rebuilt GitHub's step wrappers:

- pwsh: the script starts with `$ErrorActionPreference = 'stop'`, then the step body, then `if ((Test-Path -LiteralPath variable:\LASTEXITCODE)) { exit $LASTEXITCODE }`. It is invoked as `pwsh -command ". '<file>'"`.
- bash: `bash --noprofile --norc -eo pipefail <file>`.

Each simulated command writes a marker file before it exits, so the harness can see whether the second command ran.

| Shell | Case | Exit | Second command ran | Result |
|---|---|---|---|---|
| pwsh | `&&`, first exits 7 | 1 | no | PASS |
| pwsh | `&&`, second exits 5 | 1 | yes | PASS |
| pwsh | `&&`, both exit 0 | 0 | yes | PASS |
| pwsh | old multiline, first exits 3 | **0** | yes | control: reproduces B1 masking |
| bash | `&&`, first exits 7 | 7 | no | PASS |
| bash | `&&`, second exits 5 | 5 | yes | PASS |
| bash | `&&`, both exit 0 | 0 | yes | PASS |
| bash | old multiline, first exits 3 | 3 | no | control: `-e` already stopped |

- The real chain is read word for word from `ci.yml` at the target head. Run from the repository root, it exits 0 under both wrappers: 196 existing `tools/unix-exec-validation` tests and 2 `test_nextest_groups` tests, each time.
- About the exit code: when a chain fails, the pwsh wrapper reports 1 even with the trailing `exit $LASTEXITCODE`, while bash keeps the real code. The coordinator's evidence says pwsh "normalizes the chain failure to 1", and that matches what I saw. The coordinator was right to correct the assertion from exactly 7 to nonzero, and the evidence describes that correction openly. CI only needs a nonzero exit, and it gets one.
- No other Windows step can hide a failure the same way. Every other multiline `run: |` block in `ci.yml` is limited to Linux or macOS by its `if:` condition, so it runs under `bash -e`. The only Windows-specific step is a single pwsh command.
- Quoting is safe in both shells. `'test_*.py'` is a literal string in pwsh and bash alike, and Windows pwsh does not expand globs for native commands.
- N1 is still advisory and does not block: `validate()` uses bare `assert`. CI runs with `-I` and without `-O`, and the rejection test fails closed. No extra churn is requested.
- N2 is still advisory: the `tomllib` / Python ≥ 3.11 requirement will be confirmed by the hosted run, and an older interpreter would fail closed.
- Conventional commits: yes. One concern per commit: yes.

## Spec: PASS (reused, inputs unchanged)

The design inputs (Sol `1ee25f0`, Opus disposition `d00a94f`) and the scheduling config are byte-identical to the state that review `32dde4b` checked. So I reuse these results from that review without running them again:

- `show-config test-groups` maps exactly eight tests (R6 ×3, T1, T2, T3, T4, U6) into `arxd-three-node-cluster` with max threads 1. All other tests are in `@global`.
- The consensus subset gave 68 passed, 0 failed, 462 skipped in 27.159 s at `-j4`. The coordinator's run gave 68/68 in 27.459 s.

I did not run fresh Rust or nextest commands for this recheck. The correction stays inside the approved scope: the existing step still runs one added command. It changes how failures are reported, not how tests are scheduled, so it is **not** a fourth suite-level fix attempt.

## Security: PASS

There are no secrets, network access, credentials or production code. `permissions: contents: read` is unchanged. The guard parses a fixed repository path as TOML data under `python -I`, and nothing in it is evaluated. Joining the two commands with `&&` adds no shell-injection surface, because both commands are fixed literals.

## Gates still binding

- Publication may be prepared only now that this review passes. Before any merge, the following must happen at the exact head:
  - a fresh different-provider review;
  - green results on all five required hosted gates;
  - the Windows `verify` job finishing within the unchanged 12-minute cap, with hosted Windows nextest wall time and job duration reported.
- If the job times out, the work goes back to design. The cap is not raised, and `ci-time-budget` is not bypassed.
- This is suite fix attempt 3. Any hosted cluster deadline or Orphaned failure stops further changes and escalates to the owner. There will be no fourth tweak.
- Main stays red on `43788629298` at exact `01a42d5`. Nothing merges until exact-main CI is green or the owner gives explicit steering. A green repair PR is not a green main. No manual reruns to fish for a green result.
- The owner's three-attempt oversight applies to the suite as a whole and is recorded in PROGRESS. PR149 stays held and untouched. M2 remains open. Hardware paths remain **unverified on real hardware**.
