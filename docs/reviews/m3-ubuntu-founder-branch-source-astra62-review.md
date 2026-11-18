# M3 Ubuntu founder observer source review

**Hold candidate 30547a71.** A natural refusal would identify the outer founder
decision, but two different guard decisions print the same evidence. F10 also
keeps a successful issued leaf alive across shutdown. Correct the local sites
below before spending the first full-suite Ubuntu observation.

Astra 6.2 high, independent static review under the recorded owner stuck-review
override. Compared PR176 f1bf99bcbb3e18656879d6ce5cbf24e367c6056a to
30547a719e8e08e39aaf392a236b554eb4247817. Builder 84522a3b, retained ea4542d2
and candidate have identical crates tree ecbdb9da65d76c01dc3d8cb649a4126390d8bafc.
Line references below are to that candidate. Read the corrected brief, evidence
and the retained reviews from 76fab91b and 2be92556.

## Standards

1. **Blocking, F10 leaf lifetime.** In found_tests.rs:552-569, `found` is a
   named local. Matching `Ok(_)` neither moves nor drops its IssuedLeaf. Its DER
   Vec survives the shutdown await and emission until scope exit. PR176 drops
   the temporary returned by unwrap at the semicolon. Use `Ok(leaf) => drop(leaf)`
   in that match. Keep the existing eight drops, Busy/sign checks, gate
   release/thread join, read drain, uninitialized assertions and 20s wrapper.
2. **Blocking, evaluation and inactive calls.** found.rs:824,984 and
   consensus.rs:352,504,514 inspect the Option and then perform the original
   lookup again. Even unrelated calls in a test binary execute these probes.
   Put missing-store/Raft marks in the error closure of the original
   `as_ref().ok_or_else(...)?` lookup. The replaced eager error constants
   have no effects or destruction. Keep the same references and clones.
   found.rs:440-448 writes the new caller fields even in F6's slotless buffer.
   Gate those writes on active slots. At store/engine classification sites,
   guard diagnostic-only payload matches on Some before evaluation. A no-op
   inside mark cannot skip evaluation of its arguments. Keep F6 phase events
   and its existing formatter intact.

**Erasure and ownership audit.** In the two shared files the added storage,
imports, captures and observations are test-gated. The three fixture files
inherit their test gates. Erasing the test arms restores the original guarded
match and fallback, including discarded late payloads without bindings.
After erasure the map_err closure blocks yield the same error constants or
match on the same owned error. The proposal success arm introduces a block
around the same reply match. Its outer success payload is unit, the responder
is submitted once, and the inner reply is consumed before the same permit drop.
The extra receive tail blocks reduce to rx.await.unwrap_or on the owned receiver
with no intervening local destructor. The owner lock ends before the caller
await. Both store locks end in the original inner closure, then the store Arc
and permit drop in order before completion. No new engine/store handle or
permit escapes in a diagnostic handle. I found no non-test resource-order
regression in those hunks. This is a source-level judgment, not expanded MIR.

The founder clock guard and each guard caller clock guard still run once.
The three owner `>= ||` checks and final `< &&` send check retain their
operand order and short circuit. Neither side may be resampled to explain an
earlier decision. The added clock reads for active slots can perturb timing.

**Footprint, advisory.** The five-file diff is +1121/-53. The fixed slot array,
bounded writer and two immutable guard lanes earn their place. Repeated
qualified mark calls, double cfg inside cfg-only arms, two shadow async
receivers, and consensus-wide visibility for caller-only helpers do not.
Use local cfg-gated aliases, observe the existing receiver completion once,
and narrow caller-only helpers such as phase_reply to the found module.
Its raw index has just two valid call-site constants. Keep those private.
Prefer deleting redundant machinery in touched hunks. No new trait, adapter,
schema, file, production API or general cleanup framework is needed.

## Spec

1. **Blocking, guard attribution.** consensus.rs:713-752 records both
   ready-after-deadline and elapsed wait as CallerExpiry/Expired. Keep the
   existing arms and single guard clock. For example, encode late received
   payload as CallerReply/Expired and wait elapsed as CallerExpiry/Expired.
   Keep closed receive distinct and on-time receive independent of its inner
   result. In the three early owner stops at 536, 591 and 634, Unknown
   in LinearizerReply, AppliedReady or BlockingJoin actually means stopped
   *before* that operation. Label those specific encodings as such or leave
   unperformed edges empty. Never count these writes as helper latency. The
   individual expired/closed operand may stay Unknown.
2. **Blocking, leader wait.** found.rs:210ff deduces ReplyError from the mapped
   phase exit. At found.rs:999ff inspect the original metrics wait error in the
   existing map_err before returning Unavailable. Use bounded reasons for the
   distinctions supplied by that error (e.g. timeout if typed as timeout).
   Keep one slot-11 publication per path, including success if recorded. This
   requires no new wait, clock or engine query.
3. **Blocking evidence gap.** The formatter test sets ReadyLate itself. The
   precedence test verifies string order and a saved scalar. Both would pass
   with swapped live decision tags or a cleanup unwrap in a victim. Keep the
   useful packing, saturation, detached-lane, frozen-copy and sink checks.
   Couple acceptance to the real sites: a focused static mapping check can
   inspect the actual founder/guard arms, their returned expressions, the
   pre-map leader error and F10's explicit drop before shutdown. Inspect the
   three returned-Err cleanup branches for ordering and absence of a secondary
   unwrap on a fallible cleanup result. A formatter-only test or an artificial
   timeout is insufficient evidence for those properties. No engine execution
   is required to correct this check coverage.

   Two cheap improvements within the existing pure cases: enter a real pending
   phase before asserting F6 worker() is None, and use a retained first-call
   handle during a second capture to verify its write cannot appear there.
   Comparing a live Arc to a freshly allocated one at found.rs:684 exercises
   allocation identity only. A private static wiring check can stay outside
   the five-file patch.

**What the first natural failure could establish.** found.rs:1049ff marks
InnerOnTime, ReadyLate and WaitElapsed inside the actual selected arms. Issue
admission Busy precedes that match. The final event retains the pending phase
when work times out. Initializer errors are borrowed before collapse. Proposal
separates submission failure, forwarding, other inner failure and closed reply.
Guard linearizer forward/quorum/fatal and both ready-error layers are distinct.
The between-phase MissingRaft has its own caller scalar, subject to the lookup
correction above. A late result can supersede an inner failure. Read the outer
tag, phase events and relevant lane together.

Release stores from the three instrumented helpers go to fixed lanes selected
before spawn. Legal preflight edges map to 0..9, phase replies to 10/11,
proposal to 12..19 and guard edges to 20..33 or 34..47. The lane never changes
inside either worker. Actual alternatives for a terminal word exclude one
another within one call. OwnerSend/Pending records intent before send.
Worker enter, blocking submit/start, mutex acquisition and store result can
bound intervals. A gap does not separate queuing, descheduling or lock wait.

**Optional rows.** Successful gate, preflight/proposal caller receive and
successful blocking-helper reply have no dedicated timestamp in this patch.
If root accepts these as Unknown, adjacent real edges still localize a primary
refusal. Exact gate, delivery or helper-return latency then stays unavailable.
If retaining a success row required by the brief, write it at the existing
successful operation, once. Do not synthesize it from the next edge. A missing
store reply plus BlockingError cannot identify a join failure because
adapter.rs:26 collapses both error sources. No adapter change is warranted.

**Cutoff and cleanup.** The return tag is selected in found. Capture awaits
return, takes 48 sequential Acquire loads, then samples the end marker before
cleanup. A worker can publish after the decision during that interval.
These are return-time observations. Missing, saturated or racing rows stay
Unknown. Caller event times use the issue deadline origin, worker times use
the capture origin. Raw microseconds do not establish a cross-origin order.

At initial query/proof refusal, the node exists and blocking/TLS work has yet
to start. Both fixtures save the Copy error and snapshot, clear/drop the router
once (record poison), attempt shutdown once at the original absolute 40s bound,
record weak-store/log status, emit, then panic with the saved founder error.
A failed shutdown return or cooperative timeout cannot replace that error in
these branches. Their Ok paths move the leaf into the original TLS/proof flow,
retain later assertions and the query's handle disposal. Disposal proves no
join. F10 records a returned shutdown error before enforcing the founder
refusal. Its outer 20s timeout can still preempt emission. Later proof panic,
startup failure or cancellation carries no universal cleanup guarantee.

## Security and evidence

The recorder owns scalars, Instant and an Arc of atomic slots. No key, CSR,
leaf, state, permit, ticket, Raft handle, callback, process identity or path is
retained there. Fixed ASCII output uses closed classes and integers, after
timed founder work. The writer reserves 13 bytes for the sticky truncation
footer and emits its own newline. Three 8192-byte records plus the fourth
512-byte pair bound new diagnostic records to 25088 bytes per job. Only the
three victim calls opt in. The fourth case samples file-free endpoints only.
Four unique visible pairs from the same job/attempt and consistent wall/local
durations can support recorded scope intersection if max(start_ms+1)<min(end_ms),
conditional on a common stable wall clock. Hidden success output, regressions
or inadequate margin leave overlap Unknown. A first observed refusal can
still be localized. Cross-process causal ordering and host load remain unknown.

**Actual here.** Inspected five source diffs, unchanged prior-review blobs,
capture sites, slot writers, proof continuations, F10 macro, leaf and blocking
adapter. Static checks matched the three founder arm bindings, two identical
guard expiry encodings, ordered business-clock/drop tokens in the four changed
helper ranges, and F6 formatter text. Five candidate blobs passed UTF-8/LF
and diff whitespace checks. All eight retained log hashes matched.

**Reused only.** Builder's initial Clippy failed, mechanical batch 1/1 used,
final Clippy and four pure tests passed. Fmt, guard8 and sampler60 passed
before that batch. No final-source rerun of those three is evidenced. No
Rust/build/test, engine, mutation, network, hosted gate or failing case was
run for this review. Historical Ubuntu's three Unavailable returns preceded
query/proof or occurred at F10 final issue. PR Windows/macOS and parent
main05b9c829 were green. No candidate reproduction, host cause or fix credit.

**PROGRESS proposal only.** Astra 6.2 high exact-source review complete,
revision required. Root independently dispositions these findings before any
source correction grant or publication. Preserve the three actual victim calls
and the normal full-suite first-attempt carrier. PR176 held, last merged PR177,
C invalid/T unrun, Sol 3/3 exhausted, historical Opus 0/2 unchanged, M2 open
and M3 unchecked. Hardware paths **unverified on real hardware**.
