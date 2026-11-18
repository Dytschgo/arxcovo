# PR178 Windows founder deadline disposition

Astra 6.2 / high, independent stuck-path review, 2026-11-18.
The Windows caller selected `WaitElapsed` while awaiting the founder proposal.
The proposal owner had successfully submitted `Command::Found`. The capture ends
before it establishes the responder outcome. PR178 remains held.

## Provenance and scope

Reviewed local evidence commit `6ca4f493cf71f8a421c3e6cd52adc31a258e7837`.
It adds only documentation over published `363d5320684505c15b2e3284b7c6f5a4f9ea842b`.
Cached Windows job JSON identifies CI 44262803622, attempt 1, job 134659990323,
that published head and a failed Windows verify. The checkout log identifies
PR178's synthetic merge `39cd4f3090029e8c7930afcbec56e8730b0e6920` into
`e3e79089e6c36f6483cb3db8842e5894f26ee00d`. Local git comparison confirms the
founder, proposal helper and anchored proof source equal main at the published head.

Read the retained files under `D:/Temp/arx-pr178-363d5320-hosted` (each <8 MiB).
All four SHA-256 values matched `docs/evidence/m3-pr178-windows-founder-deadline.md`.

| File | Bytes | Independently computed SHA-256 |
| --- | ---: | --- |
| windows.log | 213797 | fe582435040f10a49cf57f1df47e5a63b60c4d73f677243477e1b283d19be986 |
| windows-job.json | 6693 | 961c910f39ac98918b12b6c51447479cd7655d9161afb90531c3d69dd5daf091 |

The Windows guard, fmt and Clippy succeeded. Nextest ran 35/652, with 34 passes,
1 failure, 36 skipped and 617 unrun. `anchored_preproof_single_voter` reported
5.888 s for the whole test. At anchored_join_tests.rs:194 it panicked with the
saved original founder refusal. The proof stopped before TLS, mint and enrollment.
The three new identity tests were unrun on Windows. Ubuntu and macOS each have
three identity PASS records and successful jobs. Windows feature timing is open.

## Caller decision and worker observations

The nine caller events have no overflow. In issue-relative us, preflight ran
17..213 (Ok), initialize 447..100444 (Ok), and leaderwait 100449..4655216 (Ok).
The wait for `current_leader == Some(1)` and `state == Leader` occupied 4554767 us.
Its return time includes caller scheduling. The instant leadership changed and the
reason for that span are unknown. Proposal entered at 4655324 with 344675 us left.
Finaldeadline entered at 3 and exited at 5004265 with zero remaining, Unavailable,
and proposal pending. In the test build, `found()` tags each selected match arm:
`Ok(r)` on time yields InnerOnTime, the remaining `Ok(_)` yields ReadyLate, and
`timeout_at(deadline, work).await` returning Err yields the observed WaitElapsed.
The header reason starts at Unknown. No more specific caller reason was written.

Decoded using `consensus/found.rs` (`Reason`, `Lane::slot`, `pack`, `freeze`).
For a nonzero word, reason is its low byte (0 Pending, 1 Ok here), elapsed us is
`((word >> 8) & ((1 << 48) - 1)) - 1`, and the upper byte signals saturation.
All 48 slots are present in the log, none saturated. Worker and phase-reply slots
use `Slots::start`. Caller events use `deadline - ISSUE_BUDGET`. Use differences
within one origin only. Pending at a submission or send edge records intent.

| Slot | Edge | Packed | us from slot origin | Reason |
| ---: | --- | ---: | ---: | --- |
| 0 | Preflight Admission | 9473 | 36 | Ok |
| 2 | OwnerEnter | 24065 | 93 | Ok |
| 3 | BlockingSubmit | 24064 | 93 | Pending |
| 4 | ClosureStart | 32257 | 125 | Ok |
| 5 | MutexAcquired | 32513 | 126 | Ok |
| 6 | StoreReply | 33025 | 128 | Ok |
| 8 | OwnerSend | 43264 | 168 | Pending |
| 10 | Initialize reply | 25718529 | 100462 | Ok |
| 11 | Leader wait reply | 1191765249 | 4655332 | Ok |
| 12 | Proposal Admission | 1191772929 | 4655362 | Ok |
| 14 | OwnerEnter | 1191837441 | 4655614 | Ok |
| 15 | Submission | 1191838208 | 4655617 | Pending |
| 16 | SubmissionReply | 1191846913 | 4655651 | Ok |

All other words are zero. Preflight 1/7/9 are OwnersGate/BlockingJoin/CallerReply.
Those writers record errors only. Preflight's positive phase exit establishes
success. Proposal 13 (OwnersGate) likewise records failures only. Slots 17/18/19
are EngineReply/OwnerSend/CallerReply. Slot 19 records a closed caller channel
only. Guard1 (20..33) and guard2 (34..47) are empty. Caller phase order and the
pending proposal place the refusal before ticket validation, both guards and signing.

In `consensus.rs::propose_response`, slot 16/Ok is written inside the Ok arm of
`client_write_ff(cmd, Some(responder)).await`, 34 us after the submission marker.
The owner next awaits the `ProgressResponder::complete_only()` receiver. Slot 17
is written when the owner observes success, forwarding, other error or closure.
Slot 18 precedes its separate oneshot send to the caller. Each word is a Release
store read individually with Acquire after founder return, before cleanup. The
end marker is 5004297 us on the slot clock. Empty 17 and 18 mean neither write
was visible at its own load. The engine may already have processed the command,
the owner may have lacked a poll, or an outcome may have arrived later. The trace
cannot date engine receipt, commit, durable apply, responder readiness or delivery.
It also cannot attribute time to election, I/O, contention or cross-process load.

The refusal branch saved the error, cleared/dropped the router, awaited bounded
shutdown and checked both weak handles. Router, shutdown, store and log all read
Ok. These later observations preserve refusal precedence and establish cleanup at
those checks. They cannot reconstruct the write outcome or prove rollback.

## Disposition and one proposed observation

This closes the final-branch ambiguity for this Windows ProofInitial call only.
PR176 Ubuntu's query initial, proof initial and F10 final had no scoped capture.
Their branches remain unresolved. Prior artificial F10 control invalid, treatment
unrun. One natural red establishes this path, with no measured reproduction rate.
Debug reproduction, causal experiments and repair remain outside this review.

Owner decision: retain the hold, or separately grant a diagnostic source review
and one first-attempt hosted observation on a new diagnostic revision. The smallest
additional question is whether this owner eventually receives a successful engine
response despite the caller refusal. Reuse the existing slot writers. For this
one proof only, retain the scalar slots handle alongside the immutable return
snapshot and read just 17 and 18 once after the existing bounded shutdown. Preserve
both cuts and the original panic. No new wait, poll, engine query or change to
business clocks, deadlines, scheduling priorities, quarantine or runner settings.
Use the existing full-suite carrier and unchanged real proof assertion. No local
Windows engine or repeat of the failed head. Source and execution need separate
authorization. Reachability of this red is evidenced, frequency remains unknown.

Preregister the narrow prediction: on a matching red (ProofInitial WaitElapsed,
proposal pending, 16/Ok, empty 17/18 at return), a subsequent successful response
observed by the owner yields 17/Ok and 18/Pending after clean shutdown. That pair
supports eventual owner success across the two reads. It leaves engine timing at
the original deadline unknown. Forward, ReplyError or ReceiverClosed at 17 refutes
that success prediction for that call. Shutdown itself can affect the outcome.
Green means no reproduction. Another branch is a different result. Missing rows,
unclean teardown, invalid capture or unrelated red leave the prediction unresolved.
Stop after that first attempt in every case. No second cell or runtime fix credit.

## PROGRESS proposal only

Record Astra 6.2 high disposition complete, Windows branch identified, engine
outcome unknown. Keep PR178 held. Last merged PR176 and first-green main
`e3e79089`, CI 44229539357 (648 s wall, 645 s charged), are separate evidence.
Sol 3/3 exhausted, historical Opus 0/2 unchanged. M2 open, M3 unchecked.
Hardware **unverified on real hardware**. This review executed no runtime checks.

PR https://github.com/Dytschgo/arxcovo/pull/178, head 363d5320684505c15b2e3284b7c6f5a4f9ea842b, checks Ubuntu/macOS success, Windows failure, ci-time-budget and agent-review success, merge held.
