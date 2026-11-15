# M2 executor durable privacy scan diagnosis

Implementation sub-agent: OpenAI GPT-6.1 Sol, high. Local base:
`fafb8d4f4bd460eff51328d781966d2e57f0fa22`. No publication or push.

## Symptom and bounded loop

Reported external evidence: PR106 old head `75d2ef8`, macOS run
43658953897, job 132643749895, failed the raw SQLite/journal byte assertion
in `service::executor::tests::adr0017_dispatch_durable_matrix`, old line
1744, label `windows_admin`. This log has no matched-marker identifier.
No CI artifact was fetched; its historical matched bytes are unknown.
Local `git diff 75d2ef8 -- crates/arxd/src/service/executor.rs
crates/arxd/src/service/executor/tests.rs` was empty before edits.

Read AGENTS, KICKOFF, dytschgo-debug and its active profile, ADR0017, and
the actual test call graph first. Matrix: `good`/`completed` typed fixtures
→ `backend_fixture(OnceReport)` → `dispatch` → synthetic backend `run`
→ `complete`/`assess` → `Activity` SQLite finish/list → drain/drop → raw
database and journal scans. `OnceReport` only consumes a mutex-held report.
No native execution adapter, helper, subprocess execution, or macOS native
API is involved in this test. Normal Cargo build/test tooling was used.

Feedback command, executed before hypotheses:

```text
cargo test -p arxd --offline service::executor::tests::adr0017_dispatch_durable_matrix -- --exact
1 passed; 404 filtered out; test 3.15s
```

This drives the actual reported assertion, but a random fixture cannot be
expected to reproduce on one run. Bound diagnosis to controlled synthetic
fixture changes, rather than repeated probabilistic CI runs.

## Deterministic reproduction and cause

Added closed labels to the six existing marker assertions. Temporarily set
only the `windows_admin` fixture's `local_host_identity.host_id` to 59
ASCII `a`s followed by `62002`, using a TRUNCATE-mode SQLite connection.
The valid host ID is unrelated to any executor report. The exact command
above then failed at the original scan:

```text
windows_admin: persisted synthetic marker uid_b
0 passed; 1 failed; test 1.14s
```

Ranked hypotheses after reproduction: (1) unrelated random host identity
collides with a short marker, (2) dispatcher persists report bytes,
(3) stale unrelated SQLite bytes trigger the scan. Predictions: a collision
exists even before dispatch; changing only host identity removes it; actual
persisted marker bytes remain detectable after the fixture correction.

The minimized regression creates a normal service store, writes a valid
64-character hex host identity containing both `62002` and `63003`, and
confirms both byte matches **before any executor/report call**. `host_id()`
validates and returns this identity. `Activity::open_inner` normally generates
32 random bytes and stores their 64-character hex encoding; both five-digit
markers can occur there. The raw file scan searches every byte without
distinguishing this table from activity/report data.

This proves a concrete false-positive mechanism, not a production sanitizer
leak. It is consistent with the reported CI failure, but cannot establish
which marker/table matched in that historical macOS database. Do not label
the original event conclusively solved or a CI flake from its case-only log.

## Fix and red/green evidence

Only executor tests change. After opening each matrix store, overwrite only
its unrelated host identity with 64 `a`s, before dispatch. Production host
identity generation and all sanitizer/store code remain unchanged. The
existing six raw-byte checks, both database/journal paths, immediate output,
durable metadata, list_activity, and all 81 matrix cases remain covered.
Assertion diagnostics report only a case and closed synthetic-marker label.

Regression `adr0017_durable_privacy_host_id_collision` proves the two host-ID
collisions, then their removal by fixture pinning. It also inserts each of
the six synthetic markers into a separate probe table and confirms pinning
does not conceal those persisted bytes. The probe table permits malformed
and overlong identifiers that the real store's CHECKs reject.

Controlled red/green:

- With the regression's pin call omitted, exact regression fails at the
  byte-absence assertion (`synthetic host ID pinned`, 0.03s).
- Restore the pin: regression passes, including all six intentional marker
  insertions. Initial exact green was 0.07s; final grouped run also passes.
- Restore the original matrix's same forced `windows_admin` host-ID
  collision, now followed by pinning: exact matrix passes (3.16s).
- Remove temporary forced-matrix setup; final eight ADR0017 executor tests
  pass together (3.13s). No temporary mutation/debug instrumentation remains.

Diagnostic harness ledger (not failed production fixes): initial second
SQLite writer used default DELETE journal mode and failed with SQLite
extended error 2570; explicitly matching service TRUNCATE mode reached the
reported assertion. Initial deliberate overlong-marker injection through
`activity.principal` hit its 64-byte CHECK; moved synthetic injections to a
probe table. An initial pin-omission mutation failed first at a redundant
host-ID equality check; removed that check and reran to demonstrate failure
at byte absence itself. One fixture fix, no production fix attempts.

## Validation and handoff

- Windows: `cargo test -p arxd --locked --offline --lib
  service::executor::tests::adr0017_`: 8 passed, 398 filtered out.
- `cargo fmt --all --check` and `git diff --check`: passed.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  passed (31.70s).

No network, publication, privileged/native service, helper, driver, A4,
workflow, Cargo, ADR, policy, or MCP join changes. Scope is executor tests
and this evidence document. Coordinator should carry this provider/effort
and outcome into PROGRESS when integrating; last merged PR is not changed
by this local commit. Independent review and hosted gates remain outstanding.
M2/full F/G/PA6 remain open; hardware paths **unverified on real hardware**.
