# M2 development discovery cache completion

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-14. Branch
`t3/m2-discovery-completion-cache`, base `6f3d0c2` (includes retained `86917c4`).
Initial cwd matched Git top in the assigned isolated worktree; status was clean.
Only the four assigned Rust files and this evidence were changed. No subagents.

## Before and after

Development discovery returned an unmarked tools table and audited it succeeded
although a probe could time out. Cancellation could return `Ok(null)` with a
failed audit. MCP could not safely infer completion from dispatch `Ok`.

The producer now computes completion from the entire fixed tool table. Every
probe must either exit zero with confirmed cleanup and current-user match, or
return the existing `Reason::Unavailable` (tool resolution unavailable). Other
probe errors remain errors. Nonterminal/uncertain results have `found: null`,
not false; their table and audit outcome are failed. Nonzero exits also prevent
cache eligibility. Cancellation, including during the final probe, retains the
existing null/failed behavior. The native resolver and execution paths are
unchanged; no new interpretation of arbitrary probe errors as absence is added.

The minimal response addition is `outcome: succeeded|failed` on discovery tables,
computed beside the typed audit outcome in the producer. This reuses the existing
Value dispatch interface and store cache contract without another dispatch API.
It is withheld if audit completion fails. Command response shapes are unchanged.
No dedicated observed identity is manufactured: development mode and claim stay
present, and cached `last_observed_identity` is null.

MCP reserves the discovery ticket before starting work and caches only a released,
audited, eligible result. `Ok` alone is insufficient. Existing store ordering
rejects older/equal cache writers. Failed, partial, cancelled, refused and
storage-failed requests retain the previous observation. The retained
`tool_discovery: cached` rendering is unchanged, as are current execution
availability and the complete no-cache rendering block. No executor activation,
fallback, extra account, owner approval flow or store API change was added.

## Verification actually run

- Regression red: `cargo test --locked --offline -p arxd --lib
  development::tests::discovery_completion -- --nocapture`, exit 1:
  timeout produced `Succeeded`, expected `Failed`.
- Regression red: `cargo test --locked --offline -p arxd --lib
  mcp::discovery_tests::development_producer_roundtrip -- --nocapture`, exit 1:
  producer outcome was null, expected succeeded. An earlier fixture setup run
  selected the unavailable dedicated executor; corrected to foreground Activity
  before recording this regression.
- Final `cargo test --locked --offline -p arxd --lib development::tests --
  --nocapture`: exit 0, 9 passed, 0 failed, 0 ignored, 371 filtered out.
- Final `cargo test --locked --offline -p arxd --lib mcp::discovery_tests --
  --nocapture`: exit 0, 3 passed, 0 failed, 0 ignored, 377 filtered out.
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --locked --offline -p arxd --lib --tests -- -D warnings`: exit 0.
- `git diff --check`: exit 0. Newline-normalized source comparison against
  `e7-no-cache-golden.txt`: exact match. Unix E7 runtime test was not run.

The test-only probe seam injects terminal results into the real producer loop;
no test invents the development completion field. Real initialized rmcp clients
use in-memory duplex transport and a temporary Activity store. Tests cover
complete mixed found/missing inventory through list_hosts, all-known-missing
completion, timeout/output-limit/shutdown/cancelled probes, uncertain identity or
cleanup, nonzero exit, probe errors, cancellation and real SQLite audit UPDATE
failure. A bounded channel holds an older producer open while a newer discovery
completes; the older release and an equal-ticket writer cannot replace it.
The ordering test also proves the MCP ticket precedes the first probe, and no
cache exists while that producer is held. Existing dedicated identity/refusal
coverage remains passing; development inventory retains null observed identity
and cached status after shutdown while current execution becomes unavailable.

No real tool/process/native probe, installed service, private owner state,
privilege, network, VM, GitHub or CI dispatch was exercised. Hardware paths are
**unverified on real hardware**. M2 remains incomplete; PA6 / Linux F/G OPEN.
Optional Windows harness and launchd profile evidence tracks remain parked.
Fresh combined Grok/Muse review and normal required gates remain for the
coordinator before publication; this implementation was neither published nor
merged. PROGRESS, ADRs and other worker files were left to their owners.
