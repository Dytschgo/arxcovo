# Windows fixture restoration merge checkpoint

Owner explicitly answered "yes authorized" to the root coordinator's pinned
PR164 one-use budget-only protection exception. Approved head:
`60f30c8673e9c9653d1a177d79379a1975f3107f`, base `c612152`.

PR CI43987282029 attempt1 succeeded on all three platforms. Ubuntu/macOS logs
show PASS for all24fixturetests each. Independent Opus exact-head PASS6933714689
and agent-review59090456374 were freshly verified. No armed auto-merges existed
before the protection window or immediately before the merge.

Only ci-time-budget was removed from required_status_checks. strict stayed true;
three verify checks stayed bound to App15368; agent-review remained any-app.
The pinned squash merge produced `39b3dd73052abd64c1c3d3042e62f080469f1597`
at2026-11-17T08:58:52Z. Its tree `e25a79893e5f07b40cd96bbde12ec8d82821e934`
equals the reviewed head's tree exactly.

In finally, the original five contexts and bindings were restored (Actions15368,
agent-review explicit-1). Readback returned the original null app binding for
agent-review. strict, admin enforcement, linear history, force/deletion fences
and every other protection field match the full before snapshot exactly.
Local action records: D:/Temp/arx-pr164-final-gates/{authorized-action,
protection-before,protection-after,merge-result}.json. No manual success status,
ci-speed label, admin bypass, other merge, or ADR0025 activation occurred.

First exact-main CI44051366327 attempt1 is running; feature merges remain held.
Windows fixture coverage is unverified on hosted Windows and **unverified on real
hardware**. The bounded eight-test diagnostic sample is not acceptance evidence.
M2 open, M3 unchecked; Sol3/3, Opus0/2 unchanged. This exception is consumed.
