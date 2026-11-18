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
worker() -> Option<Arc<Slots>>, mark(&Option<Arc<Slots>>, Edge, Reason),
and emit(&Snapshot, Cleanup). No arbitrary strings/callbacks enter these APIs.
Task-local Buffer gains optional Arc<Slots> only in capture_call; pending phase
selects preflight/proposal/guard1/guard2 lanes. fresh_read and propose_response
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

Three callers save the original Result<IssuedLeaf,FoundError> outside work.
For each proof, on found Err only, retain that Result and return early from the
35s work future; on Ok consume the leaf in the existing unmodified proof flow.
Use an outer Option<Result<...>> and take its Ok value; keep Err until enforcement.
Run existing blocking/TLS joins if present, router clear/drop, shutdown within
original absolute 40s cleanup deadline, weak-store/log and handle assertions.
Save each cleanup outcome without immediate unwrap; continue remaining cleanup
within the SAME bound, report fixed scalar statuses separately. No replacement
shutdown/drain or deadline extension. Startup absent remains cleanup Unknown.
F10 saves final issue Result, then saves existing shutdown Result within its
unchanged 20s wrapper. Its original released/uninitialized checks already ran;
do not add another drain or assert capacity after shutdown as a substitute.
Emit the frozen snapshot and cleanup status after timed work/cleanup, then enforce
saved found success FIRST, then cleanup outcomes, then original work timeout.
Cleanup failure cannot mask found Err. On success all original assertions remain.
This covers returned found Err only: later proof panics, cancellation, startup
panic and outer timeouts can still bypass cleanup/emission. No universal panic
cleanup promise, Drop logger, catch_unwind framework or hidden retry.
## Cross-process overlap with bounded self-only evidence

At three capture entries/returns and fourth test body start/end, outside found,
read host uptime centiseconds from /proc/uptime, boot identity from
/proc/sys/kernel/random/boot_id and own /proc/{process::id()}/stat start ticks.
Concrete reader: File::open + Read into fixed [u8;1025], at most 1025 bytes;
reject >1024, malformed/truncated fields, symlink/reparse components and unavailable
or untrusted proc mount. No read_to_end/string, process listing, env or commandline.
Frozen-scope gap: portable std cannot certify proc mount/race safety. If root cannot
establish trusted immutable procfs for this job, emit Unknown; do not expand scope.
Keep only PID u32, startticks u64, SHA256 boot identity, uptime u64 and status enum;
parse stat after its final ')' and discard all other bytes. Never emit raw bytes.
Validate identity at both ends; same host digest and uptime clock required. Use
one-centisecond uncertainty intervals; require strict overlap beyond resolution.
All four recorded scope intervals must intersect; report pairwise otherwise.
This establishes live scope overlap only, not simultaneous found phases or CPU use.
Missing/hidden passing output, ambiguous identity, clock regression, unsupported OS
or inability to validate proc safely => overlap Unknown. No platform cfg required:
absent proc simply yields Unknown. No Windows global snapshot or process killing.
This proves only recorded intervals, never descendant lineage or absent host load.
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
