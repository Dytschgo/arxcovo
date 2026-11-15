# M3 OpenRaft 0.10.0 bounded implementation handback

Status: **BLOCKED, no implementation retained**, Sol high, 2026-11-15.
Parent `3cdf92f` (`work/m3-openraft-adapter-boundary`); main baseline
`38750cf1e179e14da6fa871972027b6d66028ec9` (PR #144).
The corrected brief `bc90124` and C1/C2 recheck were read before implementation,
as were AGENTS, KICKOFF, ADR0005, ADR0023 and PROGRESS Ordering.

The explicit instruction to stop on actual C1/C2 API rejection applies. No responder
substitution, type-config override, dependency-feature workaround or weakened store
semantics was adopted. This commit retains only this evidence. A partial draft and
bounded compiler logs remain locally in `D:/Temp/arxcovo-openraft-target`; they are
unreviewed working material, not a ready implementation or acceptance artifact.

## Confirmed blocker: default responder disagrees with C2

Cached primary source: `D:/Temp/arx-openraft-0100/openraft-0.10.0`.
Archive SHA256:
`2628033834a66c7c5bf54c49673a5bbba7192fbcf529314449a0393cb15f7ad0`,
matching the resolved openraft registry package checksum.

`src/raft/mod.rs:258` defaults `Responder<T>` to
`openraft::impls::ProgressResponder<Self, T>`. Decision 2 retains every default:

```rust
openraft::declare_raft_types!(pub Arx: D = Command, R = Applied);
```

C2's owner used the specified construction:

```rust
let (responder, reply) = OneshotResponder::<Arx, _>::new_pair();
let result = match raft.client_write_ff(cmd, Some(responder)).await {
    // completion receiver awaited by the tracked owner
```

Actual `cargo test -p arxd --lib cluster::consensus --locked` compiler output
from the service draft (paths shortened; no diagnostic text changed):

```text
error[E0308]: mismatched types
   --> crates/arxd/src/cluster/consensus.rs:102:60
102 | let result=match raft.client_write_ff(cmd,Some(responder)).await {
    |                                          ---- ^^^^^^^^^ expected `ProgressResponder<Arx, Result<..., ...>>`, found `OneshotResponder<Arx, _>`
    = note: expected struct `ProgressResponder<consensus::Arx, std::result::Result<ClientWriteResponse<consensus::Arx>, ClientWriteError<consensus::Arx>>>`
               found struct `OneshotResponder<consensus::Arx, _>`
error: could not compile `arxd` (lib test) due to 1 previous error
```

Primary source also exposes `ProgressResponder::complete_only()` in
`src/raft/responder/impls/progress_responder.rs:95`; the macro supports an explicit
`Responder<T>` override. These are possible decisions for Opus to assess, **neither
was implemented or compiler-verified here**. Preserve cancellation-independent
JoinSet ownership, 64 engine-held permits and the required shutdown/U6 proof in
whatever corrected contract is selected.

C1's plain non-async append implementation, with Completion constructed as its
first statement, did compile in `cargo check -p arxd --locked` after correcting
ordinary type aliases. No U4 runtime cancellation tests ran. This is API agreement
only, not callback correctness evidence. That adapter draft is not retained in Git.

## Confirmed blocker: existing lock entry cannot remain byte-identical

Requested dependency settings were used exactly: openraft `=0.10.0`,
default-features false, only `tokio-rt`; futures-util `=0.3.34`; tokio `sync` added
on existing `=1.48.0`. Cargo resolved **35 added packages**, with no version,
checksum or source replacement of any existing package. Exact package-block
comparison nevertheless found two changed existing blocks:

- `arxd 0.1.0`: necessary new direct dependency edges `futures-util`, `openraft`.
- `chrono 0.4.45`: added dependency edges `js-sys` and `wasm-bindgen`.

Draft lock diff: 334 additions, zero deletions. Additive lines are **not** the same
as byte-identical existing entries. `cargo tree -e features -i chrono --locked`
shows OpenRaft/display-more/openraft-macros enabling chrono default features,
including `wasmbind`, which enables those edges. Both packages already exist in
the baseline lock; their versions did not change.

Removing only those two new chrono edges from the resolved draft lock and running
`cargo check -p arxd --locked` produced:

```text
error: cannot update the lock file <workspace>/Cargo.lock because --locked was passed to prevent this
help: to generate the lock file without accessing the network, remove the --locked flag and use --offline instead.
```

No manual incomplete lock, patch dependency, different version or feature
substitution was retained. Opus/coordinator must resolve the byte-identical
restriction before accepting a dependency commit.

Actual `cargo tree -e features -i openraft --locked` (workspace path omitted):

```text
openraft v0.10.0
└── openraft feature "tokio-rt"
    └── arxd v0.1.0 (<workspace>/crates/arxd)
        └── arxd feature "default" (command-line)
```

No openraft serde/clap/bt/single-threaded/type-alias feature was enabled. Transitive
crates can still use serde; this tree is not a claim of a serde-free dependency graph.
Full local tree outputs: `openraft-tree.txt`, `openraft-feature-tree.txt`,
`chrono-features.txt` in the dedicated D: directory.

## Actual checks and corrections

All Cargo runs used TEMP, TMP and CARGO_TARGET_DIR set to
`D:/Temp/arxcovo-openraft-target`. No installs or cache deletion occurred.

The temporary partial draft comprised dependencies, default Arx/Applied types,
position/vote/entry mappings, bounded canonical membership codec, static Command
Display and the named Membership error. Four tests exercised mapping/vote order,
normal command mapping/static Display, maximal 3605-byte joint membership,
every nonempty truncation, default node roundtrip and malformed/bounded inputs.
It was removed from the worktree after preserving the local draft; the following
passes apply **only to that partial draft**, not a real-engine service:

| Run | Actual result |
| --- | --- |
| `cargo test -p arxd --lib cluster::consensus --locked` | 4 passed, test runtime 0.04 s; compile 8.96 s |
| `cargo nextest run -p arxd --locked cluster::` | 40 passed, 456 skipped; 1.492 s test wall time; compile 25.70 s |
| Existing cluster coverage within that run | All original 36 passed, including both four-process-kill tests (eight probes total) |
| `cargo fmt --check` | Passed |
| `cargo clippy -p arxd --all-targets --locked -- -D warnings` | Passed, 24.24 s |
| `git diff --check` | Passed before cleanup; rerun on final evidence |

Initial draft errors were ordinary incorrect generic type spellings, dereferencing
MutexGuard and import paths; those were corrected before the adapter compiled.
A codec test incorrectly expected BasicNode::default to be empty, causing two
observed failed runs (one earlier queued draft and the expanded test). Actual
`src/node.rs` sets it to `localhost`; the corrected roundtrip passes. The earlier
commentary suggesting Suite/address incompatibility is withdrawn. No official
Suite case was moved or reclassified on that mistaken assumption.

The C2 E0308 failure above was **not fixed or retried with a substitute**. The
partial passing draft excludes the rejected service. No broad Windows owner
fixture suite was repeated, and no attribution is made for historical access-denied
failures. Hosted checks and independent Opus review have not run on this handback.

## Resume boundary and claims

Only the coordinator should update PROGRESS with this Sol/high/blocked outcome;
this subtask's exact-file restriction excluded PROGRESS. Last merged PR remains
#144. No publication or merge was attempted.

After Opus corrects the responder/lock constraints, resume the prescribed small
implementation commits. Adapter U4/U5, Params/U3, StorageHelper R1-R4, tracked
service/U6, mem_net and mandatory T1/T2 remain unimplemented/unverified. S1 (27)
and S2 (19 plus transfer_snapshot) are the brief's **proposed, wholly unrun**
partition; there is no Suite conformance claim. T3 snapshot catch-up is also unrun,
not an accepted deferred fourth commit. This handback has no election, quorum,
ReadIndex, enrollment-from-commit, restart-convergence, byte-counter or shutdown
proof. No tasks, stores, listeners or Raft engines remain running from the draft.

Fresh independent Opus review is required before any later publication. No
subagents, pushes, PRs, watches, native M2 work, admin steps, VMs, Lab Manager,
dispatches, variables, CA or signing actions occurred. M2 stays open: A7 FINAL
NO-GO and A12 consumed/cleanup-unconfirmed, with no retry. Transport, real hosts,
power loss, physical expiry and authentication remain **unverified on real hardware**.
