# PR176 founder refusal: independent stuck-path review

**Hold PR176. The natural refusal phase and cause remain unknown.** The Ubuntu
job establishes three failures in real founder issuance. The most informative
next measurement would capture the first refusal under the observed concurrent
workload, at the branch that produces it. A deliberately locked store selects
that branch in advance.

OpenAI / Astra 6.2 high, single independent reviewer under the owner's stuck-review
exception. Static review only. Base `05b9c82930cd3812614fc20f5d03d0a5fa05952a`,
PR head `f1bf99bcbb3e18656879d6ce5cbf24e367c6056a`, checkout `4b67adb9`.
The checkout adds only documentation after the PR. The five query feature paths
match `f9d0a413`, including the test inventory. No runtime checks in this review.

## What the retained evidence proves

Ubuntu CI44213815251, raw log SHA256
`040328dacf0d441ac1229c380ee091d0b456f9f4ff9d25c1ebddfa7650786f5e`.
All three panics unwrap an inner `Err(Unavailable)`. Times cover whole tests.

| PR176 call site under crates/arxd/src/cluster/consensus/ | Seconds | Last established boundary |
| --- | ---: | --- |
| anchor_query_tests.rs:521 | 10.058 | Initial found, before mint, TLS, query or proof |
| anchored_join_tests.rs:67 | 15.348 | Initial found, before existing preproof flow |
| found_tests.rs:552 | 15.303 | Final F10 issue, after gate release/join, read drain and uninitialized assertions |

F10 had dropped eight admitted callers. The final call was the first opportunity
to initialize. Reaching its unwrap leaves open whether initialization began or
committed. The first panic preceded the runner's cancellation notice. The other
two and a passing 24.814s durable query/shutdown case were in flight. Log
delivery times do not establish overlapping *phases* or an injected cancellation.
PR Windows/macOS passed. Parent main's first CI44212212325 passed (608s wall,
605s charged, Windows 604s, 13 founder cases per OS). Earlier main `09134085`
failed Windows F6/F8 at initial issuance. F7 missed its expected clock refusal
without printing its actual variant. A shared call path does not establish a
shared cause. See retained dispositions `20870818` and `30185388`.

## Standards

1. **Cleanup evidence is lost on precisely these failures.** Both single-voter
   tests unwrap inside the future passed to `timeout_at`. Panic unwinds past
   router clearing, explicit shutdown and weak-reference checks. F10 likewise
   unwraps before shutdown. The 35s work/40s total proof bounds and F10's 20s
   wrapper cannot catch a panic. This is a test failure-path defect. It prevents
   certification of teardown and could affect elapsed time after the refusal.
   It cannot explain the preceding return value. A process exit proves no
   in-test join. Preserve the primary error even if cleanup independently fails.
2. **The phase observer misses the failing calls.** Only F6 scopes the task-local
   buffer. Even when scoped, a phase entry describes a caller expression. Owners
   and blocking workers do not inherit that scope. A mapped guard error loses its
   read cause, and the final event combines an elapsed timer with an already-ready
   result rejected at the strict return check. Later clock sampling cannot
   distinguish those branches. CSR parsing, signing and clock checks sit between
   phase pairs. A pending label alone cannot diagnose a mutex wait.

## Spec

1. **Ubuntu integration proof is incomplete.** ADR0005 and the owned-query brief
   require real-engine proof. The new proof never reached its query. In
   `consensus.rs::start`, each node creates its own four-unit issue semaphore.
   `found` still borrows one unit with immediate admission. A failure there
   returns Busy. The new query clones that pool, acquires issue then read, and
   moves both owned units and the store Arc into submitted blocking work. The
   store guard drops before those resources and before reply. No query ran on
   any failing node before its refusal. Separate nextest processes cannot share
   those semaphores. Host scheduling and durable I/O remain shared resources.
2. **Caller refusal and engine outcome can diverge.** In `found.rs`, preflight
   uses an owned read unit and an Owners task that submits a blocking store
   snapshot even if the caller drops. Eight retained reads bound the strays.
   F10's drain means the closures released their read units. It does not join
   every async owner. On the subsequent issue the 5s budget starts afresh.
   Initialize, leader wait with remaining time, and proposal precede two fresh
   authorization guards with separate 1500ms read budgets. Proposal owns its
   own unit and may finish a write after its receiver drops. Each guard can
   refuse before 5s on linearizer, readiness, store or receiver failure.
   The outer deadline also rejects late completion after inline crypto. Thus
   Unavailable proves neither deadline expiry nor rollback. A closed or poisoned
   Owners lock path, failed blocking join/store read, or closed oneshot can also
   return Unavailable in preflight without exhausting the budget. Partial founding
   after initialize is an explicit non-resumable case in the founder brief.
   No specific budget, ownership regression or underlying engine error is
   established by the three unwraps. Retrying on the same node changes the case.

## Security

The local query validates the 32-byte input. Its pending-code lookup and bounded
voter list are advisory. TLS possession and code-anchor matching precede proof
release. The signer still requires a committed ticket and two fresh guards. No bypass identified
on this path. A diagnostic needs only test labels, relative times, fixed error
classes, phase and cleanup flags. Never emit result payloads, CSR/DER, keys,
proofs, store contents, raw pointers, private paths or process command lines.
The private harness reads the entire command log before its 8 MiB assertion
and prints a sanitized tail. Bound reads before allocation and emit allowlisted
ASCII status only. Keep raw evidence private.

## Diagnostic validity and process attribution

The exact C/T patches add a second Gate at F10's final issue. Only the position
of its drop differs. In T the holder cannot release until found returns. If
preflight runs, its store read cannot complete. The predicted red therefore
mostly repeats neighboring `f10_outer_budget`, which already holds that mutex,
expects Unavailable, checks retained capacity, releases it and then founds.
T's predicted three caller events would also fit a preflight worker never
scheduled within 5s. T could calibrate an observer against a known obstruction. It cannot explain
a refusal after the original holder has joined, or in either gate-free proof.

The private harness and C artifacts agree with review `3320f566` and result
`7a5302c9` retained at `aac1fede`. One Windows nextest C compiled and exited 0,
14 ordered events, all six phase exits Ok, final Ok at 34.021ms. Gate joined,
issue/read free 4/8 and shutdown Ok. The release marker is a late checkpoint.
The wrapper's cp1252 print error followed the saved child outcome. All 132
crate files were restored. The post-command snapshot recorded two cargo names
and one rustc name. **C invalid under its grant. T unrun.** No pair verdict.

`run_cell.py::processes` projects matching processes to names. It checks before
launch and after `p.wait()`, records no root PID or start identity, and loses
ancestry across parent exit. Its regex already matches the hashed test basename
(`arxd-648de2072896f9ec.exe` without extension in ProcessName). The recorded
artifact hash identifies a file. Neither match identifies a process instance.
Other descendant names escape the filter. `judge_restore.py` rejects any
nonempty snapshot. The later cargo PIDs 67088/40220 from pwsh 61076 started
after C. They cannot identify the earlier three entries. No kill was performed.

For a separately authorized harness, record a bounded prelaunch inventory,
root launch handle, PID and OS creation UTC. Retain start/exit identity for
descendants including nextest, rustc, build helpers and the hashed test image,
through root exit and the bounded final snapshot. Match the expected test image
to the private target artifact locally. Traverse parent PID plus parent creation
identity at the child's start. Require parent lifetime to cover child birth and
reject reuse or ambiguous timestamp resolution. Record start/end observation
bounds. A final snapshot cannot reconstruct exited intermediate parents.
Classify each candidate as owned, demonstrably external, or unknown. Record
owned survivors and competing external work separately. A missed event, access
denial, incomplete chain or interval gap stays unknown. Stop on unknown or
admission violation, preserve evidence, and leave every process alone. Separate
wrapper, command, oracle and restoration statuses. This prospective contract
cannot change C's original admission rule.

## Proposed feedback loop for a separate decision

Use the retained natural red as the symptom control. Prior isolated greens add
no missing phase information. The smallest workload justified by the log for
testing *cross-process overlap* is the three named victims plus
`owned_query_timeout_shutdown_retains_submitted_resources` as the observed
fourth case. It exercises real store/shutdown work in its own process. Its
duration suggests an overlap worth testing, without identifying the bottleneck.

- **Treatment first, if separately authorized.** One diagnostic Ubuntu nextest
  selection of those four exact cases at PR176, one attempt, original engine,
  storage, worker counts, deadlines, assertions and default runner concurrency.
  Keep both proof flows and all of F10's original setup. No additional gate,
  artificial delay or retry. A narrow selection changes suite order and host
  load. Record actual process overlap on one host clock. Do not assume it.
- **Probe.** Expose a test-only capture wrapper to the sibling proof modules
  and scope only the three target found calls. Use the existing bounded
  phase buffer and tag the actual final decision (inner result on time, elapsed
  wait, or ready result rejected late). Preserve pre-collapse error classes.
  For worker edges use separate fixed per-call timestamp slots so the existing
  24-event caller buffer cannot overflow from the added subphases. Pass them
  explicitly to the owner/closure. Mark preflight owner entry, blocking
  submission, closure start, mutex acquisition and completed read.
  For each guard distinguish linearizer reply, applied-ready, store result and
  caller expiry. Mark proposal submission/reply and initialize result. Capture
  missing edges as unknown. Snapshot at refusal before teardown, emit outside
  timed work. Save the original result, attempt existing bounded cleanup, report
  its status separately, then enforce the original success assertion. Timestamp
  reads can perturb scheduling. No synchronous printing or waiting in timed work.
- **Interpretation.** A natural inner Unavailable with a completed mapped error
  before expiry selects a different investigation from a pending worker at 5s
  or a completed guard that missed 1500ms. A worker started without an observed
  acquisition localizes an interval only. Descheduling can occur in that interval.
  If a load comparison later becomes material, the matched control is the same
  three victims and recorder with only the fourth process omitted. It requires
  its own decision after a localized red. One contrast cannot estimate a rate.
- **Stop after the first invocation, whatever the outcome.** Preserve the first
  red and its phase, branch and cleanup evidence. Green means no reproduction.
  Missing overlap, overflow, another failure, unknown teardown/lineage or source
  drift leaves the corresponding question unresolved. No automatic control,
  rerun, fix attempt, settings change or quarantine. No action authorized here.

**Coordinator PROGRESS proposal.** Record Astra 6.2 high independent static
review, natural failure unresolved, invalid C and unrun T, and the proposed
four-case Ubuntu diagnostic awaiting a separate decision. PR176 stays held.
Sol shared fixes 3/3 exhausted, historical Opus 0/2 unchanged. M2 open, M3
unchecked, v1 incomplete. Hardware paths **unverified on real hardware**.
