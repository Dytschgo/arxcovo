# Muse independent source review: Stage F A3 closed driver selections

2026-11-14; Muse Spark (muse-spark-1.3, high); independent Standards/Spec/security
source review sub-agent. No delegation, no native query, no product CLI, no
Win32/process/service/account/owner operation, no native filesystem probe, no
network/GitHub/CI/dispatch, no ignored-test execution, no PROGRESS edit, no
source edit, no push/PR.

## Pins (exact)

- Sol original: `9a78227ba0af7901425dc8c4c614ac72cb5ac9cb`
  (`9a78227 test(exec): prepare closed A3 driver selections`).
- Retained/reviewed head: `1b5544cab8502a2c99ece72574197709c236545e`
  (`1b5544c`, same message).
- Evidence original `4742ad81f57a45baf8a3596b5ec34cd34f28a380` and retained
  `c514a8656091c7212acb9169fd5cebdcf1401025` (current HEAD) both record the
  same preparation checks.
- `git diff 9a78227 1b5544c` is empty: identical content.
- `git diff 4742ad8 c514a86` is empty: identical content.
- Base: actual `main` `20a19e11ff713b0ceb8dd9dd0a84554a3a9c1ba8`.
- Accepted contract: plan `6306d543c62246781157927f9c76d4bcc97bfe`
  (retained `451e0a9`, empty diff) and Muse plan review
  `89689b9` (retained `15aacb3`, empty diff).
- Scope diff `git diff 15aacb3 1b5544c --stat` is exactly six files:
  `crates/arxd/src/exec/disposable.rs`,
  `tools/unix-exec-validation/driver.py`,
  `tools/unix-exec-validation/test_driver.py`,
  `tools/unix-exec-validation/mutation_check.py`,
  `tools/unix-exec-validation/test_f0_blockers.py`,
  `tools/unix-exec-validation/test_f0b_transport.py`.
- No production, packaging, Cargo, workflow, ADR, or PROGRESS change in the
  source commit. `docs/evidence/linux-stagef-a3-driver-preparation.md` is the
  supporting evidence file, not in review scope for behavior.

## Checks actually run (only allowed exact filters + fmt/Clippy/mocked mutants)

Call graph inspected before running: the three exact Rust tests below call
only the pure `hardening()` parser and compile-time `include_str!` package
text (`crates/arxd/src/exec/hierarchy/parse.rs:413-468`); no `LinuxFs`,
setup, Runner, subprocess, or native call. `mutation_check.py::rust_mutants`
compiles stdin slices of the real pure ports and excludes native adapters;
all Python mutants are in-memory with mocked tests. Linux-only ignored
fixtures are not compiled or executed on Windows.

- `python tools/unix-exec-validation/mutation_check.py`: 72 audited tests OK;
  23 Python assertion kills; 3 exact pure Rust baseline passed; 4 compiled
  Rust assertion kills; driver source hash unchanged
  (`4d81acdf62009e284b55d7219699a094d4d4dc445b7bc6673dc7b7671d7c1d6e`).
- `python tools/unix-exec-validation/test_f0_blockers.py --mutants`: 23
  assertion kills, source hash unchanged.
- `python tools/unix-exec-validation/test_f0b_transport.py --mutants`: 36
  assertion kills, 4 explicitly equivalent/no-credit edits, source hash
  unchanged.
- `cargo test -p arxd --lib --locked --offline
  exec::hierarchy::parse::tests::a3_packaged_profile_status_fixtures -- --exact`:
  1 passed.
- `cargo test -p arxd --lib --locked --offline
  exec::hierarchy::parse::tests::p6_exact_hardening -- --exact`: 1 passed.
- `cargo test -p arxd --lib --locked --offline
  exec::hierarchy::parse::tests::bounded_valid_inputs_and_total_byte_data
  -- --exact`: 1 passed.
- `cargo clippy -p arxd --tests --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `git` CLI read-only inspection only (`log`, `diff`, `show`, `rev-parse`,
  `status`).
- Not run (as tasked): Linux compilation, native query, service/process/
  account/owner operation, host/root/filesystem probe, network/GitHub/CI/
  dispatch, ignored tests. Linux execution remains **unverified on real
  hardware**.

## Standards

Pass. One-concern test/preparation delta with proportionate fences; no
optional infrastructure.

- `tools/unix-exec-validation/driver.py:13` keeps `TEST` as the one
  source-selected plan constant with A1/A2 as the default; no independent
  profile switch, arbitrary test/unit/drop-in argument, environment selector,
  new CLI mode, or workflow input.
- `standard_profile()` (`driver.py:23-27`) validates `TEST` against exactly
  the three `exec::disposable::` paths before any effect and derives profile
  presence from it; called in `test_argv`, `install_runtime`,
  `runtime_files`, `run_body`, and `main`, so invalid selection raises
  `E_TEST_ARGS` before mutation or privileged probe.
- `runtime_files()`/`verify_runtime()` (`driver.py:775-803`) reuse the owned
  set: exclusive `/run/systemd/system/arxd.service` from unchanged packaged
  bytes, owned `arxd.service.d` (0700), fixed `90-disposable-a1a2.conf` from
  existing `fixture_override` (historical name kept). Packaged files are 453
  and 134 bytes, inside the default 4096-byte `private_read` bound.
- `crates/arxd/src/exec/disposable.rs` extracts the A1/A2 hold loop verbatim
  into `hold_identity_ack()` and keeps postconditions -> `READY=1` -> hold in
  every caller; A3 tests add the absent-ack check, assertion, `READY=1` send,
  then the hold-only helper, adding no setup. This implements both precision
  notes from the plan review.
- `test_driver.py` adds one focused test per behavior (closed selection,
  pairings/install bytes, attribution lifecycle, exact attribution, sole PID,
  exit/ack/real-success, read metadata, exclusive mode, remove-set
  validation, capture mode/nlink, extended summary); F0/F0b diffs are
  one-line `verify_runtime` mock patches at the existing mock boundary only.
- `mutation_check.py` extends the audited in-memory mutants for every new
  fence (swap/omit profile, skipped drop-ins/bytes/sole-PID/held/prestart/
  cleanup attribution/identity) and sharpens the Rust pure baseline to three
  exact filters; the `wrong test` anchor now mutates the default to
  `a3_standard` while the new `test_closed_selection_before_effects` still
  covers the cross-account value.
- No baseline smell worth a change; `runtime_files` removes what would have
  been duplicated install/verify path logic.

## Spec

Pass. Matches the accepted plan without overclaim.

- Exact file bytes/sets/modes: `verify_runtime` checks `LoadState`,
  `Id`, `Names`, `FragmentPath`, and the exact space-separated `DropInPaths`
  set-string per mode (profile drop-in first, fixture drop-in last; None has
  the fixture only), the 0700 root-owned drop-in directory, exact directory
  membership with no extras, and per-file byte equality via `private_read`.
  Refusal is `E_UNIT_COLLISION`, including identical foreign copies.
- Lifecycle: prestart attribution after `daemon-reload` before `start`
  (`driver.py:899`), held attribution on every active pre-ack poll
  (`driver.py:645`), and cleanup attribution after owned-set identity check
  before `stop` (`driver.py:929`). Reservation, collision refusal
  (loaded/failed unit, alias, any search-path fragment/drop-in,
  `/run/arxcovo`, fixed cgroup; collision owns/cleans nothing), and cleanup
  lifetime are unchanged.
- Sole PID before ack: `MainPID` equals the recorded PID, `/proc/PID/exe`
  equals the protected artifact, supervisor membership holds, and the new
  bounded `cgroup.procs` read must equal exactly `PID + '\n'` before
  `identity-ack` is created (`driver.py:657-664`).
- Exit fence: terminal state requires ack plus `Result == 'success'` and
  `ExecMainStatus == '0'`; `verify_test_result` additionally requires the
  exact selected `test {TEST} ... ok` line, one-pass/zero-failure summary,
  returncode 0, and existing `harness-pid`. Wrong result, wrong test, zero
  tests, overflow, and early exit all refuse.
- Collision/owned cleanup and F0b contract preserved: `same_object`
  whole-set validation before any removal, stop of the proven fixed unit only,
  F0b snapshot in stop-finally position, cgroup-absence requirement, then
  removal/reload/stage removal and reservation release. Closed codes
  unchanged (no new code, no mode field/schema); 1 MiB capture, 120 s
  monitor, 140 s runtime, 360 s prebuild, 5/30 s command, 40 s stop, and
  1024/4096-byte bounds unchanged.
- No execution authority follows: evidence and code comments grant no
  dispatch, retry, installation, activation, or acceptance; A1/A2 history
  stays valid only for its recorded run.

## Security

Pass. No new privileged path, secret, or trust claim.

- All runtime paths are fixed absolutes under `/run/systemd/system`; no user
  input reaches a path. Writes use `O_EXCL | O_NOFOLLOW | O_CLOEXEC` 0600
  exclusives; reads use no-follow, root/0600/nlink-one/size validation.
  Drop-in directory pinned to 0700 root-owned with exact-membership refusal.
- Profile absence is proved by file omission; no weakening drop-in and no
  NNP/capability override exist. `Hardening::None` vs `Standard` still comes
  from the existing bounded production procfs reader/parser with exact R4
  matching.
- Trusted fresh-host containment assumption is unchanged and stated; this is
  not a hostile-root defense. No account/legacy-fixture cleanup, no user
  choice on the fixture name, no secret or owner data in source or logs.

## Findings

No blockers; no source edit required. Two advisories, both non-blocking:

1. `verify_runtime` re-reads package bytes and all runtime files on every
   10 ms active monitor poll until ack; files are tiny (453/134 bytes) so
   this is bounded, but a future slice could verify once per state change.
2. Linux compilation and execution of the new driver paths and the ignored
   A3 tests remain unrun on Windows; A3 acceptance stays open per the plan.

Two findings total (both advisories); worst is the unrun-Linux note, which
the evidence already labels honestly. Source accepted as the
source-preparation contract; it grants no native execution.
