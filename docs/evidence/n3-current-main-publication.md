# N3 late-dispatch fix on main after E1

Coordinator: OpenAI / gpt-6.1-sol / medium, 2026-11-12.
Base: `884a301eaa1e6dda6d218e439315ecdf3902ae82` (PR40).
Port source: `1976afb1a2f3e7d2b22cbf819909a0a22575be24`.

Both executor source/test files are byte-identical to the port source.
No other product file changes. E1 owner modules, the client correction,
SDK cancellation fixture, Cargo, workflows, scripts and tools are inherited
unchanged from main. The historical port evidence and two independent Opus
reports are retained unchanged. Their tests and mutations belong to their
named revisions; no fresh mutation pass is claimed here.

The stopping refusal is completed synchronously while holding the worker-set
lock. It registers no worker after drain takes the set. Quarantine still wins
over stopping, and the accepted-worker completion path remains shared.

Actual coordinator checks at this integrated source, all exit 0:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo nextest run --workspace --locked --no-fail-fast`: **235 passed,
  2 skipped**, default profile; run
  `1e0fe360-7201-4651-9917-024dbc4d5058`, 10.330s test phase.
- Git byte comparisons for all five extracted files and `git diff --check`.

The two skipped subprocess fixtures were exercised through their normal
parents, never invoked directly. No source instrumentation or test mutation
was applied. ExecutionFailed did not recur in this run; it remains unresolved.
This run is not a repair or waiver of E2's distinct red run bdbf032b.

Before: the original independent review demonstrates the late-registration
race and records regression mutations. After: this source retains the fix,
priority correction and their tests with a green combined local run. Fresh
Opus integration review and all hosted required checks remain mandatory.
Reverting this isolated source patch is possible, but reopens the drain race.

No backend activation, new report contract, schema change, owner CLI,
privileged operation or experiment retry. Native, installed and dedicated
M2 paths remain **unverified on real hardware**; M2 is incomplete.
