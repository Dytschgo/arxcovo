# M3 Ubuntu founder branch observation: implementation contract

Sol 6.1 / high / DESIGNONLY / 2026-11-18; assigned base
`a54a066198951d9dba6530bf4e408e890ca65d5f`; no children.
Only this brief is authorized here. No source/patch, build, test, runtime,
network or public action. PROGRESS text below is a proposal, not an edit.
Read with AGENTS, KICKOFF, ADR0005, m3-owned-anchor-query.md,
m3-founder-joiner-tls-proof.md, and ../reviews/m3-founder-query-astra62-review.md.
Engineering/Debug skills applied within this boundary: retained natural red is
symptom evidence; deterministic phase loop remains open. No hypothesis/fix credit;
Debug reproduction, injection and runtime phases are deferred by explicit scope.
## Baseline and decision

Exact PR176 source: `f1bf99bcbb3e18656879d6ce5cbf24e367c6056a`.
Read-only `git diff <that SHA> HEAD -- crates` is empty at assigned base.
Ubuntu CI44213815251 refused found at anchor_query_tests.rs:521 (new query),
anchored_join_tests.rs:67 (existing proof), found_tests.rs:552 (final F10).
All returned Unavailable; phase unknown. The query never reached its query.
owned_query_timeout_shutdown_retains_submitted_resources was also in flight;
runner completion times establish neither phase overlap nor contention cause.
Parent main05b9c829 / CI44212212325 separately green; PR Windows/macOS passed.
Adopt Astra's actual-branch/worker diagnosis; reject artificial F10 T now.
C remains invalid; T unrun. No control/treatment pair or causality claim.

Root carrier: ONE new reviewed diagnostic source revision, existing normal
three-OS CI, existing Ubuntu verify full suite, first attempt only. This deliberately
retains suite/load instead of Astra's narrowed four-case selection. Other OSs
supply supporting evidence only. No manual rerun/dispatch/new workflow, filter,
job, runner, timeout, quarantine, output-setting, retry or concurrency change.
Disposable dedicated Ubuntu job does not establish absence of host load.
## Frozen scope and gates

Future source allowlist (relative to crates/arxd/src/cluster/):
consensus/found.rs (existing observer, preflight, founder guard, final branch);
consensus.rs (only propose_response and fresh_read observation statements);
consensus/anchor_query_tests.rs (named initial call and fourth-case markers);
consensus/anchored_join_tests.rs (named initial call and refusal cleanup);
consensus/found_tests.rs (only f10_drop_bounded final call/refusal cleanup).
No new source file, dependencies, node fields, API for production or framework.
Existing F6 capture/output stays unchanged. Only three named calls opt in to new
slots. Fourth case gets one bounded start/end marker pair, no altered behavior.
All earlier F10 drops, Busy checks, gate release/join, drain and uninitialized
assertions remain. No new gate, delay, yield, task, wait, retry, fixture or fake clock.
Freeze all other source, Cargo/lock/toolchain, workflow/helpers/settings and guards,
including tools/ci/test_windows_cluster_quarantine.py. assert_issuance_required
allows ONLY #[cfg(test)]: no cfg(not(test)), cfg_attr or platform cfg in found.rs.

Every added item/local/argument/statement is cfg(test); do not duplicate functions
into test/non-test implementations. Existing signatures can stay unchanged:
read opt-in once at shared helper entry, then explicitly clone scalar handles into
existing owners/closures. No StateStore Arc, permit, ticket, key, CSR, Raft handle
or callback is added to the recorder. Preserve their acquisition/drop ordering,
blocking submission, reply, cancellation and shutdown semantics. No observer lock.
Original business clocks, Instant calls, predicates, short-circuit ordering and
number of evaluations remain unchanged; diagnostic samples are separate only.
Non-test erasure must equal baseline semantics (including temporary/drop scopes).
Stop and report a concrete scope gap if this cannot be demonstrated; no workaround.
## Concrete private Rust seam and fixed storage

Extend existing cfg(test) found::observation module to
pub(in crate::cluster::consensus); exports/types have that same restricted visibility.
Keep existing capture API for F6. Add Call::{QueryInitial,ProofInitial,F10Final},
async capture_call<T>(Call, impl Future<Output=T>) -> (T, Snapshot),
worker() -> Option<Handle>, mark(&Option<Handle>, Edge, Reason),
and emit(&Snapshot, Cleanup). No arbitrary strings/callbacks enter these APIs.
Task-local Buffer gains optional Arc<Slots> only in capture_call; pending phase
selects preflight/proposal/guard1/guard2 lanes. Handle owns Arc<Slots> plus an
immutable typed lane captured BEFORE spawn; workers never infer guard number.
fresh_read and propose_response
obtain handles synchronously before spawning; workers never consult task-local.
Other callers, including enrollment/query and F6, receive None. Guards select
separate lanes; no node-pointer/global registry, env flag or store lookup.
Slots owns [AtomicU64;48] plus call start Instant; no heap collection/locks.
Use array::from_fn; each edge has one writer, one packed Release store:
low8 reason, next48 elapsed_us+1 (zero absent), upper8 flags (saturation bit).
Acquire snapshot loads; saturate at 2^48-2 us and flag invalid saturation;
no sequence counters, retry loops or worker samples when handle=None.
Slot layout is fixed (unused edges stay zero):
- 0..9 preflight: admission, owners_gate, owner_enter, blocking_submit,
  closure_start, mutex_acquired, store_reply, blocking_join, owner_send, caller_reply.
- 10 initializer_reply; 11 leaderwait_reply.
- 12..19 proposal: admission, owners_gate, owner_enter, submission,
  submission_reply, engine_reply, owner_send, caller_reply.
- 20..33 guard1 and 34..47 guard2: admission, owners_gate, owner_enter,
  linearizer_enter, linearizer_reply, applied_ready, blocking_submit,
  closure_start, mutex_acquired, store_reply, blocking_join, owner_send,
  caller_reply, caller_expiry. Applied-ready distinguishes both nested Results.
Record errors BEFORE current map_err/unwrap_or/? collapse, matching by reference;
never move an engine error into the recorder. Owners_gate includes poison/closed/
missing engine/store; preflight store result precedes original Arc/permit drops.
blocking_join means helper reply: adapter::blocking already collapses join errors;
missing store_reply + BlockingError stays Unknown, never asserted JoinFailed.
Add a caller-only MissingRaft marker at the existing check between preflight
and initialize; it has no pending phase. Inspect the existing borrowed Option
without a second Raft lookup or changed unwrap/error semantics.
Initializer distinguishes NotAllowed from other API/fatal; proposal separates
submission failure, engine failure, forwarding, receiver closed and success.
Guard records original early expiry/closed checks without reevaluating them;
caller_expiry captures its original timeout/late/closed branch, not worker failure.
Reason enum is closed: Pending, Ok, Busy, OwnersPoison, OwnersClosed, MissingRaft,
MissingStore, MutexPoison, StoreCorrupt, StoreOther, BlockingError, ReceiverClosed,
SenderClosed, Expired, InitNotAllowed, ApiOther, Fatal, Forward, NoQuorum,
ReadyInnerError, ReadyOuterError, SubmitError, ReplyError, Unknown.
No Debug/Display payloads, leader IDs, commands, CSR/DER, keys, proofs or paths.
Existing 24 caller events stay at current capacity; worker edges add ZERO events.

Split the original final wildcard arm into Ok(_) and Err(_) without new binding
lifetimes: guarded Ok(r) still uses its ONE original Instant::now()<deadline.
Inside the selected arms record InnerOnTime, ReadyLate or WaitElapsed respectively;
never infer that tag from later samples. Keep the original returned Result.
Before returning, freeze caller events + final tag/result class + 48 slot loads
into Snapshot, before cleanup; record cutoff time and missing/saturated flags.
This is a bounded sequential snapshot, not an atomic view of all workers;
unpublished/racing edges stay unknown, never reconstructed after cleanup.
Observe guard local expiry similarly at its actual original decision arms.
No formatting, printing or filesystem read in any found timed phase.
## Save failure, clean up, then enforce original success

Only the returned-founder-Err branches change cleanup behavior. On Ok move the
leaf straight into the existing unmodified success flow; discard F10's Ok leaf at
its original statement boundary. Retain the scalar snapshot for later emission.
On proof Err save the Copy FoundError and snapshot outside the 35s work future
and return early. Both nodes exist, with no blocking/TLS work yet submitted.
Use common router clear/drop and shutdown sequence ONCE under existing absolute
entry+40s cleanup bound. On this refusal branch only, save lock/shutdown/timeout/
weak-store/log statuses without early unwrap, emit them, then fail on the original
founder Result first. Success-path handles/assertions/cleanup remain unchanged.
handles(n) removes/discards handles; do not claim it joins them.
F10 must first reach original gate release/join, drain and uninitialized checks.
On returned final refusal save it, attempt existing shutdown once under the same
20s wrapper, emit separately and enforce found first. No extra drain or gate.
Late proof panics, startup failure, outer timeout and cancellation retain original
limitations; no universal cleanup, Drop logger, catch_unwind framework or retry.
## Cross-process overlap with file-free markers

OMIT all procfs/filesystem/process readers, PID/starttick/boot/host identifiers.
Existing Ubuntu job+attempt+unique nextest case supply external same-job provenance.
At three capture entries/returns and fourth body entry/exit, record checked integer
SystemTime epoch units and each process-local Instant elapsed at the two boundaries.
Fixed one-pair record per case; never compare separate Instant origins.
If all four pairs are visible and unique, calculate intersection on the recorded
wall clock with quantization: max(start+1 unit)<min(end). Label this conditional
on a common stable wall clock. Regression, wall/elapsed mismatch, missing records/
provenance or insufficient margin leaves overlap Unknown; agreement cannot exclude
clock steps between samples. Verified monotonic cross-process overlap is Unknown.
A wall intersection describes scope lifetimes, not simultaneous found phases,
CPU use or historical cause. Hidden success output is missing evidence.
Missing overlap must not discard independently observed primary-refusal evidence.
No Windows/global process snapshot, environment read, filesystem trust exception,
new dependency, platform predicate, or process killing is introduced.
## Bounds, admission and disposition

Snapshot contains <=24 events (times clamped to u64), 48 slots and two self markers;
only bounded integer/static enum formatting. Use a fixed [u8;8192] fmt::Write sink,
reserve truncation footer, reject overflow; at most8192 emitted bytes per victim,
fourth pair <=512 bytes total, new output <=25088 bytes/job; never allocate logs.
Existing nextest output policy may hide success markers; do not change it to recover
missing evidence. Green/no overlap means no reproduction of the target joint case;
a red without overlap still localizes its branch, not the historical joint cause.
Before any push: root-set separate compile admission budget, exact-diff independent
Astra6.2 review, cfg erasure/drop/clock audit, privacy/cap and existing guard checks.
No local Windows engine. Future checks must exercise slot bounds, opt-in isolation,
actual branch tags and primary-error precedence within that separately set budget.
One full-suite first attempt is sole observation; compile failure, missing evidence,
new failure or source drift stops. No followup/control/retry/fix permission implied.
PROGRESS proposal: Sol6.1/high design only; Astra diagnostic adopted, full-suite
carrier selected, exact-diff review/compile admission pending; C invalid/T unrun;
PR176 held, M2 open/M3 unchecked, Sol3/3 exhausted/historical Opus0/2 unchanged;
no fix credit; hardware **unverified on real hardware**. Babysitter unchanged.

## Root disposition and implementation budget - 2026-11-18

Root adopted all four Astra corrections from2be92556 in this revision.
Authorize local implementation ONLY, with one workspace Clippy invocation plus
at most one mechanical import/type/lint correction batch and one Clippy recheck.
No semantic repair after a natural red. Fmt, unchanged static guard and bounded
pure-recorder checks may run once; no local Raft/Windows engine or failing proof
rerun. Exact-source Astra review precedes publication. No push/CI invocation is
included in the implementation grant. Record every failure and actual/reused check.
