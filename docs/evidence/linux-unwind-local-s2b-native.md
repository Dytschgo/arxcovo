# S2b native verification preparation

Builder: OpenAI / gpt-6.1-sol / medium, implementation sub-agent, 2026-11-14.
Source/harness commit: `1e23ac02f75eb624225dc055a611145c69f4b393`.
Fresh Anthropic Opus 5.5 high source review is required before push/publication.
No review was commissioned in this task; no subagents were used.

Initial cwd and Git top level matched the supplied isolated worktree. Initial
HEAD `284b930992f2ce75c27d84a70d179c0c3aeb94f2` was clean. Only the five owned new
paths were changed. Existing CI, E3/E4 harnesses, production source and PROGRESS
were not edited. Read AGENTS, KICKOFF, proposal S2b U-a/U-b/U-c,
`m2-s2b-independent-security-review.md`, and the E3/E4 native output/label
review lessons. Reviewed S2b source `efe8e55a360575e0da22202c24bbc7049cce5c60`
is retained as `88195d5595105657c6dbc79ca7468ecd582da8a9`; source review
`d94b176` is retained as `e71fc59` per the assignment. The driver pins all six
reviewed source file bytes, plus the workflow and mutation manifest.

## Prepared behavior and limits

The separate non-required plain pull_request workflow is path filtered, uses
contents: read and persist-credentials: false, the same three pinned actions as
E4, Ubuntu, Rust 1.95.0 and nextest 0.9.151. Timeout is 10 minutes, incremental
builds 1 and dev debug 0. It installs only in a future hosted run. No workflow
has been run here.

The driver prints numeric UID/eUID and refuses either zero before executing
Cargo. It requires Linux, GitHub Actions, matching cwd, clean checkout/top level,
valid HEAD and tracked 100644 paths. Paths reject escape, symlink, reparse point,
hardlink and nonregular targets. Reads cap source files at 256 KiB, manifest and
workflow at 32 KiB; process output caps at 2 MiB (Git inventory 8 KiB and HEAD
128 bytes). No raw process output is emitted. Every process has a time cap and
owned process-group cleanup; a global 480-second budget bounds native work.
Initial baseline cap 210 seconds, each mutant 85 seconds with 95 seconds reserved,
and final baseline 90 seconds. Hosted rebuild durations are unmeasured.

The bounded process/read/path primitives follow the existing E4 approach without
changing or importing the executable E4 harness. S2b keeps only the single library
test selector and its two fixed mutations. Before and after baselines request
explicit success output and require exactly one named arxd PASS with no SKIP or
error. Kills require exactly one named FAIL, exit 100, and the single exact footer
`error: test run failed`; other statuses/errors, compilation failures, skips and
timeouts never earn kill credit. Optional padded nextest counters are accepted.
Restoration runs in finally, checks original bytes and the six-source fingerprint,
and requires the final native baseline before success.

`drop_skips_store` removes only the Group Drop Release store, retaining cleanup.
`group_forgotten` forgets the local `_group` in the owned fake Runner frame before
the fixed panic. It uses a valid local binding, rather than the misplaced unix.rs
forget identified by review. This is decomposition-only evidence, **not real
Runner ownership proof**. It leaks only the temporary test Group descriptors in
the nextest child, closed at process exit. The fixed Rust fixture owns a small
nonempty temporary leaf; tempfile removes those ordinary files on unwind after
the assertion. No harness recursive filesystem cleanup is introduced. No trust,
root, NNP fence or production real run_core path is altered. Both mutations have
exactly one in-memory match. Their native compilation and assertion behavior
remain UNRUN; compilability is a source-level expectation, not a claimed result.

## Checks actually run locally

- `python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py`:
  final 11 tests passed. Includes in-memory pinned mutation applicability,
  real nextest 0.9.151 output-format samples (arxd label and padded counters),
  fail-closed categories, path/reparse/hardlink/read bounds, Python-only process
  timeout/output bounds, root/eUID guards and synthetic whole-run restoration
  after kill, survival, compilation failure and timeout.
- Same isolated Python command for `tools/ci/e3-native`: 25 tests, OK, one
  Windows symlink skip. For `tools/ci/e4-native`: 22 tests, OK.
- `git diff --check` and staged diff check: passed before source commit.

No native execution, Cargo command, native fixture, install, GitHub action,
push, PR, hosted CI, owner/admin or private-state action occurred. No failed fix
attempts. Native baseline and both native kills remain **UNRUN** until hosted
execution after fresh source review. M2 incomplete; real-Runner PA6 **OPEN**;
A13 not accepted; hardware paths **unverified on real hardware**. Production
remains inactive. No old Lab Manager state or protected VM was touched.

## Exact SHA-256 manifest at source/harness commit

- `.github/workflows/s2b-native-evidence.yml`: `40fb88d0ec96c28f4d4aa10d5f0cedc3fce778d913c0f24394ec255f28db346f`
- `tools/ci/s2b-native/driver.py`: `c64b52867e11e093a25206de8aec2855c188a4f73a42e57461b01296a33ab47c`
- `tools/ci/s2b-native/test_driver.py`: `71ca9f9f11af31e703ef8ba2e9dc82d3da4cf1783de379cab178d3f387a74271`
- `tools/ci/s2b-native/mutations.jsonl`: `28f05948ca4aace6944fbe8c77676e8f702575042cf77d9c9c176096ac5b0f94`
- `crates/arxd/src/exec/cgroup.rs`: `64d4cb8c48e516d5fdcd92f85fe2c9c9abb028ee55251929e95824c16211d121`
- `crates/arxd/src/exec/mod.rs`: `f8d425f7a57a309b7e10c29b4609f2c0c75002a33bc1eb0c00e04cd47dd3f9b4`
- `crates/arxd/src/service/executor.rs`: `45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed`
- `crates/arxd/src/service/unix_bridge/tests.rs`: `b2e0c0efc24feb46555fee12413edf79c8afe22d0221908fde59238a597a780d`
- `crates/arxd/src/service/unix_bridge/tests/e6.rs`: `f26d1ed0778c5b24495f46bde26b8248f046aedfbcae07440857453bf17dabb1`
- `crates/arxd/src/service/unix_bridge/tests/s2b.rs`: `6eb93098dfda74ae52e751a07f69fd9252417184d65d6064cb6a26270643e089`
