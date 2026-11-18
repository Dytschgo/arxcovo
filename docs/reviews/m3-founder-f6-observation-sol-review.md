# F6 observation: independent static review

Reviewer: OpenAI / Sol 6.1 / high, 2026-11-18; no children.
Reviewed HEAD: 9532803ae2b46c2961c1fba4adcc21cfc21c6d9e; initially clean.
Targets: docs/evidence/m3-founder-f6-observation.patch and
docs/briefs/m3-founder-f6-observation.md, as committed at that HEAD.
Patch SHA256:
6fb49eba004936204ec45fb113f0f89b640fb2625686546222a6c67549b7d3cd
Read AGENTS, KICKOFF, debug/review skills and their active profiles, founder
disposition, ADR0005 and ADR0024. This is independent of the probe builder.
Task-specific static-only restrictions supersede diagnostic runtime loops.

## Standards

PASS; zero blocking findings or substantive smell findings.
The diff is confined to temporary founder instrumentation and one F6 call.
No product change, attempt-budget reset, quarantine or architectural deviation.
No correction to the exact patch or plan is required by this review.

Static Rust feasibility:
- The six phase expressions produce Result<T, FoundError> after their existing
  mappings. Generic exit<T> borrows each result briefly, then the macro moves
  it back to the original outer ?. No Clone, Debug or changed error conversion.
- Macro double braces give hygienic result locals. Observation paths resolve
  in found; its child found_tests can access observation and pub(super) capture.
  The observer child can import private ancestor Instant and ISSUE_BUDGET.
- Event is Copy; [None; 24] is valid. Buffer contains only scalars/static strings
  and is Send + 'static. RefCell<Buffer> is Send; Sync is not required here.
  Inspected pinned cached Tokio 1.48.0 task_local.rs: scope owns the value and
  inner future, swaps task-local state per poll, and restores it on return/drop.
- No Ref/RefMut crosses an await. capture consumes work via await without
  requiring it to be 'static or spawning it. Its concrete issue future retains
  the original borrowed node/CA/CSR lifetimes; no new Send obstacle identified.
  This is static reasoning, not compiler or lint verification.

## Spec

PASS; zero blocking findings. Ready for a separate root execution decision.
The six macros preserve original call/mapping/await/? ordering. In particular,
the FoundMembers construction ? remains inside the proposal expression and
returns from the same work async block. There is no extra async phase boundary.
Finaldeadline surrounds the unchanged timeout match, including its original
Instant::now() < deadline guard. All four SignerClock reads, both fresh guards,
signing, issuance/read permits and Owners operations retain their order.
No spawn, yield, sleep, retry, timeout extension or scheduling policy is added.

The macros retain expression results; intermediate mapped error types are Copy,
and added borrows end synchronously. No original resource is cloned or retained
for output. The original work is dropped by the timeout path and the issuance
permit leaves found before capture formats anything. Cancellation still drops
the original future; no Drop logger, fallback work or continued polling is added.
Observer calls add monotonic reads and bounded synchronous overhead; they can
perturb timing and the existing 20 s whole-test deadline still includes output.
This is not a claim of zero execution-time disturbance.

Only F6's initial issue is scoped. Task-local context is not inherited by
spawned Owners tasks. Other F6 guard calls occur after capture exits; unscoped
observer calls collect nothing. No global lock or node/pointer selector exists.
Both production macro definitions expand directly to their original work
expression; observation is cfg(test), and found_tests is already cfg(test).

Interpretation limits are correctly preregistered: pending identifies the last
unmatched caller expression, not worker completion or a causal storage seam.
An inner proposal ? can bypass exit without a pending await. Remaining zero is
an observer sample, not the original final predicate's sample. All six Ok exits
and final Unavailable do not uniquely identify a cause. Admission refusal before
finaldeadline, panic or cancellation can leave no trace; missing output must be
reported without a fallback invocation. Overflow invalidates the trace.

The plan requires identical pinned source, private checked non-reparse cache,
target and evidence directories, no competing build/test workloads, offline
pinned tools and sanitized configuration admission. Its exact nextest command
selects only F6, one test thread, retries 0, fail-fast and final success/failure
output. Compilation/config/admission failure consumes the future grant too.
The root grant must expressly include compilation and ONE natural invocation.
No F7/F8, repeat, injection, tuning, installation or network is included.
Restoration checks expected hashes before reversing only this patch; drift
stops restoration rather than overwriting changes. Full crate-byte verification
and absence of the marker in active source are required afterward. Survivors
are recorded for coordinator disposition; unrelated processes are not killed.

## Security

PASS; zero blocking findings. Every emitted field is an integer, bool or fixed
allowlisted static string. BadCsr/Rejected payloads and all underlying errors
are reduced to variant names. No result Debug, CSR/DER, key, CA marker, code,
proof, store content, owner path, node identifier or pointer is formatted.
The fixed event array never grows: at most 14 events on the complete path,
24 stored events maximum, then an overflow bit. Using maximum u128 decimal
widths and longest allowed labels gives at most 4431 emitted bytes including
header and final LF. Formatting occurs once after found returns, not in phases.
Existing fail-closed certificate discard, clock refusals and guards are intact.

## Checks and disposition

Actually ran git apply --check --whitespace=error-all: PASS, no application.
In-memory unified-diff reconstruction verified all six non-test phase identities
and the final identity against original source tokens. Reversing just capture
in memory exactly restores found_tests; the sole insertion is in F6.
Compared all 130 tracked crate files byte-for-byte with failed main
09134085dce93742d8812821dbe11834dcee7a9c: identical. Ordered path-NUL/bytes-NUL
aggregate SHA256:
026f868a360d527e742e6f84dbfb00b9931ce485455a0681ddbabeff02f342ac
Cargo.lock, toolchain and local config tracked diff against failed main is empty.
No active source patch, Rust tool, build, test, engine or network was run.

Grant readiness: PASS for root to consider the exact bounded grant above.
This independent review does not authorize execution, publication or merge.
Hosted failure remains unreproduced after ONE green local control; no cause
claim or hosted stability credit. Sol 3/3 exhausted; historical Opus 0/2 unchanged.
M2 open, M3 unchecked, PR176 held; hardware paths **unverified on real hardware**.
Only this review is committed; PROGRESS and active source remain unchanged.
Standards: 0 findings; Spec: 0 findings; Security: 0 findings.
