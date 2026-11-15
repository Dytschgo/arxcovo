# A7/A11 fixture phase and budget correction proposal

2026-11-15, Sol coordinator. Documentation only, independent review pending.
[Blocker](../evidence/linux-a7-a11-integration-source.md) is accepted:
30s packaged startup, four pre-READY calls, 50s tool-probe budget and20s
RunSpec are incompatible as guaranteed acceptance budgets. No source fixed yet.

Proposed smallest bounded fixture scheduling change:

1. Complete fixed handoff/package/profile/view/account admission and one
   production setup before READY. Keep the packaged30s TimeoutStartSec and
   every packaged hardening byte unchanged. READY means setup is ready; it
   does not mean probes or acceptance passed. Send READY, then complete the
   existing MainPID/exe/supervisor identity-ack before starting any Runner call.
2. Start exactly two scoped case workers, one owned account/Runner domain each.
   Each case runs production arxd tool-probe then the fixed E4 reentry, in
   that order, with per-call artifact/view/account checks unchanged. Parallel
   account cases avoid summing unrelated sequential budgets; no generic pool.
3. Fixture-only tool-probe RunSpec55s covers the existing50s probe allowance
   plus5s scheduling margin; fixed E4 reentry RunSpec10s covers its5s rights
   probe plus5s overhead. Output4096, memory64MiB/swap0/pids32 remain per case.
   Admission has at most three5s property queries and Runner cleanup retains
   its existing bounded allowance. Reviewer must check this arithmetic against
   actual cleanup paths: target per-case<=90s, identity-ack<=5s and final join/
   completion margin<=10s, inside unchanged120s monitor and140s RuntimeMaxSec.
   Unsupported/slower admission refuses; no timeout retry or product change.
4. Existing monitor starts after blocking systemctl start returns READY;
   verify this placement in source. Keep packaged startup and post-READY
   acceptance clocks distinct. Do not add a startup override or global timeout
   increase. No other fixture inherits the A7/A11 RunSpec values or workers.
5. On worker failure/panic, cancel owned caller/shutdown tokens and join both
   workers before controller exit; keep all actual process-tree cleanup and
   residue checks. Never accept an unjoined task, early READY alone, missing
   case, success from one case, or a partial output. Only both complete case
   oracles plus joined cleanup can reach the fixed completion marker and
   successful exact libtest summary. Driver final unit stop/cgroup/UID cleanup
   remains required. Already-received identity ack is not acceptance credit.

This changes only test phase ordering and the derived fixture RunSpec budget;
production activation, Runner implementation, tool-probe deadlines, package,
identity/NNP/descriptor/cgroup limits and old native grants are unchanged.
Expected source ceiling: ignored controller, fixed driver oracle/mapping mocks,
pure scheduling/cancellation counterfactuals and delivery contract. A new native
disposition still names both cases/source/workflow/bounds and fresh host.
No implementation/execution authority until the correction is independently
accepted, documented through normal gates and merged. Alternative serial
budget increases or copied/shortened probes are not proposed here. A7/A11/M2/
full F/G/PA6 remain open and **unverified on real hardware**.
