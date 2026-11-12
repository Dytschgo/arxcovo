# PR36 SDK cancellation fixture evidence

Builder: OpenAI / gpt-6.1-sol / high, local implementation sub-agent, 2026-11-12.
Base: `7f47d8dc26ae0cbc0a39e7fe045a96fa32ba789e`, clean before work.
Published background PR36: `c044b723ff0863df85aca90ea0b85567593fff44`.

## Scope and conclusion

Only the local Windows IPC test fixture and this documentation change. No
production change or architecture deviation. No native dedicated executor,
privileged constructor, SCM/LSA/account/rights operation, owner administration,
VM/Lab Manager operation, private-state read, direct ignored subprocess fixture,
GitHub mutation, hosted rerun or CI policy change was performed.

The hosted generic `until` panic could have come from either backend entry or
durable completion. Its cause remains unproven. The reported hosted evidence
is from the coordinator brief; no additional hosted log retrieval was needed.

A controlled scheduling counterexample used the same SDK request, bridge,
random private named pipe, authenticated listener and real executor dispatch.
Only the fake backend preflight was held on a channel. The original eager
`tokio::spawn(handle.await_response())` started the real SDK's 200ms deadline.
The probe waited for preflight entry and the SDK timeout, allowed another 200ms
for notification processing, then released preflight. That extra diagnostic
sleep is not proof of a general scheduling bound and is absent from final code.
The worker correctly skipped backend entry. The first red labelled missing
backend entry; the second minimised red first observed durable
`failed/cancelled`, then asserted the original calls==1 expectation:
`left: 0, right: 1`. Thus backend-entry failure can occur while cancellation
and durable completion are correct. This is not a reproduction of the unknown
hosted scheduling history.

Ranked explanations after that red: (1) cancellation before backend entry;
(2) the generic fake's independent 3s exit before cancellation observation;
(3) cancellation/audit completion stall. The controlled counterexample supports
(1). No natural baseline failure established (2) or (3), and neither is claimed
as the hosted cause.

## Coverage before and after

Before: 200ms SDK response timer armed concurrently with waiting for calls==1;
fake independently returned after 3s; both waits had the same panic message.
After: a dedicated test-only fake signals preflight and backend entry. The test
holds real preflight for 250ms, deliberately beyond the old startup budget,
then waits for backend entry before polling `await_response`. rmcp 3.9.0 starts
its response timer inside that method. The automatic test still requires the
specific SDK Timeout error at the unchanged 200ms timeout; automatic timeout
coverage was retained, not replaced by explicit cancellation. A second test
calls the real SDK `RequestHandle::cancel` after the same entry handshake.
Both assert durable `failed/cancelled` before any client/executor/server shutdown.

Fake worker lifetime follows cancellation or channel disconnect, rather than
an unrelated 3s backend expiry. Dropping the fixture sender during panic releases
the worker even with cancellation propagation disabled. Preflight retains a 3s
fallback; async stage assertions use 20s, the previous helper's nominal budget.
The controlled preflight delay is test stimulus, not a production timeout change.

## Executed feedback loop

Commands run from this worktree, Windows, pinned repository Rust toolchain.
Focused command unless noted:
`cargo nextest run --locked -p arxd -E 'test(dedicated_fake_sdk_cancellation_propagates_without_shutdown)'`.

| Revision/probe | Run ID | Observed result |
| --- | --- | --- |
| Unchanged prepared base | `5b12c678-00e4-44f3-af0c-5043a96c9016` | 1/1 pass, 0.275s test phase |
| Held preflight + original eager timer | `8cfcbe50-dea3-4239-9246-7e8ff8b26173` | red: stage backend entry not reached, 1.475s |
| Minimized counterexample, durable cancellation checked first | `7e0ca31c-05a1-4b3a-93dd-b02f13739fb5` | red: backend calls 0 vs 1 after durable failed/cancelled, 0.721s |
| Corrected automatic + explicit tests | `c1e60597-7dfb-4f31-bcd5-4eb9865d1844` | 2/2 pass, 0.541s |
| Actual CancelOnDrop cancellation suppressed | `2180e797-7563-403a-8d58-ba4210f4946c` | 0/2 pass; both fail at durable audit completion without shutdown; 20.563s; both processes exit |
| Guard restored, stress-count 3 | `afb1e822-84eb-46fe-966d-2c2279af39d0` | 3/3 iterations pass, 2 tests per iteration; 1.625s total |

Corrected/mutation/stress filter: `-E 'test(dedicated_fake_sdk_cancellation)'`.
Mutation used `--no-fail-fast`; stress used `--stress-count 3`.
Temporary mutation replaced only `CancelOnDrop::drop`'s `self.0.cancel()` with
a no-op reference. The original production file was restored from HEAD before
stress and final checks; it has no final diff. No permanent mutation, probe-only
code or diagnostic sleep remains. There were eight focused iterations total
(counting stress iterations individually), below the maximum ten.

`cargo fmt --all` applied formatting; `cargo fmt --all -- --check` passed.
`cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
Final `cargo nextest run --workspace --locked`: **221/221 passed, 2 skipped**,
run `a856ce13-dd02-4161-acf3-69602aade807`, 10.675s test phase.
The two ignored subprocess fixtures were reached only through their normal parents.
One full workspace invocation; no retries or second full run.

No hardware acceptance is claimed: native/dedicated M2 is **unverified on real
hardware**. No PR publication or review was performed. Coordinator Opus must
independently review before publication. Historical builder-base checkpoint: last merged PR was #9 at
`6f18686af7a8498bf887f833f278f43d152867f2`.
