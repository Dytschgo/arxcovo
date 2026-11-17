# PR164 Windows I4 replay-proposal red: Opus static disposition

Reviewer: Opus 5.5, high, 2026-11-16, single agent, no children. Worktree head `a2d2c6a` (PR164 source head `17a3fb2a`, main `c612152`).
Scope: static reading only. No source change, cargo/test, rerun, loop, CI or GitHub API call, VM/admin step, install, cache deletion, push,
PR status/comment/watch/merge. This file is the only edit.
Inputs: `AGENTS.md`; the dytschgo-debug skill; the E7 disposition (`docs/reviews/m3-issuance-i4-failure-opus-disposition.md`); the recovery plan
(`docs/reviews/m3-windows-three-node-opus-recovery-plan.md`); the R5 brief (`docs/briefs/m3-r5-quarantine-restoration.md`); issuance brief I4/E3/E7;
resumption brief red handling; the failed job log `D:/Temp/arx-pr164-windows-failure.log` (264 lines, SHA-256 re-hashed here and matching
`38af2126…94f3e7dc3`, bounded lines 225-263 read); and the source cited below.
Debug-skill deviation, stated: Phase 1 (an agent-run red loop) cannot happen here because executing tests is forbidden. Everything below comes from
one hosted sample. No hypothesis is tested and no fix is credited.

## Exact facts from the log

- Windows job 133587063992, run 43949259583. `i4_leadership_loss` FAIL in 11.495 s. Panic at `issue_tests.rs:237:14`: `unwrap()` on
  `Err(NotLeader(None))`. 37/594 run: 36 pass, 1 fail, 17 skipped (R5 among them, as intended); 557 not run.
- `[DIAG-3n] v1 fail t=11244 lag=15@4513 over=0 ticks=184 cand=Store`; `attr none`; `helpers leader=311:860167:859857 write=2:307:307 converge=0 stop=0`.
- Line 237 unwraps the test body's direct `c.node(new).propose_response(Command::redeem(..))` (`issue_tests.rs:226-237`). It is not a `Cluster`
  helper, so `attr none` is the expected shape and is not a helper failure. `converge=0`: the E7 barrier (`:260-270`) and `converge` never ran.
- Helpers: one `write` call made 307 `NotLeader` retries, and 311 `leader()` calls polled 860,167 times (859,857 in one call). Both helpers ran before
  isolation, so the founding phase already churned (the fixture leader is at term 2, not 1). The helper absorbed that churn and the direct call could not.
- Store-op histograms (edges 10/50/200/1000 ms): n1 Vote max 282, Append 586, Apply 1079; n2 Vote 344, Append 659, Apply 1083; n3 Vote 400
  (6 ops at or above 200 ms), Append 640, Apply 903. Runtime lag max 15 ms, `over=0`: no CPU-lag signal. `cand=Store` is the heuristic in
  `diagnostics.rs:368-374`, a candidate and not a cause.

## Timeline from positive ring samples

The ring takes one sample per metrics publication (`diagnostics.rs:455-465`, watch-driven). A watch coalesces values, so a missing transition proves
nothing. Fields: t, term, state, leader, last, committed, applied, ack_ms. Node k is index k-1, and `l` = index 0 (n1).

| t (ms) | n1 (isolated) | n2 | n3 |
|---|---|---|---|
| ≤8597-8629 | L term 2 | F term 2, leader 1, last 4 | F term 2, leader 1, last 4 |
| 8876-9199 | L term 2 | (no sample after 8629) | C term 3, last 4 |
| 9200-9436 | L term 2 | F term 3, leader none | (no publication 9199→9681) |
| 9668 | L term 2 | **C term 4**, leader none, last 4 | |
| 9681-9682 | L term 2 | C term 4 | **L term 3**, leader 3, last 5 |
| 10083-10323 | L term 2 | C term 4 | F term 4, leader none, last 5 |
| 10538-10799 | L term 2 | C term 4, then F term 5 from 10810 | C term 5, last 5 |
| 11244 | L term 2 (ack_ms 4354 at 11242) | F term 5 | L term 5, leader 3, last 6 |

What these samples establish:
1. n1's last quorum ack was at about t=6888 (11242 − 4354), which is consistent with isolation (`:189`). Its 4 s selection bound (`:190-210`)
   therefore ends near t≈10.9 s.
2. `new` = index 2 (n3). This is an inference, strongly supported: the selection loop needs a non-`l` Leader inside the 4 s bound, and n3 at
   9681 is the only such sample. n2 has no Leader sample anywhere in its retained window (7790-11242).
3. n2 had entered term 4 (9668) **before** n3's first term-3 Leader publication (9681). No n2 sample shows `leader=3` during term 3.
4. n3's log stayed at last=5 through terms 3 and 4 and reached 6 only as the term-5 leader. The replay entry was never appended, so no
   duplicate consumption is possible from this run.
5. `NotLeader(None)` is `ForwardToLeader { leader_id: None }` (`consensus.rs:391-392`). That fits n3's term-4 Follower or term-5 Candidate window.
   The exact submit time of the proposal is unknown.

## Classification

**Violated test assumption (established by evidence).** I4 treats the first replacement node observed in Leader state as the node that will later
commit the replay. Between that observation and the proposal, it releases the old issuer, awaits that issuer's refusal (≤5.5 s) and waits for read
permits, all without re-checking the leader. Fact 3 falsifies the assumption: when the observation became possible, the connected peer was already
at a higher term. A Leader metrics sample does not prove established leadership. This is the same family as E7 (a first-seen Leader metric treated as
current) at a different site: before the heal, on a direct proposal to the chosen new leader, rather than cleanup that selected the old leader.

**Product contract: not shown violated.** `NotLeader(None)` is the designed, safe refusal. There was no wrong `Ok`, no stale data and no consumption.
Raft made progress eventually (n3 led at term 5, about 4.4 s after isolation). I found no settled bound on leader-establishment time in the
KICKOFF or the briefs I read. Whether the product owes one under hosted-Windows store latency is an owner question, not a finding.

**Cause of the churn: unknown.** The only candidate is timing. Vote and Append ops ran 200-660 ms against `election_timeout` 500-1000 ms
(`adapter.rs:370-371`, heartbeat 100 ms). n3's 482 ms publication gap while becoming leader (its blank entry, 4→5) and the 445 ms gap before term 5
coincide with Append ops in that range. n2 started term 4 no more than 468 ms after its first term-3 sample. None of this is proven: there are no
per-op timestamps, `cand=Store` is a heuristic, and the followers took about 1.7-2.0 s after n1's last ack to start term 3 (more than
`election_max`), which is unexplained. A slow snapshot alone earns no fix credit. The host's contribution is not established.

**Not caused by the quarantine (no claim either way).** The I4 source matches main: `git diff c612152 a2d2c6a -- crates` is the R5 `cfg_attr` only.
I4 passed on hosted Windows before, in green exact-main runs that include the required I1-I9 (e.g. CI43854991316 on `6231da98`,
`docs/evidence/m3-certificate-issuance.md:221`). The red is intermittent.

## Red handling map (no attribution)

- E3: a red follows the resumption brief. No rerun, quarantine, deadline, retry or Params change. The guard forbids `ignore`/`cfg` in `issue_tests.rs`.
- E7, final clause: any further engine red stops again for disposition, with no automatic extra tweak. This document authorizes none.
- Resumption brief: the helper-attributed branch (recovery plan) does not apply (`attr none`, and the test is not one of R1-R5). The nearest branch
  is "no helper attribution": unclassified, back to the owner under the stuck rule with Opus review, and no deadline, budget, permit or retry change.
  A missing attribution proves no root cause.
- R5 re-enable criteria, recovery plan §5/§6 and the eight quarantined tests are untouched. PR164 stays held: required `verify (windows-latest)` is red
  and the `ci-time-budget` deadlock remains (R5 brief, "Gating deadlock"). Pushing a new head to draw a different sample would be green-seeking, so it
  is not proposed.

## Attempt accounting

- Sol shared suite: 3/3 exhausted, unchanged. This is not a Sol attempt.
- Opus fixes: 0/2 remain. They are scoped to the quarantined eight (§5), and none is consumed here.
- E7: the one authorized I4 version is spent (merged in PR157). Its barrier was not reached, so this red neither credits nor discredits E7. E7
  (a new-fixture ordering clarification) grants no new authority.
- This disposition is diagnosis only: 0 fix attempts.

## Smallest bounded next contract (proposal; root decides; nothing starts here)

Contract statement the evidence supports: *I4 may propose the replay only to a replacement leader with established leadership: in one pair of
metrics reads, `new` is Leader at term T and the other connected node is Follower at term T with `current_leader == new`.* As a candidate test-body
change, tighten the predicate of the existing selection loop (`:190-210`) and leave everything else alone: the same 4 s bound, no new loop or retry,
and no helper, Params, deadline, assertion or quarantine change. The predicate becomes stricter, not weaker.

Counterfactual against this log: at 9681 the predicate is false (n2 is a term-4 Candidate and never shows `leader=3` in term 3). The earliest it could
hold is after 11244, past the ~10.9 s bound. This run would therefore **still fail**, but at the selection unwrap (`:210`) with the real cause ("no
established replacement leader within 4 s") instead of at `:237` with a misleading `NotLeader`. Without the predicate, this log is exactly the `:237`
signature. A red at `:237` after the change would contradict this disposition and route back for disposition.

Limits: the change sharpens the signal but does not restore green. It cannot prevent churn after leadership is established (store ops reach about
1.1 s against a 0.5 s `election_min`). It is a new I4 fixture change, so it needs explicit new authority (an "E8"), a builder, and other-provider review
of the exact diff. It is not a fix attempt and not an E7 continuation.

**Owner decision needed.** The evidence points at election timing versus store latency, and the only lever there is a timeout/Params change. E3 and
the resumption brief forbid that change, and it is coupled to I4's 4 s selection and 5.5 s refusal bounds. No agent-side route to a green I4 exists
under current constraints. The owner must choose explicitly: (a) the E8 predicate as diagnostic sharpening only (Windows I4 may stay red);
(b) a scoped election-timing change for I4/the fixture, as a counted fix attempt with a pre-registered counterfactual; or (c) keep PR164 held.
The independent `ci-time-budget` choice (R5 brief a/b/c) is also required before any PR164 merge.

Main `c612152` stays RED until a reviewed recovery PR merges with all five gates and its next exact-main CI is green. Both blockers (this I4 decision
and `ci-time-budget`) are owner decisions.

Status unchanged: M2 open; M3 unchecked; Windows quarantine of 8 plus R5 unchanged; Windows three-node **unverified on hosted Windows**; all hardware
paths **unverified on real hardware**.
