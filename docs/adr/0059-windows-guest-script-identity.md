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
- **Explicit default DACL:** `D:(A;;GA;;;SY)(A;;GA;;<token user>)`.
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

1. **Policy gate.** These HKLM values must be absent:
   `SOFTWARE\Policies\Microsoft\Windows\PowerShell\{ExecutionPolicy,EnableScripts}`
   and `__PSLockdownPolicy` under
   `SYSTEM\CurrentControlSet\Control\Session Manager\Environment`. These must be
   absent or `0`: `EnableTranscripting`, `EnableScriptBlockLogging` and
   `EnableModuleLogging` under the matching `...\PowerShell\*` policy keys. An
   unreadable value counts as a conflict.
2. **Self-test.** A fixed probe script runs through the production path: the
   production identity, Job and request directory. It requires FullLanguage,
   MachinePolicy and UserPolicy `Undefined`, and an effective `Bypass`. It
   writes one file to its request directory and must exit with the exact
   sentinel `23131`. Any other outcome, including a start-up failure such as
   `0xC0000142` or the CLR's `0xFFFF0000`, leaves execution `Unavailable`. An
   unverified self-test cleanup fails the service.

### Workspaces (D2)

The service creates one request directory per script under the verified
`work` root, which scripts cannot write. Its descriptor is
`D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;;0x1200ef;;;<user>)(A;OICIIO;FA;;;<user>)(A;;RC;;;OW)S:(ML;OICI;NW;;;LW)`.
Scripts can create children there. They cannot delete, rename or re-ACL the
directory itself. After Job cleanup is verified, a bounded walk removes the
directory: at most 4096 entries and depth 32. The walk never follows a reparse
point and clears read-only attributes. A failed removal retires the session
with no result frame.

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
They also prove PowerShell `exit 7`, the self-test sentinel, and removal of the
request directory.

These remain owner-host items, **unverified on real hardware**: the LocalService
token shape at System integrity, the session 0 window station, PowerShell
under this profile as LocalService, the service-SID ACLs on the work root and
the vioserial device, `check_scm` and `protect_parent` against the installed
service, the effective policy values on the pinned image, and broker denials
through Task Scheduler, SCM, WMI and COM.
