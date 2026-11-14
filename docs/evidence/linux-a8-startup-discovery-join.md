# A8 startup discovery: nonnative lifecycle join

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-15. Base
`bf89f30b05ac2620556e9ef7c18b887472ef4b96`; initial worktree clean.
Read AGENTS, KICKOFF, ADR 0019/0020, brief 0020, service lifecycle,
StartupDomain/readiness, application discovery, and E6/E7 coverage.

## Coverage decision

E7 `w1_w4_t20b_order_single_domain` already proves setup/Ready/row/port
ordering, duplicate startup calls, daemon/arxd persistence and agent invisibility.
`w2_w3_shutdown_boundaries` proves setup-failure suppression and shutdown
boundaries. E6 `t15_exhaustive_setup_failure_dispatch` and
`t16_monotone_first_cause` prove failure mapping and readiness monotonicity.
The lifecycle tests use a FakeApp; E7 invokes discovery directly. Neither
joined Host::run to completed startup discovery and reopened SQLite across
successful, failed and janitor-quarantined setup.

Added one regression, inline in the existing test-only E7 source:
`service::unix_bridge::tests::e7::a8::host_ready_discovery_persisted_once_or_absent_on_setup_failure`.
It reuses the actual StartupDomain and E7 fake filesystem/manual clock setup,
real readiness, ServiceApplication discovery/audits/drain/release, Execution,
UnixBridge, and file-backed SQLite. A test-local Application wrapper supplies
environment startup and an in-memory serving loop. No listener is bound.
The only execution port is FakePort returning the synthetic valid probe table.
This does not verify native environment startup, config loading, preflight,
listener behavior or a real runner.

Each of three cases holds actual setup pending, observes zero discovery rows
and zero port calls, then releases it. Successful setup reaches Ready and calls
tool-probe exactly once. Invalid setup stays unavailable; a leftover which never
empties times out using the manual clock and becomes Quarantined. Both failure
cases complete the discovery sequence with zero rows/probes. A repeated startup
call after completion and the real Host stop/drain/release finish before readback.
The reopened database's complete bounded row set must equal successful daemon
service_start/service_stop audits, plus exactly one daemon/arxd/action/
discover_tools/succeeded/dedicated_account/finished row only on success.
No filtered query can conceal a row with the wrong tool or identity scope.

No production defect was reproduced; production source is unchanged. No
production red-green claim. Initial test development caught a RunSpec field typo
(`args` versus `arguments`) and a redundant list_activity assertion that itself
created an agent audit row. Corrected the field and removed that assertion;
existing E7 retains the visibility proof. No mutation runs: the existing E6/E7
gates already cover the individual transitions; this slice adds their lifecycle
and durable-audit join without repeating those mutations.

## Exact local validation

On Windows x86_64-pc-windows-msvc, Rust 1.95.0, all eight exact selections
passed (one test each, zero failures/ignored). Each used
`cargo test -p arxd --locked --offline <name> -- --exact`; the seven existing
tests additionally used `--lib`:

- `service::unix_bridge::tests::e7::a8::host_ready_discovery_persisted_once_or_absent_on_setup_failure`
- `service::unix_bridge::tests::e7::w1_w4_t20b_order_single_domain`
- `service::unix_bridge::tests::e7::w2_w3_shutdown_boundaries`
- `service::unix_bridge::tests::e7::w8_production_and_domain_source_pins`
- `service::unix_bridge::tests::e6::t15_exhaustive_setup_failure_dispatch`
- `service::unix_bridge::tests::e6::t16_monotone_first_cause`
- `exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory`
- `service::tests::startup_and_stop_order_includes_cancellation_and_durable_audit`

Final `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --locked --offline -- -D warnings`,
and `git diff --check` passed. Hosted Linux/macOS checks were not run.

The explicit inactive-source inventory now pins the inline `mod a8 {`
declaration inside already inventoried, test-only E7. No new file exemption or
scanner weakening. `service/unix_bridge/tests.rs` is unchanged: its normalized
LF SHA-256 remains
`e500e65fcfd42f30d0492bced7ac6911de9e0dc6619e50e87e82cc94aced7eae`, matching
the existing S2b driver pin. No refresh is required for this slice. If integration
instead adds a module declaration to tests.rs, the coordinator must refresh that
exact S2b pin through review; this slice does not edit the CI driver.

## Boundary

Nonnative evidence only. A8 native acceptance, M2, full Stage F, Stage G and
real-Runner PA6 remain open. Hardware paths are **unverified on real hardware**.
No native runner/service/owner/dispatch operations, network, subagents,
publication, push or PR. No hierarchy/linux.rs, disposable.rs, A4, ADR,
workflow, Cargo, driver or policy change. Independent review and required
hosted checks remain coordinator work before any merge.
