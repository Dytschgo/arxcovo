# Sol independent review: Opus D1 Windows diagnostics

Target: Opus6b70689/01a2e0d/fb84986 retained73cae0c/97430a5/8dd128b, based6b00d17/main6f6e9d2. Standards/Spec/Security reviewed separately from code and the recorded controls. No Windows cause, product fix or re-enable is established.

Standards: CONDITIONAL PASS on R1 below. The diagnostics module is large (~1k lines including controls) but one bounded test concern. No new dependency, workflow, unsafe or non-test adapter logic changes. Opus fields use nondeprecated actual APIs; append includes its flush, correctly disclosed.

Spec: CONDITIONAL PASS on R1. Ring128/node and event32 bounds, fixed enums/numbers and hard8192-byte rendering hold. Failure rendering uses try_lock and records unavailable snapshots rather than blocking on store state. The diagnostic classifier explicitly produces candidates, and metrics coalescing can hide transitions. Existing helper outcomes/deadlines, retries and all8 quarantine annotations are retained. Three opt-in controls are isolated from ordinary CI; the20-run green loop is honest nonreproduction, not hosted evidence. Non-gating hosted sampling remains unimplemented and requires separate measured contract review under12min job cap.

R1: watch() discarded finished JoinHandles without joining; close() ignored JoinError, so a panicked observer could disappear while a fixture printed a successful summary. Small coordinator correction retains every handle until close, aborts and joins all of them, then reports any observer panic after cleanup. Add a completed-observer-panic regression: the old close incorrectly succeeds, the corrected close fails as required. Fresh independent Opus review of this Sol correction is required. Failure Drop can only abort nonblocking; it cannot synchronously await cancellation, and no hard task-termination guarantee is claimed. Normal async teardown must join every task.

Security: PASS on content boundaries: only IDs/indexes/timing/counters/enums reach output; raw store addresses only index the internal registry, never render. No secret/code/SQL/path payloads are stored or printed. All changes are test-only and controls do not invoke M2 fixtures. stdout/stderr delivery is not a hard nonblocking OS guarantee; try-lock rendering avoids application mutex stalls.

Reusable evidence: Opusfmt/workspaceClippy/default68consensus/threecontrols and20natural iterations are reported as its actual runs, not coordinator reruns. Coordinator correction verification remains pending at this review checkpoint. Production/shared-helper semantics remain unchanged; M2 open, M3 unchecked, Windows three-node unverified on hosted Windows and **unverified on real hardware**.

## Closure

R1 is closed by Solf426eb5 and complementary Opus3f15736 review. Opus independently ran69 default consensus tests and three controls, and two compilable old-close/removed-assert mutants fail the new regression. Root separately ran9 diagnostic units/3controls/lockedarxdClippy/fmtdiff, recorded in evidence. Standards PASS, Spec PASS, Security PASS for D1 test-only source. No hosted sampling, product fix, quarantine removal or hardware acceptance is approved. Fresh exact-head publication review and five green gates remain required.
