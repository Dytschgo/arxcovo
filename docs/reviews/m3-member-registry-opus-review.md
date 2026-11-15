# Opus review: Sol member registry implementation

Disposition: **REQUEST CHANGES (narrow, test-only).** The source is correct and matches the settled brief on every axis I checked. Three
decode/validation rejections that the brief requires and the evidence claims as exercised have no isolating test: weakening all three
together leaves the full cluster suite green. Add the three cases in R1 below and the slice is merge-eligible for the Opus side, pending
hosted required checks.

Anthropic / claude-opus-5-5, independent review sub-agent, 2026-11-16. Single reviewer. Opus wrote the brief architecture (`2278908` →
`2640629`), so this report gives **no independent credit to the design**. Its independent review is Sol's
`docs/reviews/m3-member-registry-slice-sol-review.md`. This report is an independent different-provider review of Sol's
**implementation** only.

## Exact pins

| Item | Identity |
| --- | --- |
| Review HEAD (clean worktree, branch `t3/member-registry-review`) | `e617200c4e8dad616e9c1b386bdd71e7b6fa8cf0` |
| main reference (PR146), merge base of HEAD | `0ab5038d89e86e5f726fbed7e5ef6d76e64fa306` |
| Settled contract | brief retained `2640629` plus coordinator clarifications `4750ae1` |
| Source/test commit | `71b32d9`. Its tree equals builder `a8118e96d5920fde6fa35e660f6265650fd2ebe2` (empty `git diff`) |
| Evidence commit | `400f901`. Its tree equals builder `38d7d96630e7a64a4c4249d82b027c0a1d9a5e64` (empty `git diff`) |
| Ten-path manifest | All ten blob IDs in `docs/evidence/m3-member-registry.md` equal `git ls-tree HEAD`. `enrollment/code.rs` is unchanged. `71b32d9..HEAD` touches no source |
| Out-of-scope paths | `git diff 0ab5038..HEAD` touches no `Cargo.toml`, `Cargo.lock` or `.github`. No adapter, `propose`, `append` or OpenRaft wiring changed |

I read AGENTS.md, ADR 0005 (including the 2026-11-15 amendment), ADR 0023, the 155-line brief, Sol's review and R1/R2 recheck, the
evidence file, and the full `0ab5038..HEAD` source and test diff for all ten paths, plus the unchanged surrounding code in
`enrollment.rs` `apply`, `codec.rs` `decode`/`Reader::flag`, `state_store.rs` `decode`, and `log_store.rs` `decode_row`.

## Standards

**CONDITIONAL. One required test-only fix (R1). Source style, scope and size pass.**

- Scope is exactly the ten authorized Rust paths plus evidence. `registry.rs` has 94 lines (cap 150). No signature changes to
  `encode_command`, `decode_command`, `encode_row`, `decode_row`, `append`, `propose` or the adapter. No new dependencies.
- `cargo fmt --all -- --check`, workspace Clippy `-D warnings` and `git diff --check 0ab5038 HEAD` all pass (my runs, below).
- Fixture changes preserve the prior coverage. I checked each one, and no assertion was weakened:
  - **Enrollment tests.** `state()` founds node 1 at time 0, so `committed_ms` is unchanged. The retained-cap and pending-cap fixtures
    switch to distinct keys and add Removes only to stay under 16 members. The `StateFull`, `only_time_changed`, first-code
    `replay:true` at t=67, `RequestConflict`, `CodeIdCollision`, prune-boundary and `next_node_id == 74` assertions are byte-for-byte
    unchanged.
  - **State-store tests.** `pos(i)` shifts by one so Found sits at index 0. The version corruption moves from 2 to 3 because 2 is now
    valid, and v1 is covered separately. The membership-index corruptions shift with the applied index (index > applied, and
    same-index different term). The pending-cap test gains an 8-record positive control, so the 9-record rejection is now
    attributable. That is stronger than before.
  - **Log tests.** Found is prepended. The crash split, truncate and resume indices shift together. Both four-kill groups keep their
    hot-journal, exact pre/post, replay and next-index assertions. The tag-corruption probe moves from 4 to 6 because 4 is now valid.
  - **Privacy scan** now applies Found, Mint and two Redeems and scans Debug output and files.
- **R1 (required, test-only): three specified rejections are unproven.**
  - **Counterfactual.** I changed `found()` to accept `1..=MAX_FOUND_WIRE` instead of `1..=MAX_MEMBERS`, dropped the member
    `node >= next_node_id` check in `codec::decode`, and dropped its member duplicate-key check, all together. The full cluster suite
    still passed: **107/107**.
  - **Why each one goes unnoticed today.** Every 17- and 64-pair Found in the pure tests and in T4 also has nodes `>= next_node_id`, so
    the range check masks the count check. In `maximum_v2_image…`, the duplicate-key and `set64(flag+2, 74)` edits are also caught by
    the consumed-record cross-check and the ordering check.
  - **Consequence if the count check regressed.** A 17..=64-pair Found under a large `first_free` would commit and persist a state
    that every node's `decode` then refuses (`count > 16`). Every replica would apply it, so the whole cluster would fail to reopen or
    install a snapshot.
  - **Isolating cases I ran locally and did not commit.** Each one fails under the counterfactual and passes at HEAD:
    1. Pure test: `EnrollmentState::new(CLUSTER, node(18))`, then Found `(1..=17)` with distinct keys gives `BadFounders` with the
       state unchanged. Under the counterfactual it returned `Founded { replay: false }`.
    2. Maximum image: set member 1's key to member 0's key **and** set record 49's consumed key (offset
       `next + 18 + 49*145 + 105`) to the same value. Then only the member duplicate-key check fires. Under the counterfactual it
       decoded `Ok`.
    3. Maximum image: `set64(x, flag + 2 + 15*40, 74)` (the **last** member). The ordering stays ascending and the cross-check has
       nothing to compare, so only the range check fires. Under the counterfactual it decoded `Ok`.
  - **Fix.** Add these three cases to the existing tests. The evidence line "exercise … duplicate keys/nodes, … out-of-range IDs"
    should then state that each check is isolated. No source change.

## Spec (brief `2640629` + `4750ae1`)

**PASS on source. The evidence claim needs the R1 correction.**

- **Core registry.** `founded` plus `members: BTreeMap<NodeId, KeyDigest>`, at most 16 members. Every member is `< next_node_id` and
  keys are unique. Found never touches `next_node_id`, and Redeem is the only allocator, so IDs are never reused.
- **`FoundMembers`.** It wraps a private `Vec` whose only constructor `take`s 65 items and refuses the 65th, so at most 64 pairs are
  ever retained. `as_slice` is the only accessor. Lists of 0, 17..=64 pairs, unsorted lists and duplicate lists are all
  representable. The million-item counting-iterator test proves exactly 65 pulls.
- **Wire format.** `ClusterId` is unchanged at 16 bytes (asserted). Tag 4 is `90 + 40n` bytes (730 at 16 pairs, 2650 at 64) and tag 5
  is 97 bytes. Both are verified against independently built raw rows with a recomputed SHA-256, and `const _: assert!(90 + 40*64 <=
  MAX_ROW)` is present. Count 65, 64-with-63 pairs, trailing bytes, every truncation and tag 6 all return `Corrupt` with no panic.
  Decode checks `count > 64` before `with_capacity`.
- **Found apply order.** Structure runs first (1..=16 entries, strictly ascending, `< next_node_id`, distinct keys), whether or not
  `founded` is set. Then comes current-pair idempotence (`replay:true`) or `AlreadyFounded`, and only the unfounded valid case
  mutates state. An empty Found after founding gives `BadFounders` (proven by counterfactual, below). The brief and the code comments
  claim no original-bootstrap identity.
- **Common prefix.** WrongCluster, TimeOutOfRange and ClockBehind return before any mutation. The commit/prune step runs before the
  `NotFounded` gate and before the Found/Remove dispatch, so a `BadFounders` at prune time prunes records and leaves the registry and
  `next_node_id` alone (tested).
- **Mint, Redeem and Remove.** Before founding they return `NotFounded`.
- **Fresh Redeem.** `KeyInUse`, then `MembersFull`, then `NodeIdsExhausted`, all before the consume/insert/counter step. Consume and
  `members.insert` happen in one `apply`, so they share one store transaction. There is no attempt counter that could move first.
- **Remove.** `UnknownNode` when `node >= next_node_id`, `replay:true` when the node is absent, `LastMember` fail-closed, and no
  allocator change.
- **Replay after removal.** It returns `Revoked` with no grant, after the existing `Consumed`/`KeyConflict` checks. `joiner_reply`
  maps it to `Rejected` through the wildcard arm.
- **Prune.** Pruning drops records and never bindings (pure test and T4).
- **Peer authorization.** `authorize_peer` is pure and reads local applied state only, with static error text. `local_authorize` and
  `registry_covers` are `#[cfg(test)]`. The module doc and the method doc state that staleness is unbounded and that this is not
  production authority.
- **Codec.**
  - **State v2.** The suffix is `founded ‖ u8 count ‖ ascending (node ‖ key)`. Every decision-7 rejection is implemented: count
    > 16, unsorted or duplicate nodes, duplicate keys, a member `>= next`, members or records while unfounded, an empty founded
    registry, a current member whose consumed record has a different key, trailing bytes, and a non-canonical re-encode (the
    image-level `encode(&i) != b`).
  - **Size and checksums.** The independently built maximum image (64 records, 16 members, 3605-byte membership) is 13655 bytes,
    under the 16384 limit.
  - **v1 refusal.** The checksum runs before the version, so a checksum-valid v1 image gives `Unsupported` from `peek`, `install` and
    `open`. The failed `open` leaves the SQLite file byte-identical. `user_version` and the schema are unchanged.
  - **Log tags.** Tags 0..3 are unchanged, and the match accepts `1 | 2 | 4 | 5`. Semantically invalid committed Founds decode on
    reopen (T4 restarts all three nodes after committing six of them).
- **T4 (actual OpenRaft, `mem_net`, private SQLite).**
  - **Setup and founding.** Three voters start unfounded. Six invalid Founds (0, 17 and 64 pairs, unsorted, duplicate key,
    unallocated) each commit `BadFounders` with the leader state unchanged. Then `Found[1..3]` succeeds. After founding, the empty,
    17- and 64-pair Founds give `BadFounders`, a subset replays, and a conflicting key gives `AlreadyFounded`.
  - **Join and revocation.** Mint/Redeem registers node 4 in the registry only, with no Raft membership change, and all three nodes
    authorize it. The test stops a follower, commits `Remove(4)`, checks that a replay gives `Revoked`, then commits 40 `UnknownCode`
    entries. It waits until the leader has purged past the stopped follower's applied index and asserts that no snapshot reached that
    follower beforehand.
  - **Snapshot catch-up.** After restart, the test asserts at least one snapshot RPC to that follower. Because the purge index (55) is
    past the Remove's index, the Remove must arrive through the snapshot. The caught-up follower refuses node 4 and accepts the
    founders.
  - **Prune and restart.** After the prune step, all three nodes have zero records, refuse node 4, accept the founders, and satisfy
    `registry_covers`. Stopping and restarting all three then converges them again.
- **Reported honestly.** The evidence presents this as an in-process proof only: no membership workflow, TLS, signing, freshness, or M3
  acceptance. The CA handoff pins are not dependencies (lockfile unchanged).

## Security

**PASS. No blocking finding.**

- **Revocation and replay.** The removed-node replay gap the brief identified is closed. Counterfactual: removing the `Revoked` check
  fails both the pure registry test and T4. A removed ID cannot be reallocated because `next_node_id` only grows. Reusing a removed
  key under a fresh ID is allowed, as the brief states, and is not load-bearing.
- **One key, two nodes (A3).** Blocked at Redeem time and at decode. Counterfactual: consuming the code before the `MembersFull`
  return fails `registry_capacity_rejection_keeps_code_and_counter`. The KeyInUse-then-success path is tested against both a founder
  key and a joiner key.
- **Bounded input.** Found allocation is bounded by type before any command exists. Decode reads a single byte for the count and caps
  it before reserving. The member cap of 16 is enforced at apply and at decode. A state image cannot exceed `LIMIT` because only a
  valid Found or a capped Redeem writes members.
- **No secrets.** Keys are SPKI digests. Debug output contains no verifier or code material beyond what the existing privacy scan
  already covers. `Reject`/`PeerRefusal` text is static.
- **Authority stays local.** `authorize_peer` and its staleness limit are stated exactly, and nothing in the slice uses it for control
  or execution. The amendment correctly keeps Raft peer traffic from needing a quorum to form one.

## Non-blocking notes (no change requested in this slice)

- **N1.** `registry_covers` is asserted only positively in T4, so its false path is unproven. Prove it when the membership workflow
  first relies on it.
- **N2.** The `count > 64` guard in the Found row decoder cannot be told apart from truncation by tests (count 65 with 64 pairs fails
  as truncated either way). It is defense in depth for allocation, which a `u8` count already bounds to 255 × 40 bytes.
- **N3.** `Found` and `Remove` carry no actor. Anyone who can propose to the local engine holds removal authority. That matches the
  brief, but the future control or founder API must gate it.
- **N4.** The operational advisories (Weak-expiry file-close release before the first non-test restart caller, and the ReadIndex
  apply-catch-up deadline in the certificate slice) remain assigned and are not production-ready. No current failure was observed, so
  no churn is requested.
- M2 stays open. A7 is FINAL NO-GO, and the consumed A12 attempt with unconfirmed cleanup gets no retry. Full M3 acceptance is unmet.
  Everything here is **unverified on real hardware**.

## Commands I ran (actual, not reused)

Every Cargo process ran with `TEMP`/`TMP` = `D:/Temp/arxcovo-registry-review-target/temp` and `CARGO_TARGET_DIR` =
`D:/Temp/arxcovo-registry-review-target/target` (fresh and unique), toolchain 1.95.0, `--locked --offline`. PowerShell runs used
BelowNormal priority. No installs, fetches, deletions or cache cleaning. Logs are retained in that directory.

| Command (at HEAD unless noted) | Result |
| --- | --- |
| `cargo test -p arxd --lib cluster:: -- --nocapture` (fresh build) | **107 passed, 0 failed, 2 ignored**. Build 1m18s, test run **8.48s**, total wall 87.5s. T4: follower 2, stopped 15, purged 55, snapshots 1, 4.25s. T3: stopped 3, purged 39, snapshots 1, final applied 44, 2.94s. U6: 64/64 resolved. The logged panics are the existing injected-panic tests |
| Gap counterfactual (count cap widened to 64, member range and duplicate-key decode checks removed) | Cluster suite 107/107 pass, so the gap is confirmed (R1) |
| Three probe cases under the gap counterfactual | All three accepted (`Founded`, `Ok`, `Ok`) |
| Same probes at HEAD source | All three rejected. 2 tests passed |
| Counterfactual: `Revoked` check removed | 2 failed (pure registry test, T4). 27s incremental |
| Counterfactual: structural check skipped when founded (vacuous replay) | 3 failed (prefix test, registry test, T4). 20s |
| Counterfactual: code consumed before the `MembersFull` return | 1 failed (capacity test). 19s |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | Pass, 37.8s |
| `git diff --check 0ab5038 HEAD` | Pass |

I reverted every counterfactual and probe with `git checkout`. The worktree was clean before this commit. I did not run the full
workspace tests (the owner-fixture baseline is not available to agents, and no admin work was done) or hosted CI and its 15-minute
budget. No native M2, VM, Lab Manager, TLS, CA, signing, listener, push, PR, comment, status, merge or dispatch action was taken.

## Eligibility

- **Opus side: not yet.** It becomes eligible once Sol adds the three R1 cases (test-only) and a narrow recheck confirms that each one
  fails under its counterfactual. Spec and Security already pass on the current source.
- **Merge** additionally requires green hosted required checks within the CI budget, and a coordinator-only merge.
- **Material risks after R1.** The risks are design-level and already documented: unbounded local revocation staleness, a revoked
  voter still counting toward quorum until the membership workflow exists, and a v1 store or mixed-version cluster being unsupported
  (non-production only). None of them blocks this slice.
