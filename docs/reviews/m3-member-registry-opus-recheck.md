# Opus R1 recheck: member registry isolated rejection tests

Disposition: **PASS (R1 resolved). Opus side is merge-eligible for the member registry slice.** Merge still needs green
hosted required checks and a coordinator-only merge after the latest exact-main CI is green.

Anthropic / claude-opus-5-5, independent review sub-agent, 2026-11-16. This is a narrow corrective recheck of R1 in
`docs/reviews/m3-member-registry-opus-review.md` (retained `ff23622`, original `e3ca413`). It is not a broader feature review.
Sol implemented the slice and the R1 fix, so this is a different-provider review. Opus wrote the brief architecture, so as
before this recheck gives no independent credit to the design.

## Pins

| Item | Identity |
| --- | --- |
| Recheck HEAD (clean worktree, branch `t3/member-registry-r1-recheck`) | `5b451df014386929109c0ef87afd7c7e5515a68d`, equal to `work/m3-enrollment-auth-boundary` |
| R1 fix | `7a37a68`: 31 added lines, only in `enrollment/tests.rs` and `state_store/tests.rs`, with no deletions |
| Merged main | `a318ae1` (PR147). The merge base with the feature branch is `0ab5038` (PR146) |
| Production registry source | `git diff e617200 5b451df -- crates Cargo.toml Cargo.lock .github` lists only the two R1 test files and PR147's `consensus/adapter.rs` and `consensus/tests.rs` |

## Merge preservation

- **Feature blobs.** All ten registry paths have the same blob at `7a37a68` and at `5b451df`. The eight non-R1 paths still
  match the evidence manifest. The two R1 test files changed as expected: `enrollment/tests.rs` from `eea3855…` to
  `762505ee071937758f1caabaf324a57185a941b7`, and `state_store/tests.rs` from `72203af…` to
  `1bc25e51483fa1144b691b7da72b4e6746bf5591`.
- **Main delta.** `git diff 7a37a68 5b451df` and PR147's own patch (`a318ae1^..a318ae1`) are identical for the source. That
  covers `consensus/adapter.rs` (+31) and `consensus/tests.rs` (+37, one new test,
  `cancelled_append_callback_releases_store_before_worker_returns`). The two patches differ only in `docs/PROGRESS.md` merge
  resolution and in the Linux s9c evidence file, which was already on the feature side. The append fix is the only source delta
  from main, and it is independent of the registry.
- **No out-of-scope changes.** No `Cargo.toml`, `Cargo.lock` or `.github` change since `0ab5038`.

## R1 test review

1. **`founder_count_cap_is_independent_of_allocation_and_other_structure`** (enrollment). The test builds Founds of 17 and 64
   pairs, both before and after founding, with `next_node_id = 65`. Nodes `1..=count` are all allocated (`< 65`) and strictly
   ascending, and the keys `key(1..=64)` are unique and differ from the founder key `key(250)`. Each case must return
   `BadFounders` and leave the state equal to the pre-command clone. Every other structural guard is satisfied, so only the
   `1..=MAX_MEMBERS` count check can reject it.
2. **Duplicate member key image** (`state_store/tests.rs:713`). Member 1's key (bytes `flag+50..flag+82`) is set to member 0's
   key. Record 49's consumed key, at `next + 18 + 49*145 + 105`, is set to the same value. I checked that offset: a record is
   145 bytes, and the key starts 105 bytes in, after 16+16+16+32+8+1+16 bytes. Record 49 is consumed by node 59, which is member
   1. So the consumed-record cross-check finds agreement, node order is unchanged, and only the member duplicate-key check fires.
3. **Last member equals `next_node_id`** (`state_store/tests.rs:720`). The test sets the last member's node (offset
   `flag + 2 + 15*40`) to 74, which equals `next_node_id`. Order stays ascending (72 < 74), and no consumed record has node 74,
   so the cross-check has nothing to compare. Only the `node >= next_node_id` range check fires.

The test changes are additive. No existing assertion or corruption case was removed or weakened. One cosmetic nit: two comments
are missing spaces ("at74"). No change requested.

## Individual mutants (each applied alone, compiled, then restored)

Each mutant was a one-line edit to a scratch copy in the worktree. After each run I restored the file with `git checkout -- <file>`
and checked its blob against the original hash, and `git status --porcelain` was empty. The original hashes were `registry.rs`
`3764f4d5…` and `codec.rs` `ecaeb9ff…`, and both match after all three restores. I committed no source change. The patches and
logs are kept in the target directory listed below.

| Mutant | Edit | Full cluster suite | Killing assertion |
| --- | --- | --- | --- |
| M1 Found count cap | `registry.rs:60` `1..=MAX_MEMBERS` changed to `1..=MAX_FOUND_WIRE` | **108 passed, 1 failed** (compiled, 17s) | `enrollment/tests.rs:767`: left `Founded { replay: false }`, right `Rejected(BadFounders)`, at count 17, unfounded |
| M2 decoder duplicate member key | `codec.rs:135` `\|\| members.values().any(\|k\| *k == key)` deleted | **108 passed, 1 failed** (compiled, 24s) | `corrupt` (`tests.rs:95`) called from **`tests.rs:713`** (new case): left `Ok(Image…)`, right `Err(Corrupt)` |
| M3 decoder member `>= next` | `codec.rs:133` `node >= next_node_id` changed to `false` | **108 passed, 1 failed** (compiled) | `corrupt` called from **`tests.rs:720`** (new case): left `Ok(Image…)`, right `Err(Corrupt)` |

The backtrace caller lines show that the original masked corruptions still pass under M2 and M3: the duplicate-key case at
line 697 is caught by the consumed-record cross-check, and the first-member-equals-74 case at line 711 is caught by the ordering
check. Each mutant is killed only by its new isolating case. This gives
the 3 isolated tests and 3 assertion-killed mutants the prior review required.

## Commands I ran (fresh, not reused)

Every Cargo process used `TEMP`/`TMP` = `D:/Temp/arxcovo-registry-r1-recheck-opus-5b451df/temp` and `CARGO_TARGET_DIR` =
`D:/Temp/arxcovo-registry-r1-recheck-opus-5b451df/target`. That directory was new and unique, and the target was built fresh.
Toolchain was rustc/cargo 1.95.0, with `--locked --offline`. There were no installs, fetches, deletions or cache cleaning.

| Command at `5b451df` | Result |
| --- | --- |
| `cargo test -p arxd --lib --locked --offline cluster:: -- --nocapture` (fresh build, baseline) | **109 passed, 0 failed, 2 ignored**. Build 1m12s, tests 8.86s, total 82s |
| Same suite under M1, M2 and M3 (each alone) | 108 passed, 1 failed each (table above) |
| Same suite after all restores (final) | **109 passed, 0 failed, 2 ignored**, 8.51s. T4: follower 2, stopped 15, purged 55, snapshots 1, 4.39s. T3: follower 2, stopped 3, purged 39, snapshots 1, final applied 44. U6: 64/64 |
| `cargo fmt --all -- --check` | Pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass, 38s |
| `git diff --check e617200 5b451df` and `git diff --check 0ab5038 5b451df` | Pass |

The count of 109 is the prior 107, plus one R1 enrollment test, plus one PR147 consensus test. The R1 state-store additions sit
inside an existing test. The 2 ignored tests are the owned process-kill child entry points. **The earlier unobserved "110-test"
root run is not credited and does not match this count.** I did not run the full workspace tests (the owner-fixture baseline is
not available to agents) or hosted CI and its 15-minute budget. I did not check current main `a318ae1` CI. That run is
pending/unknown to this recheck, and no merge may follow until the latest exact-main CI is green.

## Evidence wording correction

The claim in `docs/evidence/m3-member-registry.md` that the tests "exercise … duplicate keys/nodes, … out-of-range IDs" was
overstated before `7a37a68`, because other guards masked these three checks. It is accurate now. The manifest's two test-file
pins and its 107 count are stale. Suggested coordinator evidence text (for the coordinator to apply; I did not edit
PROGRESS or the evidence file):

> R1 isolation (`7a37a68`, test-only, +31 lines in `enrollment/tests.rs` and `state_store/tests.rs`): Found count cap,
> decoder duplicate member key and decoder member `>= next_node_id` are each proven by an isolating case. The cases are a
> 17/64-pair Found that is allocated, sorted and unique-keyed, run before and after founding, which gives `BadFounders` with the
> state unchanged; a maximum-image duplicate member key with record 49's consumed key updated to match; and a last member equal to
> `next_node_id` (74) with order preserved. An independent Opus recheck at `5b451df` killed each guard's single-line mutant
> alone (108/1 each) at its new assertion (`enrollment/tests.rs:767`, `state_store/tests.rs:713`, `state_store/tests.rs:720`).
> The fresh cluster suite gives 109 passed, 0 failed, 2 ignored (8.86s; 8.51s after restore). fmt, locked offline workspace
> Clippy and `diff --check` pass. Test blob pins are now `enrollment/tests.rs` `762505ee071937758f1caabaf324a57185a941b7`
> and `state_store/tests.rs` `1bc25e51483fa1144b691b7da72b4e6746bf5591`. The other eight registry blobs are unchanged
> through merge `5b451df`. The production registry source is unchanged from `e617200`. The only main source delta is the
> independent PR147 append fix. This is in-process evidence only, **unverified on real hardware**. It gives no M3 acceptance.

## Limits

- No PROGRESS, source, ADR or evidence edit. The only commit is this report.
- No push, PR, status, merge, watch, child agents, native M2, VM, Lab Manager or admin action.
- M2 stays open. A7 is FINAL NO-GO, and the consumed A12 attempt gets no retry. Only pure and in-process acceptance applies here.
  Full M3 acceptance is unmet.
- The prior review's material design risks and notes N1–N4 are unchanged and not re-reviewed here.
