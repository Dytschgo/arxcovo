# Windows identity feasibility probe

Status: standalone research harness for the proposed ADR 0006 contract, based on
`3b6c819` and `git show dd0840f:docs/adr/0006-single-host-execution-boundary.md`.
It is not the production executor or milestone 2 acceptance. All S4U, privileged
process creation, desktop/session, and Job termination paths are **unverified on
real hardware**. No privileged CI experiment has run. Milestone 1 final gates
remain the coordinator's work; this probe does not advance milestone acceptance.

The harness lives in `tools/windows-identity-probe`, outside Cargo. It has no
NuGet dependencies and targets .NET 10 LTS. There are no changes to Cargo, product code, existing
workflows or policy gates. Account/right/task mutations exist only in the new
guarded disposable-CI entrypoint; none were executed during implementation.

## Safe local checks

From the repository root, under the existing development identity:

```powershell
./tools/windows-identity-probe/Test-CiValidation.ps1
dotnet build tools/windows-identity-probe/IdentityProbe.csproj -c Release --nologo
dotnet run --project tools/windows-identity-probe/IdentityProbe.csproj -c Release --no-build -- --self-test
dotnet publish tools/windows-identity-probe/IdentityProbe.csproj -c Release -r win-x64 --self-contained true --nologo
```

Only the pure helper tests and probe `--self-test` may run on the owner PC. It does not query tokens, create
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

## Disposable CI helper and manual workflow (review required before use)

Implemented files:

- `CiValidation.psm1`: pure guard, fixed naming/path, cleanup plan and evidence validation.
- `Test-CiValidation.ps1`: pure positive controls and mutations of every context key,
  source SHA mismatch, name/path rejection, SID replacement, partial setup cleanup,
  omitted evidence stages, failed kill/drain, wrong types, malformed/oversized output.
  It parses the entrypoint and nested task script without executing either.
- `ProbeBatchRight.cs`: narrow LSA add/remove of only SeBatchLogonRight for the
  pinned generated SID. Refuses preexisting explicit rights before adding. Uses
  POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT; removal uses AllRights=false.
  No secedit, blanket policy import, password fallback or privilege adjustment.
- `Invoke-DisposableCi.ps1`: the only orchestration/mutation entrypoint. A single
  invocation retains creation flags and SID in memory and attempts cleanup in finally.
- `.github/workflows/windows-identity-probe.yml`: manual dispatch only, fixed fresh
  GitHub-hosted windows-2022 runner, eight-minute budget, contents read permission,
  no secret/id-token inputs, checkout persist-credentials=false. No dispatch inputs.

The entrypoint refuses anything except the exact trusted repo, owner actor and
triggering actor, main ref, manual event, hosted Windows markers, numeric run IDs,
and matching GITHUB_SHA/GITHUB_WORKFLOW_SHA/REVIEWED_SHA. It also checks the
checkout HEAD and tracked cleanliness before loading native code. These are
**spoofable intent/provenance guards, not a security proof**. Trusted reviewed
source, fixed hosted runner selection and disposal are operator preconditions.
Do not run this helper locally, even with spoofed environment variables.

Setup refuses an existing C:\arxcovo-probe parent, run tree, generated account or
task. It creates a protected parent and exact run-attempt control tree. SYSTEM
and Administrators alone have full control; generated SID gets bin RX and work
Modify only. Code, task script and result stay private. Only fresh user group
memberships are checked; existing profiles and account rights are not enumerated.
The password is generated directly into a SecureString, with random bytes cleared
and SecureString disposed; it is never converted to a plaintext string, logged,
written, exported or sent through command arguments. S4U never receives it.

Task registration uses Task Scheduler COM TASK_CREATE (never update), service
account SYSTEM, highest run level, protected SYSTEM/admin DACL, fixed absolute
hosted pwsh path, no profile, no triggers/retries and a one-minute execution limit.
The protected script starts only the self-contained probe, with a cleared fresh
seven-key environment including SystemRoot from Windows. Output reads have a
131072-character cap, followed by a 256-KiB file cap and 128-line/2048-character
JSON limits. Polling lasts at most 75 seconds and requires a changed LastRunTime,
nonrunning/nonqueued state and LastTaskResult 0. Parsed evidence must contain
pinned SID/Medium observations, managed root exit 42, descendant exit 124 and
zero live Job processes. Only a fixed allowlisted numeric/boolean summary is
published; raw task output and setup exceptions are not printed or uploaded.
An API creation success or empty result cannot pass.

Cleanup compares current SID and task XML with captured identities, stops only
the created task, waits boundedly for stop, unregisters it, removes only the
newly granted right and account, then walks only the exact new tree without
following reparse points. The empty newly created parent is removed nonrecursively.
Tree cleanup stops at 512 entries or depth 32 rather than walking arbitrary data.
Independent cleanup steps continue after failures; any failure fails the job.
If identity/path checks fail, unsafe deletion is refused and the runner must be
discarded. No runner reuse or automatic retry is allowed. The probe's held Job
and process handles perform kill/drain; the wrapper never kills by recycled PID.
A hard runner cancellation/crash can interrupt finally: hosted runner disposal,
not a persisted recovery script, is the backstop. This is not production cleanup.

### Exact coordinator steps for a future dispatch

No dispatch is authorized by this implementation. After independent Opus security
review (or prescribed fallback), integrate the reviewed change through all normal
required gates. Keep this experiment separate from milestone acceptance and PR 11.

1. Review the final immutable main SHA including this workflow, every helper and
   probe source, and action pins. Confirm main has no additional unreviewed commits.
2. Independently verify the immutable SHA and green required checks; record review
   evidence before setting repository variable WINDOWS_IDENTITY_PROBE_REVIEWED_SHA
   to that exact 40-lowercase-hex main commit SHA. This variable is an operator
   precondition, not an automatic attestation. Leaving it unset disables the job.
3. As Dytschgo, manually dispatch `windows-identity-probe.yml` on main, with no
   inputs (UI Run workflow, or `gh workflow run windows-identity-probe.yml --ref main`).
   Confirm GitHub reports the selected SHA equals the reviewed SHA. A moved main,
   rerun by another actor, fork/repo/ref mismatch or absent variable is refused.
4. Inspect task-result, ci-evidence and cleanup summaries. All must succeed.
   Missing summaries, cancellation, timeout or cleanup failure mean failed experiment:
   discard runner, preserve only sanitized evidence, no automatic retries.
5. Clear the repository variable after the experiment. Review any future source or
   negative fixture independently, then use another fresh hosted runner. Do not
   add workflow inputs for commands, branches, account names, paths or negative modes.

### Toolchain and action provenance (checked 2026-11-11)

.NET 8 ended support November 10, 2026. The harness targets net10.0-windows;
.NET 10 LTS is supported through November 14, 2028. The workflow explicitly
installs SDK 10.0.404 (Microsoft release metadata, runtime 10.0.15); it does not
assume a hosted toolcache version. Local verification used installed SDK 10.0.303,
so the exact future workflow SDK installation remains CI-unverified.

- [Microsoft support matrix](https://dotnet.microsoft.com/en-us/platform/support/policy/dotnet-core)
- [Microsoft .NET 10 release metadata](https://builds.dotnet.microsoft.com/dotnet/release-metadata/10.0/releases.json)
- [checkout v4.2.2 primary tag API](https://api.github.com/repos/actions/checkout/git/ref/tags/v4.2.2):
  11bd71901bbe5b1630ceea73d27597364c9af683.
- [setup-dotnet v5.0.0 primary tag API](https://api.github.com/repos/actions/setup-dotnet/git/ref/tags/v5.0.0):
  d4c94342e560b34958eacfc5d055d21461ed1c5d.
  Both SHA pins were verified by read-only primary GitHub API requests, not GitHub mutations.
- [LsaAddAccountRights](https://learn.microsoft.com/en-us/windows/win32/api/ntsecapi/nf-ntsecapi-lsaaddaccountrights),
  [LsaRemoveAccountRights](https://learn.microsoft.com/en-us/windows/win32/api/ntsecapi/nf-ntsecapi-lsaremoveaccountrights),
  [TASK_CREATE and task DACL](https://learn.microsoft.com/en-us/windows/win32/taskschd/taskfolder-registertaskdefinition).

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

## Implementation verification record

Provider/effort: GPT-6.1 Sol, high; base 24d3c7e. Scope stayed within probe
files, this document and the new manual workflow; coordinator owns PROGRESS.md
updates and independent review. No subagents, push, PR, dispatch or GitHub writes.
Release build passed with zero warnings/errors; probe self-test passed 21
assertions; helper pure tests passed 82 assertions; self-contained win-x64
publish passed. No account/right/task/service/privilege/VM mutation ran locally.
The native LSA helper compiled but was never invoked. All privileged APIs, ACLs,
task execution, negative native fixtures and cleanup are **unverified on real
hardware** until independently reviewed disposable CI. Source parsing and pure
refusal tests do not establish native behavior or hosted provenance.

Pure mutation testing on temporary copies removed, separately, the SID match,
reviewed source SHA match and descendant-exit requirement. Each caused an expected
test failure; the intact helper suite passed afterward. YAML parsing and assertions
for manual-only triggers, windows-2022, contents-read and eight-minute budget passed.
These tests establish validation sensitivity, not privileged/native correctness.
