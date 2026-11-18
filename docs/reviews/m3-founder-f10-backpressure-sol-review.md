# F10 C/T backpressure: independent static Sol review

OpenAI / Sol 6.1 / high; 2026-11-18; assigned isolated worktree; no children.
Reviewed clean HEAD: 78492769f9263b5467aeff4f873a3e4bc672d005.
Source pin: PR176 f1bf99bcbb3e18656879d6ce5cbf24e367c6056a.
Read AGENTS/KICKOFF, debug skill/profile, F10 brief and both exact patches,
Ubuntu disposition30185388 (unchanged at HEAD), F6 observer safety, hosted and
binding-correction reviews, CONTRIBUTING and relevant ADR0005/0023/0024.
Task static-only scope supersedes debug runtime phases: phase1 remains open;
phases2-6 are not performed. No natural Ubuntu reproduction or cause claim.

## Standards: PASS, 0 blockers

The alternatives replace only F10's final issue/shutdown after the original
released/uninitialized pair. Identical surrounding bytes preserve eight drops,
Busy checks, owner/Arc/sign assertions, helpers, four workers,20s wrapper,
5s issuance budget, signer clocks, Owners and permit policy. No product fix.
C/T differ ONLY in one drop(gate) position; never apply them as a stack.
Brief's design worktree pin847ffa89 is historical; this review pins78492769.

Actually ran both git apply --check --whitespace=error-all: PASS, no application.
In-memory hunk/context/count reconstruction and C/T comparison: PASS.
Base target SHA256:
6c601f548eed3e02c43adc48a0bdec18f958d28e70d6a334a39ff2a4874408a5
Control patch/result SHA256:
81f8869a530ea9c4a183bec278c7d73292fd4e57c329335998114c57b6311a49
e65958bd34980c94dc0a07ec2d7929ddda8df635ad4678d96eaa1d990af44de2
Treatment patch/result SHA256:
504f59fc6eadada2b13de77d4be1154a3a36304eecc06fa4f553c2f7fb521283
593466c22e64e0dea5b2d2a163be43a644a49f3301657b5cdbbd3cde799364ce
All132 active crate files independently equal PR176 bytes; no reparse points.
Canonical sorted POSIX path-NUL/byte-SHA256/LF manifest:
de6c0835a62b0786f96ad6022ec52984b1458e93b62f21b9ff2bb7cbe6d103a3
First helper used Windows Path ordering and rejected the aggregate; corrected
POSIX string ordering matches the pin. Individual byte comparisons passed first.
Patch/reconstructed target ASCII UTF-8 LF checks passed. No compiler claim.

## Spec: PASS, 0 blockers; bounds and evidence advisories below

Gate::new clones the store Arc, then its private OS thread locks the actual
store mutex and signals readiness. Synchronous ready receive proves acquisition.
No borrow of n is stored. Drop sends release, then joins that thread; successful
return proves its guard and Arc have dropped, NOT that preflight has completed.
Poison, channel failure or thread panic can panic; such a cell is invalid.

capture awaits issue inline, no spawn or 'static requirement. Child module
ancestry permits observation access. IssuedLeaf owns Vec/node/commit, with no
n lifetime; FoundError is Copy. Saving the owned Result leaves no node borrow
across mutable shutdown. Static feasibility found; compilation remains unrun.
Task-local RefCell state is scoped per poll; no borrow crosses await and spawned
Owners work does not inherit it. Existing macro result bindings and outer ?
retain outcomes; proposal's inner ? can skip its exit on other paths.

C joins Gate before capture. T keeps Gate through capture, including trace
formatting/output AFTER found returns; only then releases/joins it. T's timeout
drops the caller work/receiver and issue permit, not its submitted owner task
or non-abortable store closure. That closure may be queued or mutex-blocked;
preflight entry alone cannot establish which. It can finish after gate release.

Post-gate released waits for read8, then snapshots issue4/read8. Permit recovery
proves closure resource release, not async owner join: tx send can still follow.
shutdown closes admission, joins engine and owner set, drops node store handles,
and waits for store-release flags. shutdown_ok=true plus capacities and successful
Gate join is the required normal-path cleanup evidence. A released marker alone
is insufficient. No new hold, abort, detached task or worker cancellation added.

Ordering: save primary; Gate release/join; read drain; capacity snapshot; await
and save shutdown; emit primary/shutdown booleans; assert capacities; original
primary success unwrap; saved shutdown unwrap. Thus shutdown Err is printed
before deliberate T primary panic, but its unwrap is skipped if primary fails.
Capacity assertions, drain timeout, Gate panic or outer cancellation can precede
and mask the primary unwrap; treat these as invalid cleanup, never expected red.
No claim of successful cleanup follows merely from shutdown_completed=1.
Cancellation/panic before capture returns can discard buffered trace; no fallback
emission or forced join is added. Stop, preserve evidence; no repair/rerun.

T count3/preflight-pending is feasible and compelled on the admitted normal path,
not merely a hope of prompt scheduling. Inspected cached pinned Tokio1.48.0
Timeout::poll: it polls work BEFORE delay, even if deadline already elapsed.
found has no suspension between finaldeadline enter and that poll; work records
preflight enter before preflight's first await (rx). Cooperative budget can delay
rx/owner progress but cannot skip that entry. Held mutex prevents preflight exit;
timer refusal records finaldeadline exit Unavailable, pending=preflight: 3 events,
no later phase. Earlier admission failure/panic/cancellation invalidates the cell.
No claim that the owner/closure was polled or finished follows from this count.

C requires all14 ordered events: final enter, six phase enter/Ok exit pairs,
final Ok exit/pending=none, no overflow. T requires exact saved Unavailable unwrap,
exit100,3 events/no overflow, final elapsed>=5000000us/remaining0. Both require all
cleanup flags and capacities; C exit0 admits at most ONE T. These are acceptance
oracles, not guarantees that a run completes successfully on an arbitrary host.
Advisory: C's positive remaining microseconds is stricter than found's earlier
Instant predicate: observer overhead or truncation can yield0 after a valid Ok.
Such a mismatch still stops; it cannot be silently relaxed or rerun.

Advisory: 5s issue/2s drain/5s shutdown/20s outer are cooperative async budgets,
not hard wall-clock caps. Gate acquisition/join and synchronous output cannot be
preempted by outer timeout; store/scheduler stalls can overrun them. Successful
normal-path joins prove cleanup; an outer timeout does not. Future root admission
must retain this qualification and reject survivors without unrelated killing.
Markers are late booleans/checkpoints, not timestamps. C's release marker follows
capture; drain/shutdown flags follow their awaits. Do not infer elapsed gate hold,
release instant or phase durations from marker/log emission order. Existing
monotonic samples describe caller expressions; no natural hosted causal inference.

## Security: PASS, 0 blockers

Five added emissions contain only fixed labels, booleans and bounded capacities.
Existing observer caps24 events, scalar timings/static variant names; no Result
Debug, paths, CSR/DER, keys, proof, store contents or private owner data emitted.
Gate/cleanup introduce no production hook, unsafe code, settings or approval flow.

## Readiness and retained status

READY for root to consider a later separate grant, with the bounds advisories.
This review grants no runtime. Future scope: ONE C and conditional ONE T local
Windows MODEL under prior PC grant and root admission, not Ubuntu acceptance.
No Rust/build/test/engine, active patch/source edit, network/public action or
PROGRESS edit occurred. Only this review is committed; no fix attempt/reset.
Supplied terminal PR176: Ubuntu FAILED; macOS/Windows SUCCESS; no refresh/rerun.
Sol3/3 exhausted; historical Opus0/2 unchanged; M2 open, M3 unchecked.
Hardware paths **unverified on real hardware**. No recovery, merge or cause credit.
