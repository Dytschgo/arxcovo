# M2 A8 startup lifecycle join — independent Muse review

Scope: base `9d5be079adc92c9426ed0f5054e2e0e964cfb9dd`, head `653d49870dded8d97787c2b771a505faf4696f71`
(`1e0c12d` Sol builder + `653d498` PROGRESS-only). Four files, +275/−0:
`exec/stage_b_tests.rs` (+2), `service/unix_bridge/tests/e7.rs` (+185 `mod a8`),
`evidence/linux-a8-startup-discovery-join.md` (+84), `PROGRESS.md` (+4).
Production diff over `application.rs`, `unix_bridge.rs`, `readiness.rs`,
`store.rs`, `executor.rs`, `service.rs`: empty. Driver
`service/unix_bridge/tests.rs`: unchanged (raw SHA-256
`E500E65FCFD42F30D0492BCED7AC6911DE9E0DC6619E50E87E82CC94ACED7EAE`,
matches the evidence S2b pin). Nonnative only; no native/process/fork/service/
cgroup/sudo/network/subagent/publication/push/owner steps taken here.

Spec source: [brief 0020](../briefs/0020-linux-hierarchy-implementation.md),
A8 line 619 ("Startup discovery runs after Ready and writes one daemon row"),
plus E7 W1/W2/W8 and E6 T15/T16 as the joined prerequisites.

## Standards

- One concern, small conventional delta: a single inline `mod a8` regression
  plus its exact inventory pin, evidence, and PROGRESS lines. No smell beyond
  judgement calls: `LifecycleFixture` is a thin `Application` adapter over the
  existing E7 `App` (no Middle Man — it exists so `Host::run` drives the real
  lifecycle); the three-case loop shares one body (no Duplicated Code — cases
  differ only by `failure`/`waiting`/`expected`); no new abstraction, parameter,
  or production hook (no Speculative Generality).
- Source inventory is strengthened, not weakened: the pin asserts exactly one
  `mod a8 {` occurrence inside the already inventoried, test-only `e7.rs`;
  `tests.rs` gains no module declaration, so the S2b driver pin needs no
  refresh. Verified `StartupDomain::new(` gains no new literal in `e7.rs`
  (the A8 test reuses the `app()` helper), so the W8 count pin (4) is intact.
- No production naming, API, or error-text change; `RunSpec` field usage is the
  corrected `arguments` form.

Findings: none.

## Spec

Against brief 0020 A8 + E7/E6 prerequisites, by callgraph inspection
(`Host::run` in `service/mod.rs:114`, real `start_discovery` sequence in
`service/application.rs:253`, `TestHooks::start_discovery` taking the actual
`StartupDomain` setup in `e7.rs:56`, `fake::startup_setup` in
`exec/hierarchy/fake.rs:1019`):

- Real join, not a stub: `LifecycleFixture::start_discovery` delegates to the
  actual `ServiceApplication::start_discovery` (setup → Ready barrier → row
  insert → `dispatch` → cache); `Host::run` drives actual
  load/verify/open/service_start/bind(no-op)/preflight/discovery/serve/stop/
  drain/release; teardown is real stop/drain/release with file-backed SQLite
  reopened and read back unfiltered (`SELECT ... FROM activity ORDER BY id
  LIMIT 8`, no `WHERE`, max 3 rows vs limit 8 — a wrong tool/actor/principal
  row cannot hide).
- Pending-before-Ready asserted inside `serve` on the real path: setup held at
  the `"setup"` gate, `check()` is `MechanismUnavailable`, zero discovery rows
  via `rows()`, zero port calls.
- Success: Ready, `tool-probe` exactly once (`spec.arguments ==
  ["tool-probe"]`), `allocations == 1`, full row set exactly
  `service_start/succeeded + discover_tools/succeeded/dedicated_account +
  service_stop/succeeded` (daemon/arxd scope).
- Setup failure (`failure=true`, emptied status bytes): stays unavailable, zero
  rows, zero probes, no daemon row.
- Janitor quarantine (`waiting=true`, never-emptying `arx-17-1` leaf,
  `kill_delay = None` → `KillTimeout` → `Quarantined`): zero rows, zero probes,
  expected `Err(Quarantined)`.
- Duplicate-startup guard: a second `start_discovery()` after completion starts
  nothing (production `discovery_started` flag, `application.rs:257`), probe
  count stays 0/1.
- Not a duplicate of existing tests: E7 `w1_w4_t20b` proves order/single-domain
  and `w2_w3` proves setup-failure/shutdown boundaries by invoking discovery
  directly; E6 `t15/t16` prove mapping/monotonicity. None drives `Host::run`
  through to reopened durable rows across all three outcomes — that join is the
  new coverage. `allocations == 1` plus the shared-latch E6/E7 tests forbid a
  second runner/domain on handle creation.

Findings: none. Gaps (out of scope, remain open): A8 native acceptance, M2,
full F/G, PA6, real-Runner behavior — hardware paths **unverified on real
hardware**.

## Security

- No privileged operation: `bind` is a no-op (no socket/service-manager/
  identity work); the only execution port is `FakePort` returning the synthetic
  valid probe table; setup runs on `Fake` `CgroupFs` with a manual clock.
- No secret or owner data: synthetic table/markers only; no PATH/`sudo`/
  service/cgroup/network access added.
- Bounded reads: `LIMIT 8` covers the complete 2–3 row set; `rows()` filtered
  helper is used only for the pre-Ready emptiness check, while the final
  assertion reads the whole table.
- Reviewer ran nothing privileged and performed no native/publish/push actions.

Findings: none.

## Checks run by this reviewer

- `cargo test -p arxd --locked --offline
  service::unix_bridge::tests::e7::a8::host_ready_discovery_persisted_once_or_absent_on_setup_failure
  -- --exact` → 1 passed, 0 failed.
- `cargo test -p arxd --locked --offline --lib
  exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory -- --exact`
  → 1 passed.
- `cargo fmt --all -- --check` → clean; `git diff --check` → clean.
- Builder credit reused as read-only evidence for the other six exact tests,
  offline Clippy, and hosted gates — not reclaimed as reviewer runs; hosted
  Linux/macOS checks remain coordinator work before any merge.

## Verdict

Accept the joined A8 nonnative lifecycle regression as reviewed: meaningful
Host/StartupDomain/discovery/audit/reopen join, three outcomes correct,
exactly-one daemon row only on success, zero probes/rows on failure paths,
production and driver unchanged, inventory pin tightened. Verdict: **PASS**
(non-native scope only). Required before merge: independent hosted
Windows/macOS/Ubuntu gates on this exact head, coordinator-owned; A8 native,
M2, full F/G, PA6 stay open. No PROGRESS or source edits made by this review.
