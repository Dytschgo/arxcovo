# M2 executor durable privacy fixture pin: independent Muse source/security review

2026-11-15; Muse Spark 1.3 high (review sub-agent). Independent
source/security review of the deterministic test-fixture correction for the
old-PR106 macOS durable-privacy failure, against current main. No
publication, push, native execution, or source/PROGRESS edits in this review.

- Base pinned: `9be76eb26786294ff1f122a407d92919b24132e1` (last merged PR #107).
- Fix pinned: `f541368dd21fd122ddfacaac68ae852d487d3a9a`
  `test(executor): pin host identity for durable privacy scans` (Sol high,
  retained).
- HEAD pinned: `87dfcdeb78f843735d9bd195864e862dbcf079da`
  `docs(progress): queue deterministic durable privacy fix review`.
- `git log 9be76eb..HEAD --oneline`: `7d9338c` (PROGRESS investigation),
  `1be0d62` (merge main into `work/m2-durable-matrix-failure`),
  `f541368` (executor test fix), `87dfcde` (PROGRESS review queue).
- Scope: `git diff 9be76eb...HEAD --stat` shows 3 files, +187/−8:
  `crates/arxd/src/service/executor/tests.rs` (+74/−8),
  `docs/reviews/m2-executor-durable-privacy-ci-evidence.md` (new, 115 lines),
  `docs/PROGRESS.md` (+6). No production source change: `store.rs`,
  `executor.rs`, and every sanitizer/store path have empty diffs.
- Spec: ADR0017 report-validation contract (typed `OnceReport` dispatch,
  identity-privacy withholding, output-release gating) plus the Sol evidence
  document above. Method: full read of the test diff, the evidence document,
  the PROGRESS entries, ADR0017 normative sections, `store.rs`
  `open_inner`/`read_host_id`, and the `OnceReport`/`private_temp` fixtures;
  call-graph audit before any test; independent re-run of the 8 ADR0017
  executor filters, `cargo fmt --check`, `git diff --check`, and workspace
  Clippy only. No native process, helper, service, network, or CI retry
  execution occurred in this review.

## Standards — PASS, no findings

- One-concern conventional commits: `f541368` touches only
  `crates/arxd/src/service/executor/tests.rs`;
  `87dfcde`/`7d9338c` touch only `docs/PROGRESS.md`; evidence document is
  docs-only. No production, Cargo/package, workflow, policy, or ADR change.
- Every behavior ships with a test: the new
  `adr0017_durable_privacy_host_id_collision` regression proves the
  collision, its removal by the pin, and that the pin does not conceal
  planted markers; the matrix itself is the unchanged acceptance surface.
- Fowler smell baseline: no finding. `pin_durable_privacy_host_id` is
  literally named; the new `contains` closure is used at all three
  assertion sites (no duplication); the closed `(marker, secret)` label
  pairs remove the old bare-format assertion without adding abstraction;
  no new types, switches, chains, or middle men; one reason for the test
  change (fixture determinism).

## Spec — PASS, no findings

Quoted against the evidence document and the test diff:

1. Pin updates only the unrelated host identity, before dispatch:
   `pin_durable_privacy_host_id` (`tests.rs:932-940`) issues a single
   `UPDATE local_host_identity SET host_id='a'×64` on a TRUNCATE-mode
   connection, called once per matrix case after `open_service_fixture`
   and before `dispatch` (`tests.rs:1542`). Production host generation
   (`store.rs:111-117`, 32 random bytes hex-encoded) and all
   sanitizer/store code are untouched.
2. No privacy-coverage weakening: all six raw-byte markers are retained
   (`sid_b`, `output`, `uid_b`, `gid_b`, `malformed_sid`, `overlong_sid`,
   `tests.rs:1794-1801`), checked against both
   `activity.sqlite3` and `activity.sqlite3-journal` after
   `drain`/`drop` (`tests.rs:1788-1808`). Immediate output, durable
   metadata, `list_activity`, all 81 matrix cases, and the exact
   `output_counts (12, 11)` / `cleanup_pair` assertions are unchanged.
3. Close/reopen and journal-mode interactions are correct: the pin matches
   the service `TRUNCATE` mode set by `open_inner` (`store.rs:94`), so the
   pin's own journal commit truncates rather than lingering for the later
   raw scan; the `Activity` handle stays open across the pin (file lock
   held, SQLite serializes the two connections), and the matrix proceeds
   through normal `dispatch`/`drain`/`drop` close paths.
4. Regression proves planted markers remain visible after the pin:
   each of the six synthetic markers is inserted into `privacy_probe` and
   re-scanned after re-pinning (`tests.rs:969-983`), so the pin cannot
   mask a real persisted leak. The probe table exists precisely because
   store CHECKs reject malformed/overlong identifiers through the real
   `activity` path.
5. False-positive mechanism is proven locally without overclaiming: the
   colliding 64-hex identity (`tests.rs:949`, containing `62002` and
   `63003`) is a valid `read_host_id` value (`store.rs:438-461`,
   asserted via `activity.host_id()` at `tests.rs:953`) and trips the
   raw scan before any executor/report call. The evidence document
   correctly keeps the historical macOS matched marker **unconfirmed**
   (no CI artifact was fetched; `windows_admin` label alone identifies no
   marker/table), and this review concurs: mechanism proven, historical
   instance not attributed.
6. No native execution: the matrix backend is `OnceReport`
   (`tests.rs:313-321`), a mutex-held typed `Report` consumer; the
   `Native` executor (`executor.rs:146-271`) is not instantiated in this
   path. The symptom run's macOS native API surface is therefore
   uninvolved in the reviewed change.

## Security — PASS, no findings

- No secrets or private owner data: markers are synthetic (`SID_B`,
  `ARX0017-OUTPUT-MARKER`, `62002`/`63003`, `S-1-x`, overlong
  `S-1-5-0…1`); the pinned `a×64` identity is a fixed non-secret
  fixture value confined to `tempfile` dirs under `private_temp`
  (0700 on Unix, owner-only DACL on Windows).
- Path handling stays bounded: the pin takes `dir.path()` from the
  existing fixture tempdir and joins one fixed filename; no new path
  input, symlink traversal, or reparse-point handling is introduced.
  `open_service_fixture` retains its `checked_path`/`private` guards
  (`store.rs:49-85`).
- Write scope is minimal: one row of one fixture-only table per case,
  no privilege, service-state, or cross-test mutation; concurrent
  `Activity` + pin connections are serialized by SQLite and the
  activity lock, with TRUNCATE mode matched on both ends.

## Checks rerun (this review, same worktree)

- `cargo test -p arxd --locked --offline --lib
  service::executor::tests::adr0017_`: 8 passed, 0 failed (3.43s).
- `cargo fmt --all --check`: clean. `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --locked --offline -- -D
  warnings`: exit 0, no warnings.

Close: Standards 0 findings; Spec 0 findings; Security 0 findings. Worst
issue in any axis: none. Historical CI exact marker stays unconfirmed by
design. M2/full F/G/PA6 remain open; hardware paths **unverified on real
hardware**.
