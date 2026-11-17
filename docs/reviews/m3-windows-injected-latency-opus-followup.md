# Windows I4 under injected storage latency: Opus bounded follow-up

Opus 5.5 high, 2026-11-17. Single agent, no children. Worktree base `e54d46d6`. The crates are byte-identical to PR164 source `17a3fb2a`
(`git diff 17a3fb2a e54d46d6 -- crates` is empty). Main is `c612152`.
Authority: the babysitter-delegated owner request for one bounded follow-up on this local Windows PC (owner-authorized for tests in
`e54d46d6`). **This is an experiment, not a fix.** No fix attempt is consumed: Sol shared suite stays 3/3, Opus recovery 0/2.
Inputs: `AGENTS.md`, the dytschgo-debug skill, `m3-pr164-i4-replay-opus-disposition.md`, the negative F1 append-1200 ms control
(`3503293`/`fe1af6e`, `docs/evidence/m3-windows-phase-attribution.md`) and the root local baseline `D:/Temp/arx-i4-local-1y__vt3f`.

## Result in one line

A test-only, per-store constant latency at the hosted *typical* level (vote 300, append 400, apply 600 ms on all three nodes) reproduced
the **exact** hosted signature once: `issue_tests.rs:237:14`, `NotLeader(None)`, `attr none`, `cand=Store`, with the same term shape.
The pre-registered counterfactual (vote 0, append 0, apply 600) passed once. Each condition ran exactly once. The reproduction rate
is unknown, and the injection model is **not** shown to be the hosted cause.

## Baseline (reused, not rerun)

Source identical, so the root record is reused. Unchanged I4 passed 1/1 in 4.340 s. Max Vote 8, Append 7, Apply 8 ms, `cand=None`,
`write=2:25:25`, term changes 2 per node (stderr SHA-256 `2a603327…63b67b`).

## Hook (temporary, test-only, now removed)

Patch: `docs/evidence/m3-windows-injected-latency-diag.patch` (SHA-256 `d0a21ca09596722095b86943453e460dae97558eca4b14dc40d5f5cdf0b83247`). It changes two files, and every change is
in `diagnostics.rs` (a `#[cfg(test)]` module) or behind `#[cfg(test)]` in `adapter.rs`:
- `NodeStats` gains `vote_delay_ms` and `append_delay_ms`. The pre-existing `apply_delay_ms` and `started()` apply hook are reused unchanged.
- `inject(store, op)` sleeps the per-store delay. For Vote (`save_vote`) and Append it runs inside the blocking closure **while the
  `VoteLogStore` lock is held**. The hosted `queue` histogram stayed below 10 ms, so the hosted latency was inside the op. Apply sleeps
  before the `StateStore` lock (the existing hook). This asymmetry is a known modeling choice.
- It is armed only by `ARX_DIAG_INJECT_MS=vote,append,apply` in the single test process, each value capped at 1250 ms, with the same
  values on all three nodes and every registered op. `Diag::new` prints one `[DIAG-INJ]` line.
- Unchanged: the I4 body, `Cluster` helpers, `Params`, every deadline, retry, assertion and election timing, and production code paths.

## Pre-registration

The scratch plan `D:/Temp/arx-opus-inj-e54d46d/PLAN.md` (final SHA-256 `0cabb6d416b894ef53f794b148ed0892034d174ece15190fbbb61f1bbbca1f43`) was written before any execution. Each
outcome line was appended after its run. The decision tree:
C1 hosted-typical `300,400,600`. If C1 was exact, run CF1 `0,0,600` and stop. Otherwise run C2 hosted-max `400,650,1080`: if exact,
run CF2 `0,0,1080`; if green, run C3 ceiling `1000,1250,1250`; if nearby red, stop.
Predictions: C1 green 45%, nearby 35%, exact 20%. CF1 would show no churn beyond baseline, and an exact CF1 would refute the
vote/append-versus-election reading.

Command, run once per condition from the worktree root, with `CARGO_TARGET_DIR`, `TMP` and `TEMP` under `D:/Temp/arx-opus-inj-e54d46d`
and `CARGO_PROFILE_DEV_DEBUG=0` (matching CI and the root `[unoptimized]` profile):

```
ARX_DIAG_INJECT_MS=<v,a,p> cargo nextest run --locked --offline -p arxd --lib \
  -E 'test(=cluster::consensus::issue::issue_tests::i4_leadership_loss)' --success-output immediate --failure-output immediate
```

The build used the same arguments plus `--no-run` (nextest 0.9.151). An earlier debuginfo build was discarded without running anything.

## 1) Actual (hosted) versus induced

| | hosted PR164 (job 133587063992) | C1 induced 300/400/600 | CF1 induced 0/0/600 |
|---|---|---|---|
| verdict | FAIL 11.495 s | **FAIL 12.485 s** | PASS 10.120 s |
| panic | `:237:14` `NotLeader(None)` | **`:237:14` `NotLeader(None)`** | none |
| attr / cand | none / Store | none / Store | (pass) / Store |
| Vote max ms (n1/n2/n3) | 282 / 344 / 400 | 308 / 310 / 310 | 7 (all) |
| Append max | 586 / 659 / 640 | 409 / 410 / 410 | 8 |
| Apply max | 1079 / 1083 / 903 | 611 (all) | 613 |
| Vote / Append / Apply op counts | 3,7,8 / 5,4,5 / 5,5,5 | 3,5,7 / 5,3,4 / 5,5,5 | – |
| founding `write` retries | 307 | 1961 | 32 |
| runtime lag | 15 ms, over=0 | 14 ms, over=0 | 15 ms, over=0 |
| per-node term changes (dump) | 1 / 4 / 4 | 1 / 3 / 4 | 2 / 2 / 2 |

Hosted Vote ops fell mostly in the 200-1000 ms bucket, with two per node in 50-200. C1 put every op in 200-1000. C1 churned the founding
phase much more than hosted did (1961 retries against 307), so C1 is harsher than hosted in that phase and gentler on Append and Apply
peaks.

## 2) Reproduction: exact (one sample)

C1 stderr (`C1.err`, SHA-256 `20ce6a9b1040d492e564d925cfb891412f729eb3ab70cc141bc77c9760261778`):

```
[DIAG-INJ] vote=300 append=400 apply=600 nodes=3
thread 'cluster::consensus::issue::issue_tests::i4_leadership_loss' (55148) panicked at crates\arxd\src\cluster\consensus\issue_tests.rs:237:14:
called `Result::unwrap()` on an `Err` value: NotLeader(None)
[DIAG-3n] v1 fail t=12348 lag=14@182 over=0 ticks=200 cand=Store
[DIAG-3n] helpers leader=1965:1812659:1810695 write=2:1961:1961 converge=0 stop=0
[DIAG-3n] attr none
```

Leader/term trace, compressed from the retained rings (fields t,term,state,leader,last,committed,applied). The dump truncated
(`bytes=8110 trunc=1`), so the windows start late:
- n1 (isolated): `10260,2,L,1,4,4,4` through 12337. `ack_ms` was 4396 at 12337, so its last quorum ack was at about t=7941 and the
  4 s selection bound ends near 11.94 s.
- n3: `8282,2,F,1` → `9803,3,C` → **`10829,3,L,3,last5,commit4`** → `11356,4,F,-` → `12125,5,C,-`.
- n2: `9075,2,F,1` → `10112,3,F,-` (leader none) → `11048,4,C,-`. **n2 never shows `leader=3`.**

This has the same shape as the hosted disposition's facts 2-5. The only non-`l` Leader sample inside the bound is n3 in term 3, so
`new` = n3 (inferred). The connected peer moved to a higher term without ever recording n3 as leader. n3's term-3 blank entry (last 5)
never committed. At the proposal, n3 was a term-4 Follower or term-5 Candidate, which gives `ForwardToLeader{leader_id: None}` and
therefore `NotLeader(None)`. The only difference from hosted: hosted n3 later led term 5 (at 11244), while C1 n3 was still a term-5
Candidate at 12337. That is not a different failure, because the panic is at the same line with the same value. It is noted only to
show that the traces are not identical.

## 3) Next smallest feedback loop (proposal; root decides)

The C1 command with the patch applied is now a red-capable, focused (~13 s), agent-runnable loop for the I4 signature on this PC. It is
**not yet shown deterministic**: n=1. Smallest next steps, each needing new root authority (none started here):
1. Rate: run C1 unchanged N=5 times, pre-registered, to pin a reproduction rate. This is red-confirmation, not green-seeking.
2. Minimise one component at a time (one run per condition, rate-scaled): vote-only `300,0,0` and append-only `0,400,0`. This
   separates election-path persistence from replication persistence. The old append-1200-on-leader-only control was negative, which
   argues against append alone.
3. Use the loop to evaluate the disposition's E8 predicate if the owner chooses it. Prediction for this C1 trace: at 10829, n2 is a
   term-3 Follower with leader none, so the predicate is false. The run would then fail at the selection unwrap `:210` (the bound ends
   ~11.94 s, and n3 never became established), not at `:237`.

## 4) Counterfactual

CF1 `0,0,600` (vote and append latency removed, apply latency kept) passed in 10.120 s. Term changes stayed at baseline (2 per node),
retries were 32 (baseline 25), Apply max was 613 ms, and `cand=Store` came from the apply highs alone (CF1.err SHA-256 `43cd31510dd897f6fa88df6b1c0095293a8357d51d71b4997a4224b2e0b2988e`).
This matches the CF1 prediction. Within this model, the injected vote and append latency is necessary for the red, and apply latency
of 600 ms alone did not produce churn. One green sample does not prove that apply latency can never contribute.

## 5) Limits

- One execution per condition. The reproduction rate is unknown, so neither the red nor the green is a rate.
- The model is a constant per-op sleep (vote/append under the store lock, apply before it) on all nodes, not hosted filesystem
  behavior. **It is not claimed to be the real hosted cause.** It shows only that storage-op latency in the hosted range is sufficient
  locally to produce the exact signature.
- Why n2 never adopted n3 as leader within its election timeout is not traced. Candidates include a follower append (400 ms under the
  lock) delaying its acceptance of n3's blank-entry append, or vote persistence stretching rounds against the 500-1000 ms timeout.
  Neither is tested.
- The rings truncated (`trunc=1`), so early transitions are missing. Watch coalescing means a missing sample proves nothing.
- Local PC and nextest timing differ from hosted runners. The baseline was reused, not rerun.
- No hypothesis credit beyond "injected vote and append latency reproduces it here". No fix credit, no hosted credit.

## Hygiene

- Source restored: `adapter.rs` `c1468e51…`, `diagnostics.rs` `f2eb65b8…`, matching the pre-run hashes. `git diff 17a3fb2a -- crates`
  is empty, and `grep DIAG-INJ crates` finds nothing.
- The isolated target and temp are task-owned under `D:/Temp/arx-opus-inj-e54d46d`, built `--locked --offline`. Existing caches were
  untouched and nothing was deleted. No detached or background processes; none remain.
- Not done: no push, PR status/comment/watch/merge, hosted rerun, protection change, global or OS setting, host stress, admin, VM,
  install, quarantine change, or edit to PROGRESS or production/test source. This commit adds only this file and the patch artifact.

Status unchanged: M2 open; M3 unchecked; Windows quarantine of 8 plus R5 unchanged; PR164 held; main `c612152` RED; Windows three-node
**unverified on hosted Windows**; all hardware paths **unverified on real hardware**.
