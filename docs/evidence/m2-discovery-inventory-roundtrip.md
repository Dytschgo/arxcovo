# M2 discovery to inventory roundtrip

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-14. Isolated branch
`t3/mcp-discovery-cache-roundtrip`, base `4ff5909`. Initial cwd and Git top
matched the assigned worktree; initial status was clean. Scope is `mcp.rs`, its
adjacent test module and this evidence. No other worker files were edited.

## Reproduced defect and API change

An initialized rmcp client calls `discover_tools {}` and then `list_hosts {}`
over an in-memory duplex transport. The existing injected dispatcher fixture
releases a successful typed discovery report into a temporary activity store.
Before the fix, inventory returns the released tools, identity and timestamp,
but still says `tool_discovery: "unavailable"`.

`tool_discovery: "cached"` now means a prior successfully released discovery
observation is available in `tools`, `tools_observed_at` and
`last_observed_identity`. It does **not** promise current execution, readiness,
activation or successful future discovery. `execution`, `execution_available`
and `execution_unavailable_reason` retain their existing meanings and values.
The `list_hosts` tool description explains the distinction. No new field is
added. The no-cache rendering block is unchanged, including its legacy values.

The existing `Activity::discovery_observation` and `cache_discovery` APIs and
dedicated response-release point are unchanged. Tickets order observation starts,
not completion times; older or equal tickets cannot replace a newer cache.

## Development producer gap (unresolved within file ownership)

Source inspection of `development::Execution::work` shows that development
discovery returns `{execution_mode, claim, tools}`, without `outcome` or an
observed identity. `Activity::cache_discovery` only accepts the released
`outcome: "succeeded"` contract. Passing the current response directly is a no-op.
The MCP development branch currently does not reserve an observation or cache it.

Adding a success marker solely because `dispatch` returns `Ok` is unsafe:
development cancellation can return `Ok(null)` with failed activity, and its tool
loop can return a nominally successful table with `found: true, version: null`
after a non-exited probe. MCP has no per-probe terminal/completeness facts to
distinguish a fully successful discovery from those partial results. It must not
invent an identity or reinterpret this as the dedicated released contract.

The precise remaining interface need is a development-producer indication of a
fully completed, cache-eligible discovery, separate from failed/cancelled/partial
results. This belongs in the development executor, outside this worker's exclusive
ownership. Existing store APIs suffice once that producer contract is settled;
no store change is requested. Development caching remains unresolved, and this
commit does not claim the entire requested feature is complete.

## Local evidence

- Red, before the production fix: `cargo test --locked --offline -p arxd --lib
  mcp::discovery_tests -- --nocapture`, exit 1; one intended assertion failure:
  inventory `tool_discovery` was `"unavailable"`, expected `"cached"`.
  Initial fixture compilation errors were corrected before this red run.
- Green, final focused mock test: same command, exit 0; 1 passed, 0 failed,
  376 filtered out, 0 ignored. It exercises actual MCP discovery/inventory;
  checks exact released tools, identity and timestamp; preserves current
  unavailable execution; rejects older/equal cache writers and the legacy
  development response shape; retains cache after failed, refused and partial
  discoveries; and checks command/discovery refusal after quarantine/shutdown.
  No development execution or native process was run.
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --locked --offline -p arxd --lib --tests -- -D warnings`: exit 0.
- Direct newline-normalized comparison of the unchanged pre-cache inventory
  source block against `e7-no-cache-golden.txt`: exact match. The Unix-only E7
  test itself was not run on Windows.

No installed service, owner state, account provisioning, real tool launch, VM,
privileged experiment, network, GitHub action or CI dispatch was used. Required
hosted gates and fresh independent Grok/Muse review remain for the coordinator
before publication. M2 remains incomplete, PA6 Linux F/G OPEN; hardware paths are
**unverified on real hardware**. Optional Windows harness and launchd profile
evidence tracks remain parked. PROGRESS and ADR files were left to their owners.
