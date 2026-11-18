# M3 authenticated identity parent-binding recheck

**B3 closed.** The guard now rejects the original line-comment mutation in
each parent independently. B1 and B2 remain closed. The local identity
slice is ready for publication preparation. Hosted review and merge gates
remain outstanding.

Independent single-agent review by Astra 6.2, high, on 2026-11-18.

| Comparison | Commit |
| --- | --- |
| Reviewed head | `2133f3fae4750232d15256168f5b4fa74249c31a` |
| Prior guard recheck target | `e66d298e0290aebe94a027e90fd32c9fb9803cf1` |
| Full production review and D-volume validation base | `092dcf9c729e3e34111fa33dd8bc753b3b98c944` |
| Main comparison base | `e3e79089e6c36f6483cb3db8842e5894f26ee00d` |

## Standards

The worktree was clean at the pinned head. The delta from e66d298e adds two
lines to the Python guard, retains the prior recheck and updates evidence
and progress. The base-to-head whitespace check passed.

I removed the identity-only section from the committed Python blob in
memory. The resulting bytes matched the complete main guard, including
its test entry point. This preserves all 40 original top-level statements
and all 16 original function and class definitions, including the earlier
query inventory and fixture matcher.

**Evidence count advisory.** The D-volume artifact manifest lists 18 log
files. The evidence document also declares two runner hashes for run.ps1
and mutate.ps1 in the parent directory. The supplied count of 18 including
the runners understates the total by two. I independently recomputed all
20 SHA-256 digests against the committed table. Every digest matched.

## Spec

The contract in `docs/briefs/m3-self-admission-kernel.md` calls for both
parent bindings to be pinned. I called the old and new guards on identical
committed Rust source strings. For each mutation I inserted only `// ` at
the start of the required binding and left the other parent untouched.

| Input to assert_identity_inventory | e66d298e | 2133f3fa |
| --- | --- | --- |
| Both parents as committed | Accepted | Accepted |
| Comment first cfg(test) in pki.rs, above mod tls_tests | Accepted | Rejected |
| Comment first cfg(test) in tls_tests.rs, above the child path | Accepted | Rejected |

The new assertion anchors the full escaped multiline binding at column one
and requires exactly one match. After the edit, the first attribute starts
three characters into the line. The raw count still sees the text, but the
anchored match count is zero. The test loop applies that exact edit to each
parent. The module count, path and complete attribute checks still run.

I ran `python -B tools/ci/test_windows_cluster_quarantine.py` once at the
pinned head. All nine tests passed in 0.816 s. The coordinator's separate
nine-test run took 0.127 s. The suite exercises the B1 block-comment and
B2 async-signature controls alongside the new B3 cases. The deliberate
block-comment fence rejects either delimiter in the three guarded files.
The binding check covers their canonical layout.

## Security

I compared the three PKI blobs at a7596271, 092dcf9c, e66d298e and
2133f3fa. Each file was byte-identical across all four commits.

| File under crates/arxd/src/cluster/pki/ | Git blob at reviewed head |
| --- | --- |
| verify.rs | `7f60471a863eb8423316e919c83bc254903c12f9` |
| tls_tests.rs | `8bed50125797725ed4bafc40b3836334ae0353dc` |
| authenticated_identity_tests.rs | `8bfa9a416bb757dce4e43d68b6f60532ede563d1` |

The production conclusion from the full review at 092dcf9c carries forward.
The handle retains the authenticated peer tuple from the verified leaf.
The assumed-founder handle returns Incomplete. Admission requires a separate
decision against authoritative state. The delta changes only how the Python
inventory checks the parent declarations and tests that check.

## Runtime evidence

Sol 6.1 high ran the D-volume validation at 092dcf9c and retained its
evidence in 6b572258. I inspected the retained stdout, Clippy record and
restoration records and compared source and artifact hashes locally.
Sol performed all Cargo executions. This recheck executed only Python and
local source, Git and artifact checks.

| Sol run | Recorded result |
| --- | --- |
| Workspace all-targets Clippy, warnings denied | Exit 0 in 39.227 s |
| Focused TLS baseline | 13 passed, ten existing and three identity tests, zero failed or ignored |
| M1, force the retained cluster | Compiled, one test ran and failed at the designated assertion on line 116 |
| M2, give the assumed founder a synthetic binding | Compiled, one test ran and failed at the designated assertion on line 217 |

The M1 stdout names the assertion "actual first-cluster identity retained
on both handles". M2 names "synthetic founder has no authenticated
identity". Both failures occurred inside the intended test.

Sol recorded exact byte restoration of verify.rs after each mutant. Both
restoration records match the SHA-256 I computed for the reviewed source,
`a65991d2ebe729fd9071ab66e36e760fc88d888f79039df1d0d48b33f4b1f5ba`.
Sol ran no post-restoration test. The last target binary was built from M2.
The earlier C-volume attempt exhausted disk during compilation and ran zero
tests. Runtime credit covers the in-memory TLS and pure registry cases.

## Publication and milestone status

All three local guard blockers are closed. Publication preparation may
proceed. The public feature head still requires its own independent
exact-head review, all five hosted gates green, a feature Windows job of
at most 660 s and fresh green main. Only the coordinator merges.

The supplied main result for e3e79089, CI 44229539357, was green at
648 s wall, 645 s charged and 643 s on Windows. That run predates the
identity tests. Hosted status was outside this local recheck.

M2 remains open and M3 unchecked. Sol 3/3 and historical Opus 0/2 remain
unchanged. Hardware paths are **unverified on real hardware**.

Standards: one artifact-count advisory. Spec: zero open findings.
Security: zero open findings.
