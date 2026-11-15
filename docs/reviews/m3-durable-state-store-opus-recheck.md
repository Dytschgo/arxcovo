# M3 durable enrollment state store: Opus bounded delta recheck

Reviewer: Claude Opus 5.5, high effort, 2026-11-15. Target:
`305ca4a1c72a5d5b94228d9e225156c0d8ec490d` on `work/m3-next-enrollment-boundary`, delta
from `2e7e5e1`. The prior full review (`docs/reviews/m3-durable-state-store-opus-review.md`,
`67bf037` → retained `2e7e5e1`) is reused unchanged on every axis. N1–N5 stay non-blocking.

**Verdict: R1 closed. Delta PASS. No new findings.**

## Delta scope

One added 20-line test in `state_store/tests.rs`, plus PROGRESS and evidence text. No production,
`Cargo.toml`, lockfile, workflow or ADR change; `git diff --check` is clean.

## R1 closure

`membership_size_boundary_reaches_constructor_and_decoder` asserts that `MembershipBlob::new`
rejects 0 and 4097 bytes and accepts 4096. It then clones the `rich` image and puts the membership
at `pos(1)`, which is the real membership position below `applied = pos(4)`. With 4096 bytes,
encode and decode return an equal image. With 4097 bytes, the test builds a complete `encode`
output with a valid SHA-256 trailer, asserts it is under `LIMIT` (16384), and requires `Corrupt`.

**It reaches the length gate.** The blob is complete, so `r.bytes(4097)` succeeds and does not
truncate. The size cap and checksum pass by construction. The position and state checks are the
same ones the 4096 round-trip passes. That leaves `MembershipBlob::new` inside `decode` as the
only thing that can reject. Temporary mutants confirm this (each reverted with `git checkout`;
the worktree was clean afterwards):

1. Limit raised from 4096 to 8192 in `MembershipBlob::new`: the new test **fails** at
   `tests.rs:89` (the constructor accepts 4097).
2. The decoder alone bypasses the gate (`MembershipBlob(r.bytes(len)?.to_vec())`), and the
   constructor is unchanged: the new test **fails** at `tests.rs:102`, where `decode` returns
   `Ok(Image { .. })` for the 4097-byte image. So nothing else in `decode` rejects it.
3. With that same decoder-only mutant, the old `canonical_roundtrip_and_semantic_decoder_rejections`
   still **passes**. The new test is what closes the gap.

## Checks run by this reviewer

Windows 11, normal user, build and temp on D:, locked and offline, no installs or deletions:

- `cargo nextest run -p arxd --locked --offline cluster::`: 23 passed, 455 skipped, in 0.747 s
  (run `41ab509d-5a39-445a-a42c-4bab460ed133`). The four-kill probe took 0.622 s.
- `cargo fmt --all --check` passed. `cargo clippy -p arxd --all-targets --locked --offline -- -D
  warnings` passed.

**Not run:** full workspace (8 owner-security access-denied failures unattributed, not fixed,
workspace not claimed green) and hosted Windows/macOS/Ubuntu gates.

## Claims boundary

Local process-kill consistency only: power loss, quorum, signing, CA, mTLS and three-node
behavior are **unverified on real hardware**. M2 stays open (A7 FINAL NO-GO; consumed A12
cleanup uncertain, no retry).

## Eligibility

The different-provider re-check that R1 required is satisfied by this Opus recheck of the
coordinator's fix. Merge still requires green hosted Windows, macOS and Ubuntu fmt, Clippy and
`cluster::` nextest (under 60 s per OS, CI under 15 min), then the coordinator's merge. No
source, PROGRESS or ADR edits, push, PR, comment, status, merge or watch by this reviewer.
