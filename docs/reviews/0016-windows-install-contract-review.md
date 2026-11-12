# ADR 0016 independent contract review

Reviewer: OpenAI / gpt-6.1-sol / medium, single-agent read-only review of the
Opus-authored contract. Base `54c43fbcc9b9a6050f067a3e437e58a018b96b00`,
head `f29c0bcbb0a2597698f302ffc98e77bbd61f8447`. No implementation,
privilege, owner, SCM, LSA, service or system-layout operation ran.

## Standards

No findings. The two-file change separates the proposed contract and build
brief, records deviations and limits, and requires ordinary-user fixtures.
The review inspected both complete documents and the relevant pinned owner,
layout and SCM implementation. No code-smell finding applies to this docs scope.

## Spec

**P1, blocking: preserve private-directory pins for configure and uninstall.**
ADR 0016 decision 1 calls the guard "exactly ADR 0012 session steps 1-3" but
explicitly lists only `pin_chain(binary, BinaryIntegrity)`. The brief's
`OwnerGuard.pins` comment likewise says "binary chain + image". Decision 8
and PR-E1 then tell `open(guard)` to run only steps 4-7.

ADR 0012 step 2 pins the layout. At the pinned implementation,
`WindowsOwnerSession::open` first loops over every `layout.private_dirs()`
and pins each with `PrivateDirectory`, then pins the binary. Steps 4-7 enable
restore, acquire the lock, check staging and recover; none restores that loop.
A direct implementation of the proposed split therefore loses the config/state
ancestor policy checks and namespace pins on the configure/uninstall paths.
A no-follow open of `owner.lock` alone does not check its parent chain.
Install's new namespace pins cannot supply this lifetime for a later command.

Required correction: explicitly retain private-directory validation and pins
in the configure/uninstall session before any lock, staging, record or right
access. Fresh install may keep its binary-only guard and create-or-verify path.
State where this additional session phase occurs relative to privilege enabling,
and carry its pins until the lock is released. Add a regression that a replaced,
reparse or untrusted private parent refuses before cleanup/recovery/right calls.
No new owner acknowledgement or repair mechanism is needed.

**P2, clarification: scope decision 3's read-only fence to provisioning.**
The unqualified text says existing objects are never renamed or deleted and
that delete access exists only on handles created by this call. Yet decision 8
retains ADR 0012 staging cleanup: current `cleanup` opens an existing staging
file with `DELETE`, checks its policy, then discards it. Configure also replaces
existing targets by the separately reviewed publication protocol. State that
decision 3 governs the installer, and explicitly retain the checked session
cleanup/publication rules. Otherwise E1 cannot preserve N6 as instructed.

## Evidence and disposition

The first issue follows from the exact proposed API/phase split compared with
the pinned source; no exploit or production execution was attempted. The second
is a contract contradiction, not a claim that the existing cleanup is unsafe.
Microsoft's privilege documentation confirms restore can bypass write ACLs and
assign a valid owner SID; TOKEN_OWNER requires a SID already in the token:

- https://learn.microsoft.com/en-us/windows/win32/secauthz/privilege-constants
- https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_owner

The CreateDirectoryW reference fetch timed out; no API claim relies on that
failed fetch. No Rust tests ran because this review changes no implementation.
Before: proposed provisioning cannot be built safely from its literal session
split. After this report: implementation remains held pending a corrected
contract and fresh Sol recheck. This report is reversible documentation only.
No feature PR was opened during the queue-drain pause.

Standards: 0 findings. Spec: 2 findings; worst is omitted private-directory
pinning in the configure/uninstall phase split. Changes required.
Every privileged and installed path remains **unverified on real hardware**;
M2 acceptance remains open.
