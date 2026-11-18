# M3 Ubuntu founder observer source recheck

**All five blockers closed.** The diagnostic source at
`ba493dfcc9fc783de3af7daba6caf658c6ab6107` is ready for the coordinator's
publication decision. One first-attempt normal full CI observation on Ubuntu
requires separate permission. This review supplies no hosted result.

Astra 6.2 high, independent source review. Compared the five Rust files against
PR176 `f1bf99bcbb3e18656879d6ce5cbf24e367c6056a` and traced the correction
from `30547a71` (same crates tree as builder `84522a3b`). HEAD retains the
review blob from `9da4bf2b` and crates tree
`3c4c30e04deaecd65f2be1f6bf63e0d713f32dd0` from correction `c3d45d6d`.

## Standards

1. **B1 closed, F10 leaf lifetime.** In `f10_drop_bounded`, the final issue
   still follows gate release, join, drain and the uninitialized check. Its
   success arm moves the issued leaf into `drop(leaf)` at the old statement
   boundary, before shutdown. The original 20s wrapper still applies.
2. **B2 closed, single evaluation and inactive capture.** Each of the five
   replacements matches one borrowed Option. Preflight clones the store,
   proposal clones Raft, and fresh read clones Raft then the store. Founder
   borrows Raft across initialize and leader wait. Each None arm returns the
   former fieldless error from the same function or async work block. Skipping
   eager construction of that constant has no observable drop effect.

   In fresh read, a missing store drops the already-cloned Raft, then the
   owners guard, before the channel and permit leave scope. Each successful
   spawn remains under the owners lock, which ends before the caller awaits.
   Both blocking reads release the store guard at the inner closure boundary,
   then drop the store Arc and permit in order. Proposal consumes its reply
   and drops its permit before sending. Erasing the test receiver wrappers
   leaves the original await and fallback. These conclusions follow from the
   source scopes and return targets.

   Caller reason and decision assignments require active slots. Both store
   result classifiers, the borrowed linearizer error match, initializer error
   match and leader error classifier require an active handle or capture.
   F6 starts slotless. Its 24-event limit, event recording, pending-phase
   transitions, exit event and formatter retain their original behavior.

## Spec

3. **B3 closed, guard attribution.** Fresh read forwards the on-time result
   with CallerReply/Ok. A ready late result records CallerReply/Expired, a
   closed receive records CallerExpiry/ReceiverClosed, and elapsed wait records
   CallerExpiry/Expired. Each refusal returns Unavailable. The three early
   owner stops leave the unperformed linearizer, applied or blocking edge
   empty. The three `>= ||` checks and the final `< &&` send check keep their
   original operands, order and short circuit. Founder and guard each use
   their original single ready-arm clock guard. The founder decision is set
   in the selected arm to InnerOnTime, ReadyLate or WaitElapsed.
4. **B4 closed, leader wait.** The existing metrics wait's error mapper borrows
   its `WaitError`, writes WaitTimeout or ShuttingDown to slot 11 and returns
   Unavailable. Only a successful leaderwait phase exit writes Ok there. A
   completed captured wait publishes once. Cancellation can leave it empty.
5. **B5 closed, live wiring and refusal precedence.** The Python audit reads
   the live founder and fresh-read match bodies and checks the tag and tail
   return in each selected arm. It inspects the pre-collapse leader mapping,
   explicit F10 drop, and shutdown/emit/primary-panic order in each refusal
   branch. Its six in-memory changes target those source sites directly.

   I also traced the invariants outside the audit's textual selectors. In
   query and proof the node and captured snapshot exist before refusal is
   saved. Refusal exits before either fixture submits blocking or TLS work.
   Router poison is recorded, the router is dropped, and shutdown is attempted
   once under the original absolute cleanup deadline. A shutdown error or
   elapsed timeout becomes a status. Both branches emit the snapshot before
   panicking with the saved founder error. F10 likewise records a returned
   shutdown error before enforcing its saved refusal. Its outer timeout can
   still cut off emission. The pure lane case writes through the first handle
   during the second capture, checks the second snapshot and actual handle,
   and checks F6 worker absence and unchanged fields with a real pending phase.

## Evidence and publication

**Executed here.** One run of `python -I -B docs/evidence/m3-founder-observer-wiring.py`
passed and rejected all six designated in-memory changes. Compared ordered
clock, closed-check, lookup and drop tokens in the four live helper ranges
against PR176. Compared F6 event and formatter source slices. Checked the five
Rust blobs and audit script for UTF-8/LF, the Rust diff for whitespace, and
the three root log hashes against the evidence appendix.

**Reused.** Root's first Clippy log has five unnecessary-lazy-evaluation
errors. Its recheck passed in 4.43s after the match changes. Four pure tests
passed once, with 641 filtered and a 46.01s test build. Root recorded final
guard8, fmt and diff passes. Sampler60 passed before the lookup batch. These
are retained checks. Runtime, engine and hosted CI were unrun here.

Successful gate, caller-receive and blocking-helper timestamps may stay
Unknown where absent. The sequential return-time snapshot supports no
synthetic latency or cross-process causal claim. PR176 remains held at
`f1bf99bc` with Ubuntu failed and Windows/macOS passed. Last green main was
`05b9c829`. Historical cause and fix remain unestablished. C invalid, T unrun,
shared Sol 3/3 exhausted, historical Opus 0/2, M2 open, M3 unchecked.
Hardware paths **unverified on real hardware**.

Open blockers at this source: standards 0, spec 0. No new blocking defect found.
