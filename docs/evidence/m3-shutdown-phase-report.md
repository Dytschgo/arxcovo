# Shutdown phase report: implementation handoff

2026-11-16. Sol high. Status: blocked at the proposal-test API boundary;
no diagnostic implementation or behavior fix claimed. No publication occurred.

Pinned input: `d1ad3b3ebd557e07218fc8880a5ae4cf487edee7`, based on
main `6e9edd5a51d1df41cdb412af6031f959de2d9d60`. The assigned checkout is
`t3/shutdown-phase-report`; the coordinator branch
`work/m3-shutdown-phase-report` resolves to the same pinned input.
The three allowed source files have no diff against that main base.

## Stop condition

The disposition requires a paused proposal, exclusive node shutdown, then
release and a caller result (success or `Unknown`) within two seconds. The
current API cannot retain that caller across exclusive shutdown:

- `consensus.rs:304`: `propose(&self, ...)` borrows the node through the
  final `rx.await` at line 334. Its spawned owner sends the result at line 331.
- A spawned caller must retain its node `Arc` while awaiting that receiver.
  `three_node.rs:75` requires `Arc::try_unwrap` before calling shutdown;
  line 77 explicitly rejects a caller that still owns the node.
- `consensus.rs:364`: `shutdown(&mut self)` requires exclusive access.
  Using a borrowed caller future instead of an `Arc` does not permit that
  mutable borrow while the future remains alive.
- Cancelling and joining the caller frees the node, as existing U6 does at
  `three_node.rs:491`, but drops its receiver. It cannot then observe the
  required success/`Unknown` result after releasing apply.

This is a source-level ownership finding, not a measured engine hang, failed
runtime test, or red-before observability result. It does not establish that
OpenRaft shutdown itself waits for the proposed gate. No detached task,
unsafe aliasing, changed shutdown signature, or production proposal refactor
was introduced to work around the boundary. The user's explicit API handoff
stop condition takes precedence over finishing a weaker test silently.

The coordinator needs to settle the test contract: either permit explicit
caller cancellation and joined cancellation before shutdown, with post-release
owner/store/permit cleanup assertions, or authorize a narrowly scoped way to
retain the proposal result receiver independently of the node borrow. The
current source allowlist restricts consensus changes to report types and
construction, so this handoff makes neither API change.

## Validation and remaining work

Read AGENTS, KICKOFF, the Opus disposition and the required Sol dependency
correction. Verified the full input SHA, branch pointers, source equality with
main, and the caller/shutdown definitions and existing cancellation path.
No implementation attempt, build, test, fmt, Clippy, U6 repetition, mutation
check, or real hardware exercise ran. The required compilable red-before
mutant, green-after tests, eight sequential unchanged U6 sanity runs, full
cluster tests, fmt, offline workspace Clippy and final source diff review
remain outstanding. The known 49 baseline workspace test failures were not
rerun. Fresh independent Opus review remains required before publication.

Implementation attempt budget consumed: 0 of 2. Behavior-fix budget consumed:
0 of 3. No PROGRESS, ADR, Cargo, workflow, PKI or unrelated marker-lock change;
no push, PR, merge, watch, child agent, VM or admin operation.

The natural hosted U6 cause remains unknown. Opus's induced state-machine
latency demonstrates a compatible sufficient mechanism, not the hosted cause.
This diagnostic is not a fix. A0 production reads remain excluded; read-held
must be logically zero on main, proposal capacity is 64, and R5 integration
belongs to future PR149 after the diagnostic parent merges. Engine join-failure
and engine-deadline outcomes remain uncovered. Hardware paths remain
**unverified on real hardware**.

## Full input Git blob identities

| Input | Git blob |
| --- | --- |
| `consensus.rs` | `d331442ec2d69433d0293bc3e97b48b66ea95ab6` |
| `consensus/adapter.rs` | `fee90a6488b672fcd6a8317c54817817e449a29b` |
| `consensus/three_node.rs` | `9999af2fba30e3b5af9f31a65911e84d1287748b` |
| Opus disposition | `3dc6b42b6a69cbc1377f42c236e42e687541bf47` |
| Required Sol correction | `821992e6c9c2d8d452c1db4bbf8668a79f166254` |
