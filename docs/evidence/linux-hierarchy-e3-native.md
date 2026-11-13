# E3 native evidence harness: local preparation only

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-13.
Harness, tests, mutation list and workflow commit: `486b149`.
No subagents. No push, hosted execution, CI rerun/watch, merge, settings,
owner administration or infrastructure changes performed.

The prepared checkout top-level matched the task workspace, outside canonical
`D:/Code/Arxcovo`. Initial HEAD was
`f32deee6a8a110ee3ca93a7ee533e9c27a654dfe`, with clean status; shared Git metadata
was `D:/Code/Arxcovo/.git`. Main/base remains `c56b6b5`. This addition belongs to
the E3 concern in existing PR61; it creates no new required check or PR.

Read AGENTS, KICKOFF, ADR 0020, the E3 brief, the adapter and native tests,
review `de2cc44`, and recheck `1de4a5b`. The
[exact-head source integration review](https://github.com/Dytschgo/arxcovo/pull/61#issuecomment-6862427094)
was read through the read-only API. Its source-only PASS retains the native
gates. Its observation of run `43489053601` establishes real-crate Ubuntu
Clippy and 318 nextest passes, but passing-test output hid L7/L8 skips and did
not establish numeric UID. That is prior review evidence, not a new run here.

## Prepared workflow and fences

`.github/workflows/e3-native-evidence.yml` is a separate non-required,
Ubuntu-only, plain `pull_request` workflow. Paths cover `hierarchy/**`, the
workflow, harness and this evidence file. Permissions are only `contents: read`;
checkout credentials are not persisted. Checkout, Rust 1.95.0 and
cargo-nextest 0.9.151 use the existing CI action pins. No new third-party action,
Cargo dependency, required-check or policy change was made. Existing CI is
untouched. Job timeout is ten minutes; no 15-minute budget exception is sought.
Actual hosted duration is unmeasured.

The workflow prints numeric UID and fails on root, runs portable harness unit
tests, then runs `python3 -I -B tools/ci/e3-native/driver.py`. The driver also
requires real/effective UID nonzero, Linux, GitHub Actions and matching cwd.
Umask is 077 to keep owned fixtures private and avoid group-write failures.
It does not install an account, change hardening, or make a skip pass.

Before and after mutations, the driver runs:

```text
cargo nextest run --locked -p arxd --lib -E 'test(exec::hierarchy::linux::tests::)' --success-output immediate --no-fail-fast
```

It appends deterministic reporter settings (`--color never`, immediate failure
output, pass status lines, no duplicate final statuses), `--retries 0` and
`--no-tests fail`. All ten native tests must have visible exact `PASS` status
lines. The parser checks root/unexpected skip markers and emits explicit
`L7`/`L8` JSON values `passed` or `skipped`: a nextest PASS plus the known
in-test skip marker is **skipped**. No skip earns native pass or kill credit.
The ordinary-UID guard is a prerequisite for interpreting marker absence.
Successful captured output is inspected in memory, never dumped as raw logs.

## Fixed mutations and expected discrimination

`tools/ci/e3-native/mutations.jsonl` is a committed fixed list, using the
`mutation`, `changes` and `assertions` conventions of
`docs/reviews/0017-local-mutations.jsonl`; it is configuration, **not run results**.
Its bytes and original source are SHA-256 pinned in the driver. Every old
fragment must occur exactly once, entirely before the native test module.
Only the fixed `crates/arxd/src/exec/hierarchy/linux.rs` path is writable.
Arbitrary paths, symlinks, reparse points and hardlinked target files are refused.

| Fixed label | Native named test (prefix `exec::hierarchy::linux::tests::`) | Source expectation only |
| --- | --- | --- |
| `L1_take_raised` | `l1_read_limit_leaves_excess_pipe_bytes_unread` | `max + 2` consumes one excess byte |
| `L1_take_removed` | same pipe test | closed writer permits EOF; unread count differs |
| `L2_nofollow_removed` | `l2_nofollow_and_nonblocking_fifo` | file flags follow the owned control symlink |
| `L3_record_bound_removed` | `l3_raw_budget_includes_dots` | 513 records are accepted |
| `L5_system_test_uid` | `l5_system_constants_and_shared_mode_trust` | ordinary UID violates the pinned zero UID |
| `L9_shared_offset_dup` | `l9_fresh_descriptions_and_owned_lifetime` | `try_clone()` duplicates the shared open description with CLOEXEC retained, isolating offset failure |
| `L9_cloexec_removed` | same L9 test | observer sees absent FD_CLOEXEC |
| `L10_symlink_regular` | `l10_real_entry_kinds` | **expected survivor; blocker below** |

Each mutant runs only its exact named native test using `test(=...)`. Portable
L6 and lexical scanners cannot earn kill credit. Credit requires exit **100**,
exactly one matching nextest `FAIL` status, and no skip/compiler marker. Exit 0
with exact PASS is `survived`; compiler failure, no run, missing/wrong test or
skip is `invalid`. The first non-kill stops further mutants without retry or
loosening the mutation. JSONL stdout contains fixed labels/statuses, elapsed
seconds and exit codes, never raw fixture data, output paths or captured output.

Each subprocess has a wall deadline and 2 MiB combined output cap. Linux
children run in their own process group, killed and reaped on completion,
deadline, output overflow or interruption. The suite has a 480-second budget:
initial baseline at most 180 seconds, each incremental mutant at most 40 seconds,
and 60 seconds reserved for the final baseline. Expiration fails closed, even
when it prevents the final baseline. Workflow setup and the hard job timeout
may also terminate the job; neither earns acceptance credit.

Every applied mutation restores original bytes in `finally` and verifies
SHA-256. There is no `git reset`, checkout, or restoration of unrelated files.
The outer finalizer verifies source restoration, emits a content hash of the
hierarchy directory (sorted relative filenames and file SHA-256 values), and
runs the final baseline, including after a survivor or compiler failure.
The tree inventory is capped at 64 entries and each read at 256 KiB. This
`tree_sha256` is an adapter-directory content fingerprint, not a Git tree ID.
SIGTERM follows finalizers; SIGKILL/hard runner cancellation cannot guarantee
finalization, and must be treated as incomplete evidence in the disposable job.

Original source SHA-256:
`c45d2a416f8504cba34368c5d11558f00d149d4cc31b208ecb41a4b4bbeeb863`.

## Local validation and open blocker

Executed on Windows only:

- `python -I -B -m unittest discover -s tools/ci/e3-native -p test_driver.py -v`:
  24 discovered, 23 passed, one explicitly skipped because Windows denied
  symlink creation. A separate synthetic reparse-attribute guard test passed.
  Tests use only owned temporary files and synthetic Python children: exact
  replacement/target guards, restoration on failure, exit/named-test matching,
  visible skip classification, deadline/output limits, fixed JSON output,
  stop-on-first-failure, and both baselines. No Cargo/native test is launched.
- `cargo nextest --version`: 0.9.151 on Windows. `cargo nextest run --help`
  confirms all selected flags and the `immediate` output setting. This is CLI
  validation, not Linux execution.
- Read-only dry application of all eight pinned rows: every old-text count is
  one, all replacements are production-only, and every named test exists.
  No source bytes were written and no native tests executed.
- `git diff --check`: passed. Product source, existing CI, Cargo, policy and
  PROGRESS remain untouched.

**Blocker: L10 cannot discriminate this exact adapter-only mutant.** The test's
symlink is named `link`. Mapping its `d_type` from `DT_LNK` to `DT_REG` still
produces `Entry::Refused`: E1 requires a dot in a regular control filename.
L10 therefore asserts the same result before and after the mutation. This is
the retained review advisory A1, established by source analysis; it is not an
observed native survivor. Killing it needs an independently reviewed native
fixture change (for example a symlink named `memory.max`) outside this task's
allowed paths. Altering the classifier, target contents, or extra listing
fields just to force failure would test a different mutation. The fixed mutant
is retained honestly, so this harness is expected to fail closed at L10 if
earlier mutants are killed. **No eight-kill acceptance is claimed.**

Fresh Opus 5.5 high security review is required before the coordinator pushes
or permits hosted execution. This sub-agent did not request that review,
delegate, push, watch or execute the workflow. Ubuntu baselines, visible L7/L8
results, all native kills and hosted runtime remain outstanding.

Native, installed and dedicated paths are **unverified on real hardware**.
M2 remains incomplete. `ExecutionFailed`, `E_PROTECTION` and the privacy flake
have unknown causes; no retry or diagnosis was performed. No owner/service/
account/cgroup/sys/VM/experiment/private-state/ignored-fixture operation was
performed. The prepared native tests use owned temporary objects; L8 alone
reads its own process status. There is no activation or owner approval flow.

## Coordinator L10 fixture correction and review handoff

Sol medium correction `231e6d7f950a642b78f7961e5a9fa48bd632e15d` changes only the Linux L10 symlink
fixture name from link to memory.max and refreshes the harness source pin.
A regular memory.max is Control; a symlink must be Refused. This removes
the source-analysis masking described above; it is NOT a native kill claim.
Production linux.rs prefix remains identical to eb93620. No authorization
expansion is needed for a fixture correction within the existing E3 scope.
The earlier blocker section records the original prepared state only.

Windows portable harness suite: 24 discovered, 23 passed, one symlink-creation
test explicitly skipped. Fmt passed; L6/P-SCAN 4/4 passed, run
91efa0dd-91f0-4ea4-a3f0-932e4b76361d. No native or workflow execution.
All eight mutation fragments still require actual ordinary-user Linux kills.
Fresh Opus high security review is required BEFORE push/hosted execution.

Current committed source/harness manifest:

| Path | SHA-256 |
| --- | --- |
| `crates/arxd/src/exec/hierarchy/linux.rs` | `e5b6ac15d01092a96ea23488f23938f1b897717efe19b5bd3f71e898ef54eaff` |
| `.github/workflows/e3-native-evidence.yml` | `c5fcb9a47846cdb564071c4e4080a087356076eea412c66ac1fbf4841d0dea0e` |
| `tools/ci/e3-native/driver.py` | `d7727f88d3f99c57e321707bd397f8ea235bcc3fca22bb4c881fa150975ef22f` |
| `tools/ci/e3-native/test_driver.py` | `e854b17fd1c4228f84fc47fb26e1acd67cd71a4ad7eb06217d4213179adec3ac` |
| `tools/ci/e3-native/mutations.jsonl` | `c824820588a406800124352931597e3a220a32b945333e85d3a186612bca3d93` |

## Harness output correction before hosted execution

Opus high review59525d6 (retained fe5fc99) found B1 nextest progress-counter
parsing and B2 rejection of the normal failure footer. Sol medium correction
78f1f3d accepts the optional numeric counter and requires exactly one
error: test run failed footer for exit100 with the named FAIL. Other compiler
error lines and duplicate/missing footers fail closed. Real 0.9.151 status
line excerpts from the review are now test fixtures; synthetic statuses also
include counters and synthetic kills include the real footer.

Portable suite:25 discovered,24 passed,one Windows symlink skip. The new
real-output regression test fails against the original driver, a counter
parser removal and a footer-check removal, each exit1 on owned temporary
copies. No native mutation is credited. No production source changed.

The workflow explicitly enables CARGO_INCREMENTAL=1 while preserving its
10-minute timeout and all driver deadlines. Fixed failure categories now
appear in final JSON; OS/value/reap exceptions emit only fixed categories,
not exception text. Cold-build timing remains unmeasured, not guaranteed.
Fresh narrow Opus review is required before any push or hosted run.

Updated harness manifest (committed bytes):

| Path | SHA-256 |
| --- | --- |
| `.github/workflows/e3-native-evidence.yml` | `7c7850d57ad6db22a8fd48f74396a4790b4eaa5d09cc853e19f025a725aa2e66` |
| `tools/ci/e3-native/driver.py` | `e359723257d93adbec3861b2e4105b993233b35a79738e96aada30752494cbf0` |
| `tools/ci/e3-native/test_driver.py` | `8b3bb14aec92955a929651eb1afcac62c322e2d5a125a58b01ff20578bc111f3` |

## Padded progress-counter correction

Opus high recheck302175a (retained64264ae) closed B2 and found padded
nextest counters still failed B1. Sol medium correction `729922a3b475e608b05f7176961f5b6359956e5c`
accepts whitespace before the counter numerator, adds the verbatim padded
0.9.151 line, and makes the baseline fixture use padded counters for all
ten native tests. Portable25 discovered:24 passed,one Windows symlink skip.
The unpadded-only regex mutant fails the real-output and baseline tests
on an owned scratch copy (exit1). No native execution or kill credit.
Production and workflow are unchanged; fresh narrow review remains pending.

Updated committed hashes:
- `tools/ci/e3-native/driver.py`: `2916049ab278ddf2a97da851925cf5b2688d3902775c16453beb3a49578489c7`.
- `tools/ci/e3-native/test_driver.py`: `2f3fba561afa0058abd55eaa7a799cc6a0b7acbd4b2c3a7c4b756331dc305471`.

## First hosted publication checkpoint

Opus high padded-counter recheck b6fcefc passes the narrow B1 delta; B2
remains closed from302175a. Reviewed source/manifest pins are unchanged.
Coordinator publishes the reviewed harness for its first hosted execution.
No native result, skip/pass outcome or mutation kill is claimed before that
run completes. Merge still requires native evidence plus refreshed exact-head
integration review and all five required checks. No activation.

## First hosted native evidence result

Run43493588144, attempt1, exact PR61 head231c367, succeeded
2026-11-13 19:35:04-19:36:35Z (91s workflow elapsed). Root verified the
API pin/conclusion/times. Independent Opus high exact-head review
[records the native logs](https://github.com/Dytschgo/arxcovo/pull/61#issuecomment-6863013251):
UID/euid1001, portable25/25 without skips, baseline-before50.7s and
baseline-after2.1s, L7/L8 passed on both, eight named mutants killed with
exit100, restored source pin and matching folder fingerprint. These are
hosted ordinary-user evidence, not product hardware acceptance. The old
c45d2a source value above is historical; current SOURCE_SHA and latest pin
table are authoritative for this exact run.

Exact-head independent integration review passed. Windows required verify
remains pending at this checkpoint; no merge yet. M2 incomplete and native
installed/dedicated paths remain **unverified on real hardware**.
