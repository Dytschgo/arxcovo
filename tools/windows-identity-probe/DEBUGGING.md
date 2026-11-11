# Setup failure feedback loop

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
