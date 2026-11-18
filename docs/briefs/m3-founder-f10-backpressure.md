# F10 actual-store backpressure preregistration (UNAPPLIED)

OpenAI / Sol 6.1 / high; design sub-agent; 2026-11-18; no children.
Preparation only. Root independently reviews exact diffs before any source execution.
No Rust, Cargo, compilation, test, probe, engine, runtime or network action authorized here.

## Pins and static scope

Assigned isolated worktree HEAD: 847ffa89d76ed6ccdf9d5015b13b1274ff4093a3.
PR176 exact source: f1bf99bcbb3e18656879d6ce5cbf24e367c6056a.
`git diff <PR176> HEAD -- crates` is empty; target bytes equal PR176 object.
Target only: crates/arxd/src/cluster/consensus/found_tests.rs, f10_drop_bounded,
after its existing released(&n)/uninitialized(&n), final issue only.
Patches are alternatives against the same base, never stacked; active source untouched.
Base target SHA256: 6c601f548eed3e02c43adc48a0bdec18f958d28e70d6a334a39ff2a4874408a5
Control patch SHA256: 81f8869a530ea9c4a183bec278c7d73292fd4e57c329335998114c57b6311a49
Control resulting target SHA256: e65958bd34980c94dc0a07ec2d7929ddda8df635ad4678d96eaa1d990af44de2
Treatment patch SHA256: 504f59fc6eadada2b13de77d4be1154a3a36304eecc06fa4f553c2f7fb521283
Treatment resulting target SHA256: 593466c22e64e0dea5b2d2a163be43a644a49f3301657b5cdbbd3cde799364ce
All active crate files baseline count: 132; canonical manifest SHA256:
de6c0835a62b0786f96ad6022ec52984b1458e93b62f21b9ff2bb7cbe6d103a3
Manifest = sorted relative POSIX path + NUL + byte SHA256 + LF per file.
Static PASS: both git apply --check; in-memory reconstruction/hashes/scope/cfg tokens;
C/T only-drop-position comparison; ASCII/UTF-8 LF; all132 crate byte hashes unchanged.
No compiler/type-check claim. Exact filter resolved from module bindings/static evidence.

## Mechanism and feasibility (static only)

Reuse existing real Gate::new(&n): readiness follows actual store mutex acquisition;
Drop sends release and joins its private thread. Gate holds an Arc, not an n borrow.
Reuse existing reviewed parent observation::capture around the identical final issue.
Child access is legal by module ancestry; capture returns owned Result<IssuedLeaf,
FoundError>; IssuedLeaf has no n lifetime. No stored future spans mutable shutdown.
C acquires then drops/joins Gate before capture; T drops it after capture returns.
The variants differ ONLY in that single drop(gate) statement's position.
No new clock calls. Five bounded scalar emissions occur outside timed found:
acquired; release/join checkpoint; drained capacities; primary flags; shutdown flag.
The release checkpoint is emitted after capture in both cells, not a release timestamp.
Existing observer retains DEBUG-F6-OBS prefix; exact F10 filter identifies this trace.
Save found Result, release/join, await released, snapshot issue4/read8, await shutdown.
Report primary scalar status then saved shutdown scalar status; assert capacities;
original success unwrap on saved found Result precedes saved shutdown unwrap.
Thus expected red occurs after verified successful cleanup, never while Gate is held.
Shutdown failure remains visible even when primary unwrap panics; invalidate that cell.
No Drop/lifetime conflict found by inspection; compiler feasibility is UNVERIFIED.
A compile/drop-lifetime error is a stop/report finding, never license for a workaround.
Earlier eight drops/Busy/owner/Arc/assertions, uninitialized, four workers, 20s wrapper,
5s issue budget, signer clocks, Owners/permits and all helper/settings bytes preserved.
No globals, env knobs, sleeps, fake clocks/errors, private flags, store mutation,
production hooks, helper/settings changes, quarantine, retries or timeout increases.

## Future admission (not execution permission)

No qualifying Ubuntu runtime is established. This may design a future local Windows
model under the prior PC grant, ONLY after independent root review and separate
execution authorization. Do NOT run the Windows engine under this preparation grant.
It is neither Ubuntu evidence nor hardware/natural-host causality evidence.
If a dedicated Ubuntu environment later qualifies, root must preregister its admission;
this document does not authorize moving execution there or adding another pair.
Use the exact pinned source plus ONE reviewed variant per fresh private storage cell;
checked non-reparse offline cached toolchain1.95.0/nextest0.9.151, isolated cache/target,
no competing build/engine. Record exact source/config/tool/artifact hashes first.
One compilation per cell, no mechanical corrections; any compile/admission red stops.
Future command shape (resolve validated absolute offline paths before admission):
`cargo nextest run --offline --locked -p arxd --lib`
Exact filter: `-E 'test(=cluster::consensus::issue::found::found_tests::f10_drop_bounded)'`.
Fixed settings: --test-threads 1 --retries 0 --fail-fast --success-output final
--failure-output final. No config/env overrides, old tests, F6/F7/F8 or extra cells.
Root must inspect actual cached config/CLI compatibility before authorizing execution;
no discovery/list/probe invocation is granted now. Any mismatch stops; do not adapt.
Run C once only; admit T once only after C fully satisfies its oracle. No repeated
natural green control, post-restore run, follow-on injection or stability sampling.
Record process exit, test and whole-command elapsed separately; bounded sanitized
traces and hashes only, never paths, CSR, cert, key, private data or Result Debug.

## Preregistered outcomes and stop rules

C: exit0; saved found Ok; 14 events, overflow=false; six phase enter/exit pairs
(preflight, initialize, leaderwait, proposal, guard1, guard2), all exits Ok;
finaldeadline enter Pending/exit Ok, pending=none at exit; exit elapsed<5000000us,
remaining>0.
No completed phase span >=5000000us; no missing/reordered/extra events.
T: exit100 ONLY at saved found success unwrap, exact Err Unavailable (not outer
Elapsed or another refusal); overflow=false/count3, finaldeadline enter Pending,
preflight enter Pending; no preflight exit; finaldeadline exit Unavailable with
pending=preflight, elapsed>=5000000us and remaining=0. No later phase entry/exit;
no other phase >=5s. Pending identifies caller expression, not worker completion.
Both: acquired/released_joined/drain_completed/shutdown_completed markers1,
issue_free4/read_free8 and shutdown_ok=true BEFORE original success unwrap.
C primary_ok=true/unavailable=false; T primary_ok=false/unavailable=true.
Missing/overflow trace, wrong result/phase/span/count, C red, outer timeout,
cleanup failure, surviving work or any assertion mismatch: stop, preserve evidence.
Expected T red ends experiment; only a treatment-specific refusal oracle is supported.
It cannot attribute the hosted natural failure, establish a reproduction rate, or fix it.
No repair/rerun/tuning: report exact failure and hand back to root for disposition.
Restore only the reviewed patch after verifying patched hashes; compare every active
crate byte to pinned baseline. Drift/survivors stop; no cache deletion/process killing.
No runtime command, source patch application, compilation or behavioral check ran here.

## Coordinator PROGRESS proposal (do not edit PROGRESS here)

Sol6.1/high prepared exact unapplied F10 C/T patches and preregistered local-model
oracle; independent root diff review and separate execution admission pending.
Debug phase1 remains open; phases2-6 skipped because runtime is expressly forbidden.
No reproduction, cause, fix, gate/recovery credit, PR or public action follows.
Shared Sol3/3 exhausted and historical Opus0/2 unchanged; no attempt consumed/reset.
PR176 red/held; parent main05b9 separately green last known; M2 open/M3 unchecked;
hardware **unverified on real hardware**. Keep existing babysitter unchanged.
