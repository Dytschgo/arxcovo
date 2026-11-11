# Windows identity feasibility probe

Status: standalone research harness for the proposed ADR 0006 contract, based on
`3b6c819` and `git show dd0840f:docs/adr/0006-single-host-execution-boundary.md`.
It is not the production executor or milestone 2 acceptance. All S4U, privileged
process creation, desktop/session, and Job termination paths are **unverified on
real hardware**. No privileged CI experiment has run. Milestone 1 final gates
remain the coordinator's work; this probe does not advance milestone acceptance.

The harness lives in `tools/windows-identity-probe`, outside Cargo. It has no
NuGet dependencies and requires the .NET 8 SDK or newer. There are no changes to
Cargo, product code, existing workflows, policy gates, accounts, or services.

## Safe local checks

From the repository root, under the existing development identity:

```powershell
dotnet build tools/windows-identity-probe/IdentityProbe.csproj -c Release --nologo
dotnet run --project tools/windows-identity-probe/IdentityProbe.csproj -c Release --no-build -- --self-test
dotnet publish tools/windows-identity-probe/IdentityProbe.csproj -c Release -r win-x64 --self-contained true --nologo
```

Only `--self-test` may run on the owner PC. It does not query tokens, create
processes, mutate accounts, or call LSA. It tests account rejection, each CI
precondition independently, workspace rejection, missing-path rejection, JSON
serialization, environment construction, and native structure sizes. Build and
publish do not execute privileged operations. The self-contained publish is
needed so a child has no SDK/PATH dependency. Keep the publish directory intact.

## What a future disposable CI run measures

The only public probe arguments are a generated local account name
`axp` followed by eight lowercase hex digits, and the exact empty directory
`C:\arxcovo-probe\<GITHUB_RUN_ID>-<GITHUB_RUN_ATTEMPT>\work`.
There are no passwords, command strings, arbitrary executables, or timeout
arguments. `--child` and `--leaf` are fixed internal payloads; they also refuse
to run without the CI guards and refuse SYSTEM identity.

1. Require Windows, `GITHUB_ACTIONS=true`, `RUNNER_ENVIRONMENT=github-hosted`,
   `RUNNER_OS=Windows`, bounded numeric run/attempt IDs, and
   `ARXCOVO_DISPOSABLE_PROBE=reviewed-manual-ci-v1`. Require an existing exact
   workspace path with no reparse point in any ancestor and no entries.
2. Require actual LocalSystem SID `S-1-5-18`, effective `SeTcbPrivilege`, and
   the presence of `SeAssignPrimaryTokenPrivilege` and `SeIncreaseQuotaPrivilege`.
   The latter two may be disabled: CreateProcessAsUser documents temporarily
   enabling necessary privileges. The harness never adjusts privileges itself.
3. Resolve only a local enabled normal user via NetUserGetInfo; require
   USER_PRIV_USER. Enumerate indirect local memberships with a 16 KiB cap,
   refusing partial results and every membership except BUILTIN Users. Resolve
   and pin its observed SID for this invocation. Missing accounts fail before
   logon. No owner identity fallback exists.
4. Register a trusted LSA logon process, choose the MSV1_0 package, send
   MSV1_0_S4U_LOGON (message 12, CHECK_LOGONHOURS flag 2), requesting Batch
   logon (4). The username and local machine domain are contiguous with the
   native structure. Emit NTSTATUS, SubStatus and both Win32 mappings without
   profile/token contents. A missing batch right or denied logon should be
   distinguishable here. The actual local-account S4U result remains unknown.
5. Require the returned SID and exactly Medium integrity RID 8192; reject
   unexpected builtin/privileged domain groups, including deny-only groups,
   and token privileges outside a small normal-user allowlist (even disabled).
   CreateProcessAsUser receives that token directly. No password logon,
   DuplicateToken fallback, owner token, interactive desktop ACL edit or
   identity/integrity coercion exists.
6. Create a private unnamed Job with KILL_ON_JOB_CLOSE and an active-process
   limit of three. Create the published probe executable suspended with an
   absolute application path, writable command line, no handle inheritance,
   no window and a fresh seven-key Unicode environment. No PATH, owner home,
   GitHub token, credentials, arbitrary environment or standard handles are
   passed. No user profile is loaded. Leave desktop/session selection to the
   API; desktop failures must be evidence, not permission changes.
7. Assign the suspended root to the Job; observe its process token SID and
   integrity before resume. Assignment failure kills the suspended root.
   Wait at most ten seconds for managed startup and exit code 42. This code
   means the fixed root payload checked environment keys and standard handles
   and created/resumed one direct descendant. Creation success alone does not
   establish managed startup. Child validation/managed failures return 1 or 2;
   native payload API failures encode the Win32 error as `0xe0000000 | error`
   (also decoded in the parent's JSON). Native loader failures remain visible
   as their actual exit code.
8. Require exactly one live descendant in the Job's bounded PID list. Hold a
   process handle, confirm Job membership, observe SID/Medium integrity, then
   terminate the whole Job with code 124. Require descendant termination,
   exit code 124, and zero live Job processes within five seconds. Failure and
   timeout cleanup also terminate/drain the Job; closing its noninherited
   handle is the final kill-on-close backstop. The leaf otherwise sleeps for
   30 seconds, limiting fixture lifetime even on an unexpected cleanup error.

Results are JSON Lines: at most 128 lines of at most 2048 characters each.
Native API outcomes use numeric errors, never FormatMessage or arbitrary
exception text. Exact observed test SIDs are intentional evidence; usernames,
workspace contents, environment values, credentials and token/profile buffers
are not logged. Payloads have no output handles and communicate by exit status.
Unexpected managed failures are deliberately generic; do not enable stack or
environment dumps to debug them on a shared machine.

## Coordinator-only future manual CI wiring, after independent review

This section is a proposed experiment, **not authorization to run it now**.
Review the privileged surface with Opus (or the prescribed independent fallback)
before adding any workflow. Use a separate `workflow_dispatch` workflow on a
fresh GitHub-hosted `windows-2022` or supported newer disposable runner. Never
use self-hosted runners, the owner PC, existing accounts, Lab Manager, or
`agent-windows-test`. Do not modify the existing three-OS required workflow.

The workflow should have read-only contents permission, no secrets, no id-token,
no environment credentials, checkout with persisted credentials disabled, and
an eight-minute job timeout. Permit only a trusted owner-triggered dispatch of
the independently reviewed immutable source SHA; refuse PR/fork inputs. Pin
third-party actions to reviewed SHAs. The harness environment checks are
spoofable intent guards, **not a security boundary or proof of hosted provenance**.
Runner selection, trusted source, isolation and short lifetime are operator
preconditions. This must never become an elevated production entry point.

Plan the workflow's disposable setup/finally steps as follows:

1. Compile, run self-tests, publish, then create a fresh control directory
   `C:\arxcovo-probe\<run>-<attempt>`. Fail if it already exists. Validate every
   ancestor against reparse points before writes. Copy the publish output into
   a `bin` child. Create `work` empty. Create only a cryptographically random
   `axp` plus eight-hex user after confirming it is absent. Keep a generated
   high-entropy password as an in-memory SecureString only for New-LocalUser;
   do not echo, pass on command lines, export, or store it. S4U receives none.
   Never enumerate or modify existing user profiles or rights.
2. Set protected ACLs: SYSTEM/Administrators control directory full control;
   test SID read/execute on `bin`, no write; test SID modify only on `work`;
   logs and task scripts in the control directory inaccessible to the test
   SID. Remove broad inherited write ACLs. Do not place privileged task code
   in the account-writable workspace. Assert the new user is standard and
   belongs only to BUILTIN Users; retain its SID for cleanup comparison.
3. A reviewed CI-only setup helper must grant **only that generated SID**
   `SeBatchLogonRight` using LsaOpenPolicy/LsaAddAccountRights. Do not use a
   broad secedit import replacing host policy. That helper is deliberately
   not implemented by this research executable. Review its error handling and
   account/SID lifetime checks alongside the future workflow. No account right
   mutations are needed on this developer PC.
4. Write a bounded SYSTEM task script in the protected control directory.
   Embed only validated numeric run/attempt IDs and generated account name.
   Scheduled tasks do not inherit the dispatch step's environment: explicitly
   set the seven environment keys listed above (SystemRoot comes from the OS),
   then invoke the absolute published executable. Redirect only the probe's
   bounded JSON output into a fresh protected result file. Propagate `$LASTEXITCODE`
   as the task result. Do not redirect PowerShell verbose/debug/error streams
   containing arbitrary setup objects into published artifacts.

The core invocation *inside that future protected task script* is:

```powershell
# Values substituted by the reviewed setup helper, never arbitrary dispatch input.
$env:GITHUB_ACTIONS = 'true'
$env:RUNNER_ENVIRONMENT = 'github-hosted'
$env:RUNNER_OS = 'Windows'
$env:ARXCOVO_DISPOSABLE_PROBE = 'reviewed-manual-ci-v1'
$env:GITHUB_RUN_ID = '<numeric-run-id>'
$env:GITHUB_RUN_ATTEMPT = '<numeric-attempt>'
& 'C:\arxcovo-probe\<run>-<attempt>\bin\IdentityProbe.exe' 'axp<8-lower-hex>' 'C:\arxcovo-probe\<run>-<attempt>\work'
exit $LASTEXITCODE
```

Register its absolute PowerShell action with `New-ScheduledTaskPrincipal -UserId
'SYSTEM' -LogonType ServiceAccount -RunLevel Highest`; set an execution time
limit of one minute and no retries. Start only this newly named task. Poll its
state for at most 75 seconds, require a completed run and LastTaskResult 0, then
parse at most 256 KiB of results and require the final successful tree result.
Creation API success or an empty result file must fail the workflow. Do not
retry a failed privileged experiment automatically.

In a workflow `finally` / `if: always()` step: stop and unregister only the task
just created, terminate only fixture processes whose handles/SID were captured,
remove the newly granted right and only the newly created user after checking
its SID matches, then remove only the fresh control tree after confirming its
absolute path, prefix and absence of reparse points. Never recursively follow
test-controlled links during cleanup. Cleanup failure fails the experiment;
the disposable runner must be discarded, never reused. Publish only bounded
allowlisted JSON evidence and numeric task result, never runner-wide logs.

## Negative matrix for that reviewed experiment

Run each fixture in a separate fresh workspace/task (no parallel cases):

| Case | Required outcome | Present evidence |
| --- | --- | --- |
| CI marker only / each missing guard | refusal before native operations | self-tests |
| Invalid account/path/traversal | refusal | self-tests |
| Missing path / reparse ancestor | refusal | missing path tested; reparse CI pending |
| Generated nonexistent account | NetUserGetInfo status (normally 2221), no logon | CI pending |
| Generated user in Administrators | reject even filtered/deny-only membership | CI pending |
| Standard user with explicit SeDenyBatchLogonRight | LSA status/substatus (normally logon-type-not-granted), no child | CI pending; grant deny only to fresh test SID |
| Standard task identity / no SYSTEM | refuse LocalSystem precondition, no owner fallback | CI pending |
| SYSTEM token lacking effective SeTcbPrivilege | preflight refusal | CI pending; requires separately reviewed token-restriction fixture |
| Missing assign-primary/quota privilege | preflight refusal | CI pending; requires separately reviewed token-restriction fixture |
| Valid S4U + direct descendant | SID/Medium, root 42, descendant 124, Job empty | CI pending |

Do not remove privileges from an existing account or alter the runner's SYSTEM
policy to produce negatives. Use only a restricted duplicate token in a future
reviewed CI helper. Disabled quota/assign-primary privileges alone are not a
negative: the documented API may enable them temporarily. After three genuinely
different failed fixes, preserve sanitized API/exit evidence and hand back to
the other provider; never elevate, add password fallback or run on the owner PC.

## Limits and primary references

The default environment intentionally does not establish cargo/toolchain discovery,
profiles, network authentication, private desktop requirements, or production ACL
correctness. Path component checks are not handle-relative, race-proof traversal.
They rely on this fresh, trusted disposable fixture, with no adversarial workspace
writer during preflight. Production must implement stronger path handling.
Account names do not cryptographically prove creation by this run; trusted setup
must create them, pin their SIDs and protect the harness and task script.

Only direct CreateProcess descendants are tested. Job objects do not establish
containment of service/WMI-mediated execution; this is not a claim of preventing
all process escapes. Leaf identity is observed via its OS token, not a separate
managed-startup report. External cancellation/crash kill-on-close, nested Jobs,
reparse races, handle leakage beyond standard handles, and post-restart cleanup
need additional independent tests before any production adoption.

- [Microsoft SDK header](https://github.com/microsoft/win32metadata/blob/main/generation/WinSDK/RecompiledIdlHeaders/um/NTSecAPI.h): MSV1_0_S4U_LOGON layout, local domain semantics, message type and logon-hours flag.
- [LsaLogonUser](https://learn.microsoft.com/en-us/windows/win32/api/ntsecapi/nf-ntsecapi-lsalogonuser): Batch returns a primary token, statuses/substatus and buffer lifetime. Its detailed Kerberos S4U notes do not prove local MSV1_0 behavior.
- [LsaRegisterLogonProcess](https://learn.microsoft.com/en-us/windows/win32/api/ntsecapi/nf-ntsecapi-lsaregisterlogonprocess): trusted logon-process registration requires SeTcbPrivilege.
- [CreateProcessAsUserW](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw): token privileges, mutable command line, environment, handles, session/desktop constraints. The documentation's password fallback is deliberately excluded.
- [Job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects): inheritance, breakaway limitations, termination, and kill-on-close.

Implementation provider/effort: GPT-6.1 Sol, high. Outcome: buildable harness and
safe tests; privileged feasibility remains unresolved. No architectural decision
replaces ADR 0006. Coordinator should copy this task/evidence into PROGRESS.md
when integrating; this bounded builder only owns probe-specific documentation.
