# Muse independent design review: Stage F A3 driver plan

2026-11-14; Muse Spark (muse-spark-1.3, high); independent design review sub-agent.
No builds, tests, native/service/process/account/owner operation, network, GitHub,
dispatch, subagents, PROGRESS/source edit, push/PR. Git CLI read-only only.

## Pins (exact)

- Sol plan: `6306d543c62246781157927f9c76d4bcc97bfe32`
  (`6306d54 docs(plan): define bounded Stage F A3 driver`).
- Retained/reviewed head: `8c81a45b96cb9b44a6c3d769808f05fce22af6ec`
  (`8c81a45`, same message).
- `git diff 6306d54 8c81a45 --stat` is empty: identical content, one file
  `docs/reviews/m2-stagef-a3-driver-plan.md` (99 insertions, docs-only).
- Referenced source pin `cd561e0` exists
  (`cd561e0 test(exec): prepare dormant Stage F A3 hardening checks`).
- Spec sources read: `docs/briefs/0020-linux-hierarchy-implementation.md`
  (Stage F / slices E1-E7, gates), `docs/adr/0020-linux-execution-hierarchy-and-janitor.md`
  (D8 R4/R5, D10 pins, bounds), current
  `tools/unix-exec-validation/driver.py` (`TEST`, `fixture_override`,
  `install_runtime`, `monitor_a1a2`, `refuse_unit_collision`, `reserve_unit`,
  `cleanup_a1a2`), `crates/arxd/src/exec/disposable.rs:383-483`
  (A1/A2 hold, dormant `assert_a3_hardening`, two ignored A3 tests),
  `packaging/linux/arxd.service`, `packaging/linux/50-arxcovo-standard-rights.conf`,
  `docs/evidence/linux-stagef-a3-source-preparation.md`,
  `docs/reviews/m2-stagef-a3-source-muse-review.md`.
- Not run (as tasked): builds, tests, native query, service/process/account/owner
  operation, host/filesystem probe, network/GitHub/CI/dispatch.

## Standards

Pass. Docs-only, one-concern plan; no PROGRESS/source edit, push, or PR.

- Proportionate fences, no optional infrastructure: closed `TEST` constant
  validated against exactly the three `exec::disposable::` paths before any
  effect (current `driver.py:13` selects only A1/A2; the two A3 paths match
  `disposable.rs:475,480`). No independent profile switch, arbitrary
  test/unit/drop-in argument, environment selector, new CLI mode, or workflow
  input. One case per fresh-host invocation; a later disposition may cover both
  fixed selections together. Future writes bounded to
  `tools/unix-exec-validation/{driver.py,test_driver.py,mutation_check.py}` and
  `crates/arxd/src/exec/disposable.rs`, F0 files only where mocks need
  alignment; no production/parser/setup/Runner, package, Cargo, workflow, ADR,
  or PROGRESS writes.
- Historical evidence handled correctly: A1/A2 valid only for recorded run
  `43627472312`; nothing rewritten. A3/F/G, M2, PA6 stay open and the plan
  labels hardware paths **unverified on real hardware**. No implementation
  choice deferred to the owner.

## Spec

Pass. Matches brief 0020 gating and ADR 0020 D8 R4 without overclaim.

- Reuses the accepted A1/A2 shape: exact TEST set, READY/private identity-ack
  hold (120 s monotonic deadline, no-follow bounded 0600/root/nlink-one
  validation in `disposable.rs:420-446`, absent-ack check `:410`, main-PID
  notification) extracted to a test-local helper, called after A1/A2
  postconditions and after each A3 assertion. No setup added to the A3 tests.
- Profile presence derived from the selected TEST, not a switch: present for
  A1/A2 and A3 Standard, absent for A3 None. Standard keeps all packaged
  profile directives (`NoNewPrivileges=yes`,
  `CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN`); None omits the whole
  file and never simulates absence with a weakening drop-in or NNP/capability
  override. A3 asserts via the existing bounded production procfs reader/parser
  (`LinuxFs::system`, `PROC_MAGIC`, `hierarchy::locate`,
  `parse::hardening`): Standard requires NNP=1 and
  `CapBnd=00000000000000c1`; None requires `Hardening::None`. R4 stays exact,
  including subset refusal.
- Attribution identities are the existing owned set, no invented authority:
  exclusive `/run/systemd/system/arxd.service` from unchanged packaged bytes,
  owned `arxd.service.d` (0700), fixed `90-disposable-a1a2.conf` from existing
  `fixture_override` (ExecStart reset + staged Python launcher, `Restart=no`,
  `RuntimeMaxSec=140`; historical name kept), FragmentPath plus exact
  DropInPaths-set match with byte comparison before start and repetition while
  held/before cleanup, extra fragments/aliases/drop-ins refused including
  identical foreign copies. `service -> harness -> execve` chain, recorded-PID
  capture, 0600/no-follow outputs, exact ignored-libtest argv, minimal
  disposable/`NOTIFY_SOCKET` environment preserved; sole-PID outer checks
  (MainPID = recorded PID, `/proc/PID/exe` = protected artifact, supervisor
  membership holds only that PID); ack releases the live process; only real
  systemd success/zero exit with the exact selected test-ok plus
  one-pass/zero-failure summary passes. Bounds reuse existing values (1 MiB
  capture, 120 s monitor, 140 s runtime, 360 s prebuild, 5 s command / 30 s
  start-stop, 40 s stop); no new bound invented.
- Collision/cleanup matches the existing contract: shared
  `/run/arxcovo-disposable-a1a2.lock` EXCL/flock held from before inspection
  through stage cleanup, refusal of loaded/failed unit, alias, any search-path
  fragment/drop-in, `/run/arxcovo`, fixed cgroup; collision owns nothing;
  trusted fresh-host containment (privileged-writer assumption per D8), not a
  hostile-root defense. Owned-set validation before removal, proven-unit stop
  only, F0b snapshot in stop-finally position, cgroup-absence requirement,
  runtime removal, reload, protected-stage removal, then reservation release;
  partial install/replacement, stop failure, or uncertain cleanup quarantines
  with reservation/evidence retained. F0b order, phase/first/final, mutation
  lower bounds, conditional reads, channels, precedence, closed codes,
  1024/4096-byte bounds, single outer `run` feedback line preserved; no mode
  field/schema. Grants nothing: no dispatch, retry, installation, activation,
  or acceptance; execution disposition stays separate with default-off workflow
  fences.

## Security

Pass. No new privileged path, secret, or trust claim.

- Technical fences only, no approval framework. No account/legacy-fixture
  cleanup, no user choice on the fixture name, no NNP/capability override.
- `unknown`/`untrusted` stay fail-closed via R4 exactness; subset bounding set
  refused; profile absence proved by file omission, never by a weaker file.

## Findings and minimal fix

No blockers; no plan edit required. Two non-blocking precision notes for the
future implementation (not this document): (1) state explicitly that the
extracted helper is the hold loop only and the `READY=1` send stays with each
caller in its current position (postconditions -> READY -> hold); (2) pin the
exact expected `DropInPaths` set-string form per mode when the pre-start check
is written. Both fit inside the plan's stated scope.

Two findings total (both precision notes); worst is cosmetic. Plan accepted as
the source-preparation contract; it grants no native execution.
