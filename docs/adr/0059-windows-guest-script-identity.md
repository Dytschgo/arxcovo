# ADR 0059 — Windows guest script identity

Status: accepted for M4 PR 5a. The coordinator decided this under owner
pre-approval (decisions D1 and D2). It deviates from the slice 5 design §3
"Identity", which proposed a `WRITE_RESTRICTED` token with a restricted-code
restricting SID. Everything specific to LocalService, session 0 and SCM below is
**unverified on real hardware**.

## Context

The `arx-guest` Windows service runs as the built-in `NT AUTHORITY\LocalService`
account with an unrestricted service SID. It runs each caller script through
the fixed System32 Windows PowerShell 5.1 wrapper, inside a per-request,
kill-on-close Job assigned at creation.

The designed script token was `WRITE_RESTRICTED` with only the restricted-code
SID (`RC`) in its restricting set. A diagnosis on 2026-11-23 found two blockers:

1. With the token's inherited default DACL, any restricted child launched with
   `CREATE_NO_WINDOW` failed console start-up with `0xC0000142`
   (`STATUS_DLL_INIT_FAILED`) before user code ran.
2. Even with a fixed default DACL, the .NET Framework CLR in PowerShell 5.1
   could not start (`HRESULT 0x80070005`). Start-up needs write access to an
   object granted only to Everyone. Adding Everyone to the restricting set would
   largely void the write restriction for world-writable objects.

Both blockers reproduced from a filtered administrator identity on a
development host. The diagnosis found nothing to suggest that session 0 would
grant the missing write.

## Decision

The script token is derived from the service token by
`CreateRestrictedToken` and then adjusted:

- **Low integrity** (`S-1-16-4096`).
- **Deny-only groups:** BUILTIN\Administrators, Local account and member of
  Administrators group, Power Users, Backup Operators and the
  `NT SERVICE\arx-guest` service SID. Entries the source token lacks are ignored.
- **No privileges.** Every privilege in the source token is deleted, including
  `SeChangeNotifyPrivilege`.
- **Groups that stay enabled:** every other group in the source token is
  unchanged. That includes `BUILTIN\Users`, `Authenticated Users`, `Everyone`,
  `INTERACTIVE`/`SERVICE`, `LOCAL` and the logon SID, so ordinary read grants on
  system files such as the .NET native images still apply. A test asserts that
  these three broad groups keep their source attributes. The boundary is Low
  integrity, no privileges and the Job, not removal of broad read groups.
- **Explicit default DACL:** `D:(A;;GA;;;SY)(A;;GA;;<token user>)`.
- **Explicit owner:** the token user. An elevated administrator token defaults
  ownership to `BUILTIN\Administrators`, which is deny-only here, so objects
  the script creates would otherwise be owned by a SID that grants it nothing.
- **No restricting SIDs** and no `WRITE_RESTRICTED`.

The child stays inside the creation-time Job: kill-on-close, 16 active
processes, 256 MiB per process and 512 MiB per Job, no breakaway. Before the
child resumes, the service verifies Job membership, no thread token, a primary
Low token with no restricting SIDs, an empty privilege set, the deny-only groups,
the exact default DACL, LocalService, session 0, not elevated and
`TokenElevationTypeDefault`.

The service accepts its own authority token only at **System** integrity, the
level SCM gives built-in service accounts, and only as
`TokenElevationTypeDefault`. The linked-token query is a secondary check whose
error codes are not interpreted.

### Start-up admission

The service advertises script execution only after both of these pass:

1. **Policy gate.** The service reads both HKLM and its own HKCU. Scripts run
   as the same user, so HKCU holds their user policy. Under
   `SOFTWARE\Policies\Microsoft\Windows\PowerShell`:
   - `ExecutionPolicy` and `EnableScripts` must be absent in both hives. These
     are the MachinePolicy and UserPolicy scopes, which would override
     `-ExecutionPolicy Bypass`.
   - HKLM `ScriptBlockLogging\EnableScriptBlockLogging` must be present and `0`.
     The HKCU value must be absent or `0`.
   - `EnableScriptBlockInvocationLogging`, `ModuleLogging\EnableModuleLogging`
     and `Transcription\EnableTranscripting` must be absent or `0` in both hives.

   `__PSLockdownPolicy` under
   `SYSTEM\CurrentControlSet\Control\Session Manager\Environment` must be
   absent. An unreadable or non-DWORD value counts as a conflict.
2. **Self-test.** A fixed probe script runs through the production path: the
   production identity, Job and request directory. It uses no cmdlets. It
   requires FullLanguage and the pinned module path, writes one file to its
   request directory, and must exit with the exact sentinel `23131`. Any other
   outcome leaves execution `Unavailable`. That includes a start-up failure such
   as `0xC0000142`, the CLR's `0xFFFF0000`, or the FailFast exit `0x80131623`
   that Windows PowerShell uses for any unhandled exception. An unverified
   self-test cleanup fails the service.

### Script content logging (decision)

Windows PowerShell 5.1 logs script blocks it classes as "suspicious" to the
PowerShell Operational event log (event 4104) whenever script block logging
is **not configured**. The wrapper passes caller text to
`ScriptBlock::Create`, so an unconfigured policy can copy caller scripts into
host telemetry. That conflicts with design §3, which says no payload may
reach managed evidence. Scripts can carry caller data, so treating them as
non-secret is not acceptable. No supported per-process switch disables the
automatic logging. The adapter does not tamper with PowerShell internals.

**Decision:** execution is available only when machine policy explicitly
disables script block logging (`EnableScriptBlockLogging = 0`). Explicit
disablement also turns off the automatic suspicious-block logging. Module
logging and transcription must be off as well. Owner-managed policy is read,
never changed. The 5b image provisioning must set the HKLM value. Hosts
without it, including hosted CI runners and development hosts, leave
production admission closed. Tests inject the policy snapshot. Scripts that
start their own transcript or enable module logging choose to do so
themselves.

The policy is checked at admission and again before every request. Each
request starts a new interpreter that reads the policy afresh, so a value that
changed or became unreadable after admission makes that request
`Unavailable`. The check is the same 13 bounded registry reads.

### Fixed environment and module discovery

The interpreter environment is fixed, sorted and free of caller bytes:
`APPDATA`, `LOCALAPPDATA`, `PATH`, `PSModuleAnalysisCachePath`, `PSModulePath`,
`SystemRoot`, `TEMP`, `TMP`, `USERPROFILE` and `windir`. Every per-user
location (profile, application data and the module analysis cache) is inside
the request directory. A fresh or Medium-integrity account profile is never
written, and everything is removed with the directory.

Windows PowerShell 5.1 prepends `Program Files\WindowsPowerShell\Modules` to any
inherited `PSModulePath`. This host showed it. The wrapper therefore resets
`$env:PSModulePath` to `$PSHOME\Modules` before reading stdin, so command
discovery is system-only, as design §3 requires. The wrapper and self-test use
no cmdlets, so starting them never triggers module discovery. Hosted
`windows-2025` CI failed admission (`0x80131623`) and timed out under the
earlier environment, which extended discovery into the image's large Program
Files module tree. That cause was inferred, not reproduced. The development
host could not reproduce the failure.

After that change, hosted CI still ended both restricted runs with
`0x80131623`. The unrestricted wrapper runs passed. Application Error event
1000 named `System.Management.Automation.ni.dll`, with one fault offset for both
runs. That module is where PowerShell's unhandled-exception handler raises
FailFast: the native image had loaded and was running. The faulting module
therefore does not show which exception was thrown. The trigger is unknown.

**NGEN decision.** `COMPlus_ZapDisable=1` makes the CLR ignore native images
and JIT instead. On this host it raises a restricted PowerShell start from
about 0.18 s to about 1.65 s. It is **not** part of the fixed environment. It
would only move the fault, at that cost for every request. Instead, failure
diagnostics re-run the failing script both with and without it.

**Diagnostics.** These run only on an unexpected PowerShell outcome, are
bounded, and use only children the test launches. The test re-runs the script
with the child's stdout and stderr captured, so PowerShell's own "An error has
occurred that was not properly handled" text, with the exception type and
stack, is printed. It then reports:
- only `.NET Runtime` and `Application Error` events whose process id is that
  child, inside the re-run's time window;
- for the restricted identity, the token owner, integrity and session, and
  the runner's elevation and UAC setting;
- owner, a DACL summary and the AccessCheck result for the restricted token
  on the native-image directories, `System.Management.Automation.ni.dll`,
  `$PSHOME`, the GAC directory and three registry keys.

Account, machine and user-SID text is removed from the output.

### Workspaces (D2)

The service creates one request directory per script under the verified
`work` root, which scripts cannot write. Its descriptor is
`D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;0x1200ef;;;<user>)(A;OICIIO;FA;;;<user>)(A;;RC;;;OW)S:(ML;OICI;NW;;;LW)`.
Scripts can create children there. They cannot delete, rename or re-ACL the
directory itself. After Job cleanup is verified, a bounded walk removes the
directory: at most 4096 entries and depth 32. The walk never follows a reparse
point and clears read-only attributes. A failed removal retires the session
with no result frame.

Setup and removal run on worker threads that hold no port handle and no
child. Setup must finish within the request deadline, plus a 2 s grace on
cancellation. Removal must finish within 2 s, and never later than the
request deadline plus 4 s, inside the host's budget-plus-5 s allowance. That
removal deadline is absolute: a late cancellation never restarts a grace
period beyond it. Likewise, a setup grace never passes the request deadline
plus 2 s. If a directory created during setup fails verification and cannot be
removed, the session retires instead of reporting `LaunchFailed`. A missed
step is left detached and the session retires. Nothing waits on it or uses its
result.

**Workspace size is `unqualified`.** It is not enforced in v1. Possible
mechanisms are a fixed-size dedicated volume or an NTFS quota. A quota is per
user, and all LocalService services share that user. Each mechanism needs
owner-host qualification or a later decision. Only the Job memory limits and
the per-request removal bound a script's footprint.

## What this protects

- **No write-up.** A Low token cannot write, delete or re-ACL any object at
  Medium or higher, including unlabelled objects. This covers the installed
  binary, the work root, other request directories, SCM, the service process
  and its token, and other LocalService processes, which run at System
  integrity. On this host, AccessCheck gave the Low token `0x120001` on a
  Medium object that granted the user `0x1f0003`.
- **Deny-only groups.** ACEs for Administrators or the service SID grant the
  script nothing. ACLs that must exclude scripts, such as the vioserial device
  and the stdin pipe, grant the service SID rather than LocalService. Request
  directories grant the LocalService user, because scripts use that identity.
- **No privileges.** This includes bypass-traverse, impersonate,
  create-global and increase-quota.
- **The Job boundary.** Process count, memory, kill-on-close, no breakaway,
  and cleanup proven by the held process signal and `ActiveProcesses == 0`
  before any result is reported.
- The service process and authority token DACLs deny the script process
  creation, thread creation, memory writes, handle duplication and DAC/owner
  changes.

## What this no longer has

- **No restricting-SID write check.** Under `WRITE_RESTRICTED`, every write also
  needed an explicit `RC` grant. Now a write to another **Low-labelled** object
  succeeds if its DACL grants the LocalService user, Everyone, Users,
  Authenticated Users or another enabled group. MIC does not separate peers at
  the same level. Examples include Low objects created by other Low processes
  running as LocalService, and world-writable Low locations.
- **SERVICE stays enabled.** The `NT AUTHORITY\SERVICE` group and the logon
  SID are not deny-only, so their read grants still apply. Making them deny-only
  is untested against PowerShell start-up.
- **Reads are not restricted.** As before, a Low token can read anything its
  groups allow, because there is no read-up policy on unlabelled objects.

AppContainer or LPAC would be a stronger profile. It is a separate fence with
unknown PowerShell 5.1 compatibility. A future `WRITE_RESTRICTED` profile would
need the CLR's Everyone-only write target identified on the owner host, for
example with Process Monitor.

## Evidence and qualification

Hosted and development-host tests use a script token derived from the runner's
own token by the production `restrict_token`. They prove the token shape, the
label and deny-only differentials through AccessCheck, and a real restricted
child inside the production Job. That child creates a file in a Low request
directory, cannot create one in a Medium directory, and can read a seeded file.
Development-host tests also check PowerShell `exit 7`, the self-test
sentinel, and removal of the request directory.

**Hosted CI quarantine.** On Windows with `CI=true` and
`RUNNER_ENVIRONMENT=github-hosted` (or a `win`-prefixed `ImageOS` when
`RUNNER_ENVIRONMENT` is absent), the tests
`production_identity_runs_powershell_and_removes_its_request_directory` and
`self_test_admits_the_production_identity` report a restricted launch ladder
in place of their PowerShell exit-value assertions. The runner's Windows
PowerShell 5.1 has exited with `0x80131623` under the restricted identity.
The ladder runs the native and Framework controls, bare PowerShell, and
the wrapper in fresh request directories. It still requires verified token
and Job setup, input handoff, and cleanup. Self-hosted and local tests
continue to require the full PowerShell and admission results. Actual
qualification requires an installed service run on the owner's pinned
Windows 11 guest and is **unverified on real hardware**.

These remain owner-host items, **unverified on real hardware**: the LocalService
token shape at System integrity, the session 0 window station, PowerShell
under this profile as LocalService, the service-SID ACLs on the work root and
the vioserial device, `check_scm` and `protect_parent` against the installed
service, the effective policy values on the pinned image (including the
required `EnableScriptBlockLogging = 0` and the absence of 4104 events for a
probe script), and broker denials through Task Scheduler, SCM, WMI and COM.
