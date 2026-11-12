# N2 isolated integration evidence

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12.
No subagents, GitHub writes, activation, owner administration or native experiment.

## Exact boundaries

- Prepared base: `937af556af2acda5b2b1f73eda5002c4ae0e9cf0` (documentation ledger).
- Actual main: `0937cacfcdb5be036edcd1a126ba0f65521599df` (#36).
- Retained N3 head / N2 base: `1976afb1a2f3e7d2b22cbf819909a0a22575be24`,
  local ref `integration/n3-reviewed-port`; [N3 evidence](n3-integration-port.md).
- N2 runtime/contract/reports: `dc30f5ab7ffa3fbcb818525399dd4e3066e432a1`.
- N2 final head: the evidence/ledger child of that commit, retained by local ref
  `integration/n2-reviewed-port`. Resolve with `git rev-parse integration/n2-reviewed-port`.
  The child changes documentation only; all tested source bytes are identical.

## Manifest (N2 only, relative to N3)

- `crates/arxd/src/local_execution_tests.rs`
- `crates/arxd/src/service/application/tests.rs`
- `crates/arxd/src/service/executor.rs`
- `crates/arxd/src/service/executor/tests.rs`
- `crates/arxd/src/store.rs`
- `docs/PROGRESS.md`
- `docs/adr/0009-service-lifecycle-and-dedicated-execution.md`
- `docs/adr/0017-dedicated-report-validation.md`
- `docs/briefs/0017-report-validation-implementation.md`
- `docs/evidence/n2-integration-port.md`
- `docs/reviews/0017-blocker-corrections-opus-recheck.md`
- `docs/reviews/0017-implementation-opus-review.md`
- `docs/reviews/0017-local-implementation-evidence.md`
- `docs/reviews/0017-local-mutations.jsonl`
- `docs/reviews/0017-output-release-disposition.md`
- `docs/reviews/0017-output-release-evidence.md`
- `docs/reviews/0017-output-release-mutations.jsonl`
- `docs/reviews/0017-output-release-sol-review.md`
- `docs/reviews/0017-report-contract-correction-disposition.md`
- `docs/reviews/0017-report-contract-sol-recheck.md`
- `docs/reviews/0017-report-contract-sol-review.md`
- `docs/reviews/0017-review-blocker-corrections.md`
- `docs/reviews/0017-unverified-output-probe.md`
- `docs/reviews/0017-unverified-output-probe.patch`
- `docs/reviews/service-report-validation-probe.md`
- `docs/reviews/service-report-validation-probe.patch`

## Port and equivalence

Applied only the four scoped runtime/test file patches from `a26570d..00fdd37`.
All four files are byte-equivalent (normalized LF) to both reviewed correction
`a803c361147917241111f1f726d7b7c64de3a6aa` and retained `00fdd37`:
executor.rs, executor/tests.rs, application/tests.rs, store.rs.
No entire application.rs, service/mod.rs or old local_execution_tests.rs was copied.
The main SDK fixture has exactly one textual difference: backend_fixture becomes
default_fixture to match the reviewed typed-pin fixture helper. Its Executor and
FakeForTest implementation, real automatic 200ms SDK timeout, explicit cancellation,
held-preflight counterexample setup and durable-before-shutdown assertions are unchanged.
Both cancellation tests ran in the single full workspace pass.

N3 shutdown/drain and worker-lock/stopping-priority source segments compare exactly
to a26570d. The accepted worker body compares exactly after replacing only its old
pre-spawn Completed cancellation report with typed CancelledBeforeSpawn. Thus the
reviewed synchronous refusal, accepted-worker drain and quarantine priority remain.

The 81 named durable-case expectations, wrong-token-class, post_switch and reverse
boundary cases, creation phases, immutable Pin, pure assess, sanitized Metadata,
store-owned syntax helper and nonempty Application/MCP output-privacy tests match
reviewed source. Capability uses independently cached configured rights; assessment
uses pin rights; backend preflight still precedes pin refusal. Full-match Some(true)
is the sole release gate; otherwise both streams use fixed empty strings and null
counts. Production Native still returns MechanismUnavailable; schema 3 is unchanged.

The coordinator explicitly clarified that the exact reviewed synthetic parse_inventory /
Execution::configured cached-capability test is authorized. Its exact-JSON and pin
validity assertions are unchanged. It does not execute preflight, its stored native
closure, NativeResolver, account lookup or a process backend. This exception does
not authorize native execution or new production constructors.

Historical mutation evidence is retained without claiming reruns: the original
validator [evidence](../reviews/0017-local-implementation-evidence.md) and
[mutation rows](../reviews/0017-local-mutations.jsonl), output-release
[evidence](../reviews/0017-output-release-evidence.md) and
[mutation rows](../reviews/0017-output-release-mutations.jsonl), and B1/B2
[correction evidence](../reviews/0017-review-blocker-corrections.md) correspond to
identical final implementation/tests here. No new assessment integration assumption
requires a changed policy or API. The main SDK fixture integration is covered by
its unchanged assertions in the normal workspace run. No mutation kills are newly
claimed and historical surviving/advisory mutations remain disclosed.

## Actual verification

All commands below exited 0. No repeat-to-green, failed fixes or ExecutionFailed
recurrence occurred. Nextest ran with the default profile.

| Boundary | Command | Actual result |
| --- | --- | --- |
| N3 | `cargo fmt --all -- --check` | pass |
| N3 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass, 35.11s |
| N3 | `cargo nextest run --locked -p arxd -E 'test(service::executor::tests::)' --no-fail-fast` | 27/27 pass, 199 filtered, run `844b1fe2-4e86-461e-ab95-faddefed99b0`, 0.259s test phase |
| N2 | `cargo fmt --all -- --check` | pass |
| N2 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | pass, 3.94s |
| N2 | `cargo nextest run --locked -p arxd -E 'test(service::executor::tests::)' --no-fail-fast` | 32/32 pass, 200 filtered, run `36670a8c-7246-4807-998d-584f7c213783`, 2.989s test phase |
| Combined | `cargo nextest run --workspace --locked --no-fail-fast` | 231/231 pass, 2 ignored fixtures skipped, run `6e394b68-f8e8-4329-8649-3132ff6d755a`, 18.314s test phase |

Only one final normal full workspace invocation. Ignored subprocess fixtures ran
only through normal parents; no fixture was directly invoked. Tests use owned temp,
synthetic report fakes and normal SDK private random pipe fixtures. Existing normal
suite coverage does not establish native dedicated acceptance.

Additional local checks: git diff whitespace, strict UTF-8/no BOM/LF in port files,
relative Markdown target existence, clean scoped manifests, preserved historical
PROGRESS tail and table rows, source equivalence and SDK one-line-difference checks.
These checks are independent of hosted CI; none provides an agent-review gate.

## Reviews and limits

Accepted corrected architecture and output amendment retain their original Sol
reviews. The [full Opus implementation review](../reviews/0017-implementation-opus-review.md)
initially found B1 missing token/observation/boundary cases and B2 capability rights
regression; fixes a803c36 and the [7758f64 PASS](../reviews/0017-blocker-corrections-opus-recheck.md)
are preserved. These reports review original source heads, not this integrated head.
Open low/advisory coverage notes remain as stated in those reports.

Coordinator Opus high must independently review the combined integration before
publication; each concern requires its own PR and all required hosted gates. No
GitHub publication, push, status, watch, dispatch, settings change, merge-to-main,
System guard test, client endpoint, E1/E2, artifact CI, state-store exit fix, CLI or
native bridge is included. Public publishing remains off. No protected fixed pipe,
SCM/LSA/privilege/account/service/system-path/admin/VM/Lab Manager/S4U/experiment or
private-state.json operation ran as part of this port. No native backend was created
or activated. Native/installed/dedicated M2 remains **unverified on real hardware**.
