# Opus review: three-node nextest contention repair (implementation)

Provider/effort: Anthropic / Claude Opus 5.5 / high. Independent review of the Sol implementation; it is not independent of the Opus diagnosis, which this review only checks for honest reuse.

Target: head `fb92e55ed072da1aa2ed8f7f9ead656086881347` on base exact main `01a42d5adfedbdbd3d6e6d35648b8adb0d76cad9`; source/config commit `0ee4d3f`, evidence commit `fb92e55`, design inputs Opus `de6a8f1` retained `d00a94f` and Sol `1ee25f0`. `git diff 01a42d5...fb92e55` touches exactly eight paths. Commit `0ee4d3f` touches exactly the five expected paths. No Rust, Cargo, lockfile, dependency, assertion, test deadline, CI timeout, retry, skip or quarantine change.

**Verdict: CHANGES REQUESTED (one blocking finding, B1).** Spec PASS. Security PASS. Standards is blocked by B1 only. A fresh source PASS is needed after B1 is fixed. Publication, exact-head review, the five hosted gates and green exact-main CI are all still ahead.

## Standards

**B1 (blocking): the Windows guard step hides an earlier failure.** `.github/workflows/ci.yml` changes the existing `Pure Unix experiment guards` step into a two-line `run: |` block. Windows runners use pwsh by default, and GitHub's pwsh wrapper sets `$ErrorActionPreference = 'stop'`. That setting does not stop on a native command's non-zero exit code. The wrapper then ends with `exit $LASTEXITCODE`, which holds only the last command's exit code. So if the existing 196-test `tools/unix-exec-validation` discover fails on Windows and the new nextest-groups test passes, the step goes green. Linux and macOS use `bash -e` and still stop at the first failure. This is the first multi-command step in this workflow that runs on Windows.

- Reproduction (actual, local, not hosted): pwsh 7.6.9 with `$PSNativeCommandUseErrorActionPreference = False`, invoked as GitHub does (`pwsh -command ". '<script>'"`). The script ran the stop preference, a Python command exiting 3, a Python command exiting 0, then the wrapper's exit line. Exit code 0. The same script with the two commands joined by `&&` exited non-zero, and the second command did not run.
- Fix within the approved scope (still one added command in the existing step): `run: python -I -m unittest discover -s tools/unix-exec-validation -p 'test_*.py' && python -I -m unittest discover -s tools/ci -p 'test_nextest_groups.py'`. pwsh 7 and bash both stop at the first failure with `&&`. A separate named step would also work. This fix changes how CI reports failures. It does not change test scheduling, so it is **not** a fourth suite-level cluster fix attempt.

N1 (non-blocking): `validate()` uses bare `assert`. Under `python -O` the asserts are stripped, and I confirmed that `validate({'anything': 1})` then accepts the config. CI uses `-I` without `-O`, and `-I` also ignores `PYTHONOPTIMIZE`. The rejection test would still fail under `-O`, so the suite fails closed. Explicit `raise` statements would be sturdier. Optional.

N2 (non-blocking, needs hosted evidence): `tomllib` needs Python 3.11 or newer. The step calls the runner's `python` without `setup-python`. An older interpreter would fail closed with `ImportError`, so this is not a bypass risk. The hosted gates will confirm it.

Smell baseline: nothing to report. The guard is one exact-equality schema with no speculative hooks. The mutation lists in the test are short and readable.

Conventional commits: yes. One concern per commit: yes. The ADR records the scheduling deviation. PROGRESS records it as fix attempt 3 with its stop rule. Labels for hardware not exercised are kept.

## Spec

Against the Sol design approval `1ee25f0` and the Opus disposition `d00a94f`, all PASS:

- There is one group, `arxd-three-node-cluster`, with `max-threads = 1`. Its filter is `package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/)`. The regex is anchored and ends in `::`, so it cannot match a sibling module. `three_node` is declared `#[cfg(test)]` at `consensus.rs:501`. It contains exactly eight `#[tokio::test]` functions: R6 ×3, T1, T2, T3, T4 and U6.
- All other tests keep the default scheduling. The default profile is not given `test-threads`, so nothing is fixed at 4 threads. CI's `cargo nextest run --workspace --locked` is unchanged and finds `.config/nextest.toml` without a `--profile` flag.
- The guard compares the full config for exact equality: the top-level keys, the group map and the whole `profile` map. That rejects a broader filter, a `@global` group, retries, `slow-timeout`, `threads-required`, `default-filter`, extra overrides, extra profiles, setup scripts and `nextest-version` drift. The mutation test exercises the listed cases.
- Correction to the disposition carried forward: its text says "default four-way parallelism". The implementation correctly keeps the machine default, as Sol required.

Actual evidence from this review, using unique target `D:/Temp/arxcovo-opus-nextest-review-fb92e55` with TEMP/TMP on D:, Rust 1.95.0, cargo-nextest 0.9.151, `CARGO_PROFILE_DEV_DEBUG=0`, `--locked --offline`, and no installs:

- `cargo nextest show-config test-groups -p arxd --lib --show-default`: exit 0. Exactly the eight tests above are in `arxd-three-node-cluster (max threads = 1)`. Everything else is in `@global`. This run covered arxd lib only; the coordinator's workspace-wide mapping is reused, not repeated.
- `cargo nextest run -p arxd --lib -j4 -E 'test(/^cluster::consensus::/)'`: 68 passed, 0 failed, 462 skipped, 27.159 s. The grouped tests finished one after another as tests 61 to 68: T2 7.775 s, T3 1.663 s, T4 2.476 s, U6 0.884 s, R6 5.218/0.210/5.193 s, T1 0.476 s. This matches the coordinator's 68/68 in 27.459 s. Uninstrumented; this is not a hosted-latency reproduction.
- `python -I -m unittest discover -s tools/ci -p test_nextest_groups.py`: 2 passed.

Reused, not rerun, and described honestly in the evidence: Opus's scratch-only shared-flush model reproduced the exact leader-deadline signature with four parallel fixtures at 16 ms. Serial runs at the same latency did not reproduce it. The T3/T4 write-helper link is inferred and was not reproduced on its own; the disposition's opening "four exact signals reproduced" overstates this, and Sol's review corrects it. The one-core CPU control rules out CPU starvation only under that control, not in general. Hosted cause is supported, not proven. U6 Orphaned under slow applies remains a separate residual risk. The 196 existing guards and fmt/diff results are the coordinator's and were not rerun here. No Clippy claim is made because no Rust changed.

CI budget risk (unchanged rule, hosted measurement required): Windows is already near the 12-minute job cap. Green main jobs ran 716 s (`43775693200`), 708 s (`43777637580`) and about 585 to 613 s on earlier runs. Serializing the group may lengthen the nextest critical path by an amount not yet measured. The repair PR must report the hosted Windows nextest wall time and job duration. If the job times out, return to design. Do not raise the cap or bypass `ci-time-budget`.

## Security

PASS. The change has no production code, credentials, network access, secrets or private data. The workflow keeps `permissions: contents: read`. The guard reads a fixed repository-relative path and parses it as data with `tomllib`. It does not evaluate or execute anything, and it runs with `python -I`.

## Gates still binding

This is suite fix attempt 3. A hosted cluster deadline or Orphaned failure stops further changes and goes to the owner, with no fourth tweak. Main stays red on `43788629298`. Nothing merges until exact-main CI is green or the owner gives explicit steering. A green repair PR is not a green main. No manual reruns. PR149 stays held and was not modified. M2 remains open, and hardware paths remain **unverified on real hardware**.
