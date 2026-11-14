# Muse independent source review: Stage F A3 dormant hardening checks

2026-11-14; Muse Spark (muse-spark-1.3, high); independent Standards/Spec/security
source review sub-agent. No delegation, no native query, no product CLI, no
Win32/process/service/account/owner operation, no native filesystem probe, no
network/GitHub/CI/dispatch, no ignored-test execution, no PROGRESS edit, no
source edit, no push/PR.

## Pins (exact)

- Sol original: `cd561e0efac11e6745ba2a2b22c3864d7f76a8ed`
  (`cd561e0 test(exec): prepare dormant Stage F A3 hardening checks`).
- Retained/reviewed head: `41ff2b158ba4f4b9df56a0bee739636e50a1f263`
  (`41ff2b1`, same message), parent `c23e0b4ae33878cff0d28d84738d11ea03986bf9`.
- Prepared HEAD `ed157e8662a0a02b438e63fc5550ab41bb2d9293` (`ed157e8`) adds
  `docs/PROGRESS.md` only (2 lines); not in review scope.
- Scope diff `git diff c23e0b4..41ff2b1 --stat` is exactly three files:
  `crates/arxd/src/exec/disposable.rs`,
  `crates/arxd/src/exec/hierarchy/parse.rs`,
  `docs/evidence/linux-stagef-a3-source-preparation.md`.
- Source blobs verified by `git hash-object` in this session:
  `crates/arxd/src/exec/hierarchy/parse.rs` =
  `145a44d4912fc28c39b0832d7decb331b64b465e`,
  `crates/arxd/src/exec/disposable.rs` =
  `d484890abfe271a9b05d164c9aecb1066308219a`,
  matching `docs/evidence/linux-stagef-a3-source-preparation.md`.
- Spec sources read: `docs/briefs/0020-linux-hierarchy-implementation.md`
  (Stage F / slices E1-E7, gates), `docs/adr/0020-linux-execution-hierarchy-and-janitor.md`
  (D8 R4/R5, D10 pins, staging).

## Checks actually run (only allowed exact filters + fmt/clippy)

- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::a3_packaged_profile_status_fixtures -- --exact`: 1 passed.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::p6_exact_hardening -- --exact`: 1 passed.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::bounded_valid_inputs_and_total_byte_data -- --exact`: 1 passed.
- `cargo clippy -p arxd --tests --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `git` CLI read-only inspection only (`log`, `diff`, `show`, `status`,
  `rev-parse`, `hash-object`).
- Not run (as tasked): Linux target compilation, native query, service/process/
  account/owner operation, host/root/filesystem probe, network/GitHub/CI,
  dispatch, Python tests, ignored tests.

## Standards

Pass. Test-only delta with proportionate fences, no optional infrastructure.

- `crates/arxd/src/exec/hierarchy/parse.rs`: production `hardening()` unchanged
  (exact `NoNewPrivs:\t1` + `CapBnd:\t00000000000000c1`, 8192-byte bound intact);
  only `mod tests` gains `a3_packaged_profile_status_fixtures`. Synthetic
  status inputs cover profile-on, NNP-off, broader-bound, both-mismatch, and
  wrong-bound-with-matching-effective/permitted sets; package pin reads the
  unchanged `packaging/linux/50-arxcovo-standard-rights.conf`
  (`NoNewPrivileges=yes`,
  `CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN`) plus the independent
  `(1<<7)|(1<<6)|1 == 0xc1` ABI check.
- `crates/arxd/src/exec/disposable.rs`: module is `#[cfg(all(test,
  target_os = "linux"))]` (`crates/arxd/src/exec/mod.rs:373-374`), so no
  production behavior change. New helper `assert_a3_hardening` plus two
  `#[ignore]` tests (`disposable_linux_a3_standard`, `disposable_linux_a3_none`)
  only assert via existing production APIs: `LinuxFs::system`
  (`crates/arxd/src/exec/hierarchy/linux.rs:102`), `proc_cgroup`/`proc_status`
  returning `ProcFile { magic, bytes }` (`linux.rs:211-215`), `PROC_MAGIC`
  (`setup.rs:9`), `hierarchy::locate`, `hierarchy::parse::hardening`. Gates reuse
  the existing four disposable-host env vars plus real/effective root, then
  procfs magic and the fixed `arxd.service/supervisor` location parse. No setup,
  Runner, account, service, or new-harness call. Comment header states the future
  driver must establish package profile presence/absence and runtime attribution.
- Evidence `docs/evidence/linux-stagef-a3-source-preparation.md` correctly labels
  fixtures synthetic (not captured kernel/systemd output), claims no mutation or
  acceptance credit, and records Linux assertions as uncompiled/unrun on Windows.
- No baseline smell worth a change; one-line doc-comment narrowing is cosmetic.

## Spec

Pass. Matches brief 0020 / ADR 0020 D8 R4 without overclaim.

- Production behavior/profile/workflow/selection unchanged: diff contains no
  production, packaging, workflow, driver, adapter, Cargo, policy, or ADR change.
  `tools/unix-exec-validation/driver.py:13` still selects only
  `exec::disposable::disposable_linux_a1_a2`; neither A3 test is selected.
- Missing future attribution not overclaimed: evidence and code comments state a
  separately reviewed driver must still establish unchanged-package profile
  presence/absence and runtime attribution; no design/execution authority for that
  driver follows from this slice. This respects ADR 0020 D8 R4 limits (R4 proves
  only NNP + exact bounding set; effective/permitted sets and cgroup-write success
  stay with Stage F).
- Native assertions compile against the real API by file-read verification only
  (signatures above); actual Linux compile/run remains unproven, as stated.
- Builder claims corroborated: three exact pure tests pass here, Clippy/fmt and
  diff checks pass; Linux assertions were not executed (ignored, Linux-gated).
  No A3 acceptance or run authority is granted; M2, remaining Stage F (including
  A3), Stage G, and real-Runner PA6 stay open; hardware paths remain
  **unverified on real hardware**.

Two findings total (one per axis); worst is cosmetic wording only. No blocker.
