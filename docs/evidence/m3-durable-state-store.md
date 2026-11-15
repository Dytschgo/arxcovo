# M3 durable enrollment state-store probe

Implemented by GPT-6.1 Sol, high effort, 2026-11-15, from coordinator base
`bf6b6f7` (main `4f323c36e2d134b56c1f6efb3035b6cc18143c84`, PR #142).
Fresh independent Opus review and hosted Windows/macOS/Ubuntu gates remain pending.
This is local process-kill consistency evidence, not M3 acceptance.

## Scope and contract

Only the brief's production paths changed: `cluster.rs`, `enrollment.rs`,
`enrollment/code.rs`, new `enrollment/codec.rs`, `enrollment/tests.rs`, new
`state_store.rs` and `state_store/tests.rs`. This is the sole evidence document.
No dependency, lockfile, workflow, PROGRESS or ADR changes.

`Command::redeem` reduces the presented bearer proof to a verifier before proposal
or forwarding. No proof is retained by the command or durable state. Future ingress
must accept proofs only, never client verifiers. The constant-time apply comparison
is a consistency check under trusted-member authority; it is not authentication or
proof of quorum. Fresh-mint proposal/retry constraints are documented on `Mint`;
there is no transport or ingress implementation in this slice.

The store holds a private exclusive lock, checks existing file and sidecar paths,
uses SQLite TRUNCATE/FULL/fullfsync with 250 ms busy timeout, and reads back the
settings and 64-page/4096-byte bounds. Schema creation, initial blob and version 1
are atomic. Snapshots contain complete enrollment state, applied position and
membership with a SHA-256 corruption checksum, not authentication. The bounded
canonical decoder checks sorted/unique records, owner/request and consumed-node
uniqueness, expiry/retention/pending limits, and membership position consistency.

Apply requires index 0 followed by exact consecutive indexes and nondecreasing
terms. Rejections still advance the durable position. Both apply and install write
and commit a clone before swapping memory; SQL errors poison the store while
preserving its pre-operation memory. Install requires a higher applied index and
nondecreasing term and refuses corrupt, foreign, oversized or unapplied images.

## Actual local verification

Windows normal-user process fixtures; all Cargo build output used
`D:/Code/Arxcovo/target`, TEMP and TMP used `D:/Temp`. No installs or cache deletion.

- Boundary-only `cargo test -p arxd --locked --offline cluster::enrollment`:
  14 passed, including A1 wrong proof on expired unconsumed code → BadProof.
- `cargo fmt --all --check`: passed.
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`: passed.
- `cargo nextest run -p arxd --locked --offline cluster::`: 22 passed; child-only
  crash fixture excluded from ordinary selection and explicitly run by its parent.
  Final complete run: 0.797 s overall, four-kill parent test 0.667 s.
- `cargo nextest run --workspace --locked --offline`: failed outside this slice.
  352 passed, 8 failed, 4 skipped; fail-fast left 115 unrun, 10.753 s total.
  All eight failures were unchanged `service::owner_windows::tests` fixtures:
  create_new_race_and_staged_source_substitution_refuse, n1, n2, both n3, n4, n5,
  n6. Each failed at `owner_windows_tests.rs:49`, SetNamedSecurityInfoW result 5
  (access denied) rather than 0 while preparing a test-owned temporary fixture.
  No baseline run was performed; these failures are not claimed baseline-green
  or fixed. No owner/account/admin remediation was attempted.

Decoder tests mutate semantic fields and recompute checksums, truncate at every
length, and exercise a maximal 64-consumption plus 4096-byte membership image.
Other coverage includes fail-closed open/order/lock/sidecars, poisoned SQL writes,
reopen clock/dedup/pruning/node-counter boundaries, invalid installs with unchanged
memory and DB blob, and byte-identical replica snapshots.

Privacy fixtures deliberately separate synthetic code/actor/request metadata from
the secret marker; randomness cannot accidentally duplicate marker metadata.
Every raw/hex marker is first planted into an owned control file and positively
detected, then removed. DB, journal, lock, snapshots, command/store Debug and static
error output are scanned for secret, proof and exposed transfer text, raw and hex.
Windows byte-range locks require the ordinary full-directory scan after closing
stores; crash scans run after killing the child and before recovery, including the
hot journal. Scans passed.

## Four owned-child crash outcomes

The parent launches only the current test executable, with no external commands or
accounts, and holds KillOnDrop ownership. Every child has a 10-second deadline;
the cluster test budget is 60 seconds. Each child applies indexes 0 and 1 itself
before reaching the consuming redeem or install hook. Hooks are compiled only for tests.

| Operation | Kill boundary | Observed recovery |
| --- | --- | --- |
| Consuming redeem at index 2 | After cache_flush, before commit | Exact pre-reference at index 1; index 2 applies once with reference outcome |
| Consuming redeem at index 2 | After commit, before memory swap/return | Exact post-reference at index 2; duplicate index 2 rejected |
| Snapshot install | After cache_flush, before commit | Exact old reference at index 1; next index 2 applies once |
| Snapshot install | After commit, before memory swap/return | Exact installed reference at index 2; duplicate index 2 rejected |

Both pre-commit cases asserted rollback-journal magic `d9 d5 05 f9 20 a1 63 d7`
and a nonzero page-record count before the parent killed the child. Each recovery
then accepted index 3 exactly once. Reference states were independently replayed.

## Limits and handoff

SQLite remains provisional until all three hosted OS gates pass, including genuine
hot-journal probes and clean marker scans. Linux/macOS execution and hosted Windows
execution are pending; local Windows evidence does not substitute for them.
Power-loss durability, quorum commit, three-node availability, physical expiry,
OpenRaft adapter/log/vote storage, signing, CA and mTLS are not implemented/proven
here and are **unverified on real hardware**. SHA-256 does not authenticate a
snapshot. No M2 retry or grant follows: A7 FINAL NO-GO and consumed A12 D7l cleanup
unconfirmed/no-retry remain unchanged. No host admin, VM, old Lab Manager, network
endpoint, publication, push, PR, merge or watch action was performed by this task.
