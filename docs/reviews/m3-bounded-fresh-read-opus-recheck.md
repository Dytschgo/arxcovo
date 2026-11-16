# M3 A0 bounded fresh read: Opus delta recheck of the refusal-contract correction

Reviewer: Opus 5.5 high, 2026-11-16. Target `8237db1bb5d0be09d7061e1ea257a1c9bcb4d521`. The prior review
(`docs/reviews/m3-bounded-fresh-read-opus-review.md`, target `32bce78`) required corrections D1–D3, and `c0bf702` recorded
Sol's acceptance of the policy. This is a **narrow recheck** of `c0bf702..8237db1` only. The Standards, Spec and Security
findings on the A0 source in the prior review carry over, because no A0 production code changed in this delta.

## Delta scope (verified with `git diff --stat c0bf702 8237db1`)

- `crates/arxd/src/cluster/consensus/three_node.rs`: +12/−6, test code only (`t2_leader_offline_reads_and_unknown_retry`
  and `r2_isolated_read_refusal`).
- `docs/briefs/m3-certificate-issuance-first-slice.md`: four lines.
- `docs/reviews/m3-certificate-issuance-slice-sol-review.md`: +4, the Sol policy-check record.

`consensus.rs` and `consensus/adapter.rs` are not touched. The delta adds no dependency, Cargo, workflow, PROGRESS or ADR
change.

## D1, R2: PASS

- **Discriminant.** `Err(NotLeader | NoQuorum)` is accepted at any time. `Err(Unavailable)` is accepted only when
  `elapsed >= READ_BUDGET` (1500 ms). Every other result fails the test, including `Ok`, `Busy` and `Corrupt`. **An early
  `Unavailable` still fails.** Fatal, poisoned owners, a `try_await_ready` error or a dropped `tx` arriving before 1500 ms
  makes the match return `false`. I verified this by reading the match arm; I did not run a mutant.
- **Soundness of the lower bound.** `started` is a `tokio::time::Instant` sampled before the call. `fresh_read` computes
  `deadline = Instant::now() + READ_BUDGET` at or after that point on the same clock (`consensus.rs:226`, `:354`). Every
  deadline path has `now >= deadline`: the `timeout_at` elapse (Tokio timers never fire early), the `now < deadline` gate
  on receive, and an owner that withholds its send. So a truthful deadline refusal always satisfies the bound, and the
  bound is not a flake source.
- **Upper bounds kept strict.** `assert!(elapsed <= 2500 ms)` is unchanged. The permit drain is wrapped in
  `timeout_at(started + 2500 ms, read_units_return(.., 2500 ms)).await.unwrap()`, so the caller's return, all
  `READ_PERMITS` returning, and `owners.set` being empty must all happen within 2.5 s **of the original call start**. The
  inner 2.5 s timer starts only after the caller returns, so it is redundant. The outer timer is the binding one, and it is
  strictly tighter than the old restart-after-return window. This differs from my suggested `saturating_sub` form but is
  equivalent in strictness. No ownership or duration assertion was weakened.
- Nit (no action): the R2 `assert!` has no failure message, so a red run prints only the `eprintln!` line above it. That
  line already shows the refusal and the elapsed time.

## D2, T2: PASS

The diff changes only the isolated-read `matches!` pattern, which adds `ReadError::Unavailable`. The 3 s
`timeout(..).unwrap()`, the follower stops, the unknown-propose pattern, the restart, replay, `grant` and convergence lines
are all byte-identical. Non-success is still required.

## D3, brief and Sol record: PASS with one nit

- **Honest limits** now says the engine timer starts at dequeue, so a caller-deadline `Unavailable` may precede engine
  `NoQuorum`, and that no shorter sub-budget and no timeout relabelling is used. This is accurate against
  `raft_core.rs:405–461`. Per the source, "may precede" understates what actually happens: with the pinned OpenRaft it
  always does. It is still truthful, so no change is needed.
- **R2 line**: `NotLeader`/`NoQuorum`, or `Unavailable` only at or after `READ_BUDGET`, with the caller and all permits
  within 2.5 s of the original start. This matches D1 as implemented. Typo nit: "within2.5s" is missing a space.
- **R5 tail**: the T2 exception is stated narrowly, and the T2 non-success requirement is retained.
- **PR A mapping**: `ReadError::NoQuorum` maps to `IssueError::Unavailable`, which closes advisory A3.
- The PR A line "existing T1-T4 … pass unmodified" (brief line 186) was not given the exception I suggested. Because A0
  merges first, PR A will see the already-corrected T2, so "unmodified" is accurate relative to that base. No action is
  needed.
- The Sol record accepts the policy, the 2.5 s bound from the start, the narrow T2 exception and the PR A mapping, and asks
  for a fresh recheck. It is consistent with the delta.

## Checks run by this reviewer at `8237db1`

These ran in a unique target directory and temp directory under `D:\arxcovo-a0-opus-recheck-8237db1`, separate from the
coordinator's run. I did not observe the coordinator's own run and make no claim about it.

- `cargo fmt --all --check`: PASS.
- `cargo test -p arxd --locked --offline cluster:: -- --nocapture`: **114 passed, 0 failed, 2 ignored** (the two owned
  `crash_child` process-kill fixtures), 9.08 s. That is the expected 109 plus the 5 A0 tests. Observed timings:
  `R2 refusal=Err(Unavailable), caller=1.5088s, units returned by=1.5091s`; R1 caller 1.5010 s; R3 caller 1.5020 s with
  149 heartbeat ticks. The panic lines in the log come from the intentional panic and poison tests, which passed.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`: PASS.
- **Not rerun:** the workspace-wide nextest (its 49 non-cluster failures reproduce on the base, per the builder), and the
  pre-fix R1 and R3 hang counterfactuals plus the R3 permit-early mutant. Credit for those stays with the builder's
  recorded evidence. The delta does not touch R1, R3 or production code, so rerunning them was not needed.

## Findings

- **No blockers.** Two nits, neither needing action: the missing space in "within2.5s", and no message on the R2 assert.

## Status

D1–D3 are applied correctly, and the A0 refusal-contract conflict is closed. Approved from the Opus side for this delta.
This is **not merge-green** by itself. It still requires a different-provider independent review of the A0 change, fresh
hosted checks on the exact head (three OSes, ci-time-budget, agent-review) and a coordinator-only merge. A0 still has no
production caller and adds no PKI dependencies. Store-file advisory 1 stays open. M2 remains open, and real-host, VM and
hardware paths are **unverified on real hardware**.
