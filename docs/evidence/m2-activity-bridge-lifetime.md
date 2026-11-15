# M2 activity integration: test-owned bridge lifetime

Provider: GPT-6.1 Sol, high. Local Windows, 2026-11-15.
Base: `1869175ba31e5c81386f45d293ed50365081fb1f` (clean at start).
Reported event: PR109 Windows run `43662502017`, job `132654674576`,
reported head `a746ab4`; no external logs fetched or historical replay claimed.

## Audited path and bounded feedback

Read AGENTS.md, KICKOFF, dytschgo-debug and its active profile, ADR0008 and
ADR0011. The fixture has no execution configuration. Requests use `list_activity`
and synthetic rejected arguments; no actual command execution is reached.
HTTP dispatch and the local connection's rmcp handler both reach
`Inventory::call_tool`: commit start, query caller-scoped rows, finish, serialize.
`Activity::finish_typed` updates in a transaction; injected UPDATE failure rolls
back and maps to the fixed MCP internal error `activity unavailable`, withholding
the page. The local response pump and bridge forward complete bounded frames.
The subprocess entry is only `arxd mcp-stdio --client-config` against a private
synthetic fixture and local listener.

Exact feedback command (also used for the controlled red and green):

```powershell
cargo test --locked --offline -p arxd --test inventory activity_v2::sdk_http_and_real_stdio_activity_visibility_storage_failures_and_bounds -- --exact --nocapture
```

Initial unchanged-source run used the same command without `--locked`: PASS,
1 test, 2.54 seconds after compilation. Historical failure did not reproduce
spontaneously. Before testing a cause, established this real SDK/SQLite/subprocess
loop with the original error/no-page/no-marker assertions. Then ranked predictions:
(1) helper lifetime expiry kills a live bridge, (2) error-frame forwarding loses
the response, (3) SQLite completion failure independently prevents delivery.

## Controlled red and green

Added one deterministic scheduling intervention after installing `fail_finish`:
on stdio only, leave the live session idle for 18 seconds. This crosses the
helper's 15-second child wait plus two-second diagnostic deadline. Bound the
following SDK error response to five seconds, preserving the original assertions.
No retry loop, load flood, timeout increase or relaxed assertion.

With the original helper: FAIL, 1 test, 19.23 seconds. Exact closed diagnostics:
`bridge completion status=Err(Elapsed(())); diagnostic=diagnostic-timeout`.
The original `activity unavailable`/no-activities/no-marker assertion failed.
This reproduces the reported diagnostic and symptom under controlled scheduling;
it does not establish what caused the historical CI scheduling duration.

Proven mechanism: `stdio_peer` detached a task whose 15-second child wait began
at initialization, while the SDK peer was still live. Its diagnostic reader then
waited on stderr of that live child. On return, dropping the kill-on-drop child
killed the bridge independently of SDK cancellation.

Fix: `InventoryPeer` owns the SDK service and optional subprocess for their joint
lifetime. Explicit cancellation closes the SDK first, then starts the unchanged
15-second child shutdown bound. A failed shutdown kills/reaps before reading the
existing bounded, category-only diagnostics and fails the test. Panic/drop keeps
the subprocess kill-on-drop protection. HTTP peers use the same wrapper without
a child. No production changes.

With the fix and the identical controlled regression: PASS, 1 test, 19.21 seconds.
The original pagination, maximum-field response bounds, validation, fail-start,
fail-finish, no-page/no-marker, unfinished-row and persisted-marker assertions
all pass; child successful exit is now awaited and asserted at cancellation.
The committed regression adds an intentional 18-second idle period to this test.

Implementation ledger: one behavioral fix. First build caught a misplaced
`unwrap` in the wrapper construction (no executable result); corrected that
compile typo and formatting before the green run. No failed behavioral fix series.

## Additional checks and pins

- `cargo test --locked --offline -p arxd --test inventory activity_v2::read_flood_preserves_actions_and_lifecycle -- --exact`: PASS, 1 test, 6.10 seconds. Store-only fixture.
- `cargo test --locked --offline -p arxd --test inventory activity_v2::exactly_once_completion_rolls_back_duplicate_missing_and_repeated -- --exact`: PASS, 1 test, 0.07 seconds. Store-only fixture.
- `cargo fmt --all -- --check`: PASS after formatting.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`: PASS, 30.76 seconds.
- `git diff --check`: PASS.

Git blob pins (LF canonical source):

- `crates/arxd/tests/inventory.rs`: `e92ef2669e6777cede83859d383190c59aef421d`.
- `crates/arxd/tests/inventory/activity_v2.rs`: `40a3c40fd8879a415b0aade734cb7f2ca8312bca`.

Only the two test files, this evidence and PROGRESS change. No native helper,
backend, service, admin, external network, workflow, driver, policy, Cargo or ADR
operations/changes; no publication, push, dispatch or subagents. Last merged PR
unchanged by this task. Independent other-provider review and hosted required
checks remain for the coordinator; no merge or PR109-unblocked claim. Historical
event cause remains unknown beyond the reproduced matching failure mechanism.
M2/F/G/PA6 remain open; hardware paths **unverified on real hardware**.
