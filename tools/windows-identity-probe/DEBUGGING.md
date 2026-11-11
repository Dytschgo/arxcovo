# Setup failure feedback loop

## Unelevated executor handle fixture (2026-11-11)

This section concerns ordinary process-only Rust fixtures, not the privileged
identity experiment below. Provider: Codex / gpt-6.1-sol / high.

Symptom: `actual_job_tree_timeout_cancel_shutdown_overflow_and_handles` expected
the handles child to exit 0 but got 101. Read PR20 review addendum
https://github.com/Dytschgo/arxcovo/pull/20#issuecomment-6796952080 and exact Windows
job logs: PR20 run 43027607061/job 130546160049, main run
43027101934/job 130544462129, PR25 run 43026547236/job 130542581606. All show the
same parent assertion; none includes child stderr. Exact hosted child cause is
therefore still uncertain; no hosted runner change is proven.

Local baseline at 0de0b725 (main 66c1b173 plus diagnostic assertion): focused
`cargo test -p arxd --lib actual_job_tree_timeout_cancel_shutdown_overflow_and_handles --locked -- --nocapture`
and focused nextest each passed once. A temporary 64-slot sampler kept successive
inheritable parent events live and ran the unchanged handles fixture through
`run_inner` for each. It failed 1/64 samples in each of two runs (2/2 red runs).
Both surfaced child line 667: `GetHandleInformation` returned 1, expected 0;
parent observed exit 101. This sampler was removed after minimization.

Ranked hypotheses after reproduction: unrelated child object occupies the same
numeric slot; actual sentinel inheritance; standard-handle type mismatch.
The minimized regression `handle_slot_reuse_is_not_inheritance` allocates at most
4096 real child-local events until the slot is valid. It uses the actual
production CreateProcess/handle list, token comparison and Job cleanup. It
requires exit 0, confirmed cleanup, identity match and a distinct-object marker.
This deterministically models the invalid assumption without relying on handle
allocation order in a hosted runner.

The sentinel is now a randomly named local event. The child snapshots any
candidate using DuplicateHandle BEFORE OpenEventW opens a reference (opening the
reference could itself reuse the slot). CompareObjectHandles then rejects the
same kernel object and permits an unrelated object. Win32 semantics:
https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-compareobjecthandles.
The stdin CHAR/stdout PIPE/stderr PIPE assertions remain intact.

Mutation evidence (each run once, both killed with child/parent exit 101):

- Restored `GetHandleInformation(sentinel) == 0` before the snapshot: deterministic
  reuse regression failed with native return 1 versus 0.
- Temporarily extended the production handle list from three to four entries,
  using the fixture's inheritable sentinel as entry four: regression failed with
  `unintended inherited sentinel object`, CompareObjectHandles return 1 versus 0.
  Thus an actual unintended inherited event is rejected, rather than merely
  accepting every valid slot. Exact source was restored in finally blocks.

Fixed regression passed, and the original combined test passed. A final temporary
64-slot sampler with the named-event helper passed 64/64 children, with zero
distinct-reuse markers in that run; this is not a measured spontaneous reuse
rate for the fixed helper. The bounded regression supplies controlled reuse.
It passed both directly and in full nextest (2/2); the old-validity mutation was
red 1/1. Formatting and
workspace/all-target Clippy with warnings denied passed. Full workspace nextest
result is recorded in PROGRESS. Production source outside cfg(test), timeout,
handle-list protection, Job and token checks are unchanged. Hosted validation and
independent other-provider review remain pending. Installed-service,
dedicated-account and VM paths are **unverified on real hardware** by this fix.

Run only these unprivileged commands on a developer computer:

```powershell
pwsh -NoProfile -File tools/windows-identity-probe/Test-CiValidation.ps1
pwsh -NoProfile -File tools/windows-identity-probe/Test-GuardMutations.ps1
dotnet build tools/windows-identity-probe/IdentityProbe.csproj -c Release --nologo
dotnet run --project tools/windows-identity-probe/IdentityProbe.csproj -c Release --no-build -- --self-test
```

The helper suite includes `Test-SetupDiagnostics.ps1`. It extracts the real experiment catch and scheduled-action path guard as AST statements, then injects exceptions without executing the experiment, COM scheduler, account operations, or batch rights. The regression requires exactly two failure rows, checks their exact field set, bounds total output to 300 characters, and verifies hostile phase strings and exception/error-record metadata cannot escape. It checks all 13 setup phase assignments plus preflight and numeric HRESULT boundaries. Output is a fixed phase (unknown on invalid input), a signed 32-bit HResult, and an allowlisted category. Exception messages, stacks, InvocationInfo, accounts, credentials and paths are never read by the serializer.

The mutations restore the missing-diagnostic symptom, case-sensitive trusted-shell rejection, and five existing security guard regressions, and corrupt phase/category/HResult/privacy handling. All must be killed. This is a deterministic feedback loop for observability and a casing defect; it does **not** reproduce or resolve the unknown privileged hosted failure.

The trusted executable still must match the exact absolute `C:\Program Files\PowerShell\7\pwsh.exe` location. Ordinal-ignore-case permits Windows casing only. Relative/drive-relative/UNC paths, traversal, alternate streams, suffixes and other locations are refused before filesystem inspection. Existing canonical-path and ancestor reparse checks are also applied; no ACL or ownership policy is relaxed.

First disposable manual experiment: [run 42995659949](https://github.com/Dytschgo/arxcovo/actions/runs/42995659949), job 130439029291, **FAILED**. Remote step summaries show provenance/helper and build/self-test/publish success, then experiment failure. Filtered log evidence: 114 helper assertions and 31 native self-test assertions; experiment emitted only `ci-experiment ok:false`, then `cleanup ok:true`, with no task-result or native diagnostic. Cleanup was reported true, not independently proven here. Hosted logs contain `pwsh.EXE`; the original case-sensitive guard demonstrably rejects that string locally, but the logs do not prove execution reached that guard. **Cause unknown**. No S4U or privileged acceptance is claimed.

2026-11-11 local candidate verification (Codex / gpt-6.1-sol, high): 115 helper assertions, 73 setup assertions, 11 killed mutations; Release build has zero warnings/errors and native self-test passes 34 assertions with `privilegedPathsRun:false`. Native self-tests exercise ordinary child processes and an empty job only. Account/SYSTEM/scheduled-task/cross-account and cleanup paths remain **unverified on real hardware** by this candidate. No architectural change or additional setup defect is established.

`Invoke-DisposableCi.ps1` must never run on an owner/developer computer. The coordinator cleared `WINDOWS_IDENTITY_PROBE_REVIEWED_SHA`; leave it unset. Independent different-provider review and coordinator integration are required before any second dispatch. This task performs no dispatch, settings/variable changes, push, PR or merge.

PR #17 review correction: the production preflight try/catch runs alone in a child PowerShell with an invalid context. The child must exit 1, emit exactly the preflight diagnostic and ci-experiment failure rows, and never print a following marker. No provisioning body is present. Output reads and waits are bounded. The test also covers a real wrapped .NET static-method error, exact phase order, task-path marker placement, and independent exact-string/reparse layers. Current local setup suite: 90 assertions; helper suite: 115. Original 73/11 figures above are historical. No privileged rerun has occurred.

- Coordinator verification of this correction: all 115 helper assertions and 90 setup assertions passed; all 17 guard mutations were killed, including preflight exit/diagnostic removal, phase ordering, wrapped errors, reparse-call removal and unsafe normalization. No native code changed or privileged operation ran.
