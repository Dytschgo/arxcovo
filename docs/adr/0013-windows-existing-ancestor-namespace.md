# 0013: Windows existing-ancestor namespace integrity

Status: implementation candidate, independent security review pending.
Provider: Codex / gpt-6.1-sol, high; 2026-11-11.
Scope: PR25 follow-up on prepared base 403742ec7136679e8fd3e457b53fa80f636a6efe.

## Cause and decision

Applying final binary-integrity policy to every ancestor rejects stock Windows
paths. The drive root grants creation and inherit-only modification; ProgramData
has a Users creation/EA/attributes grant (0x116 on this machine); Program Files
has inherit-only CREATOR OWNER full control. These do not give control of the
protected existing next component. The old boolean lost the relevant rights.

Retain each native allow ACE access mask and distinguish namespace ancestors
from final service objects. Ancestors require the existing trusted owner set,
a present non-null DACL, understood ACEs, and no effective foreign DELETE_CHILD,
DELETE, WRITE_DAC, WRITE_OWNER or GENERIC_ALL. A deny never excuses an unsafe
allow. Creation, EA and directory attribute grants, including GENERIC_WRITE,
are permitted here; inherit-only grants do not affect the current object.
A successful path walk opens every existing next component and checks its
owner/DACL independently, with strict final-object policy at the leaf.
An ancestor with an existing next component is nonempty; creation/metadata
rights do not authorize removing that component to replace the ancestor.
The service opener retains every ancestor handle with FILE_READ_DATA
(FILE_LIST_DIRECTORY), FILE_SHARE_READ | FILE_SHARE_WRITE, and no
FILE_SHARE_DELETE. This permits unrelated sibling renames and moves into the
ancestor while preventing rename/deletion of each pinned component itself.
Namespace authority also depends on the owner/DACL checks above; pins alone
do not block all modification or freeze ACLs. Immutable final leaves retain
read-only sharing to exclude writers; mutable DB/journal/lock leaves share
read/write without delete sharing. This does not change owner adapters or
development executor policies.
Unknown ACE types, unsafe owners, reparse points, unsafe lexical paths,
nonlocal disks, hardlinked final files and final private inheritable foreign
grants still refuse. No path string or ordinary group gains trust.

This refines ADR0009's ancestry contract on Windows only. Final binary/private
policies are unchanged. The System32 regression explicitly exercises namespace
validation, not System32 as a final Arxcovo binary directory (whose inherit-only
grants still fail strict final policy). No macOS policy correction is claimed.

Microsoft primary references:
- [File security and access rights](https://learn.microsoft.com/en-us/windows/win32/fileio/file-security-and-access-rights): effective versus inherit-only ACEs, child descriptors and generic mappings.
- [File access rights constants](https://learn.microsoft.com/en-us/windows/win32/fileio/file-access-rights-constants): creation versus deletion and metadata rights.

## Verification and limits

PR27 ancestor-sharing correction (Sol high, 2026-11-11): the nested owned-temp
regression failed on the untouched production opener with error 32 at the
unrelated sibling-file rename. With ancestor read/write sharing, file/folder
sibling renames and moves into the ancestor succeed; the protected next directory
refuses rename and DELETE access, mutable leaf writes succeed, and pins release
after drop. The immutable-leaf regression still excludes writers until drop.
Restoring read-only ancestor sharing fails the sibling assertion; adding ancestor
delete sharing fails the directory DELETE-access assertion. Both mutations were
restored. Focused pin/leaf/hot-journal tests passed, as did formatting, locked
workspace/all-target Clippy and full normal nextest (118 passed, 2 ignored child
fixtures skipped and exercised only through their parents). This is local
current-user evidence; fresh independent Opus review and hosted checks remain
required, with no installed-service acceptance claim.

The original System32 regression failed with Permissions on the untouched base.
The refined namespace regression also fails when the old ancestor call is
restored, and passes with this policy, including read-only native known-folder
Program Files and ProgramData observations. Native AccessCheck on in-memory
SDDL descriptors verifies creation, inherit-only full control, DELETE_CHILD,
DELETE, WRITE_DAC and WRITE_OWNER semantics; it uses a duplicate of the current
query token, not an identity switch. No on-disk security descriptor is changed.
Local owned-temp junction and hardlink fixtures exercise rejection with ordinary
positive controls. Pure tests cover final private inherit-only grants, individual
control rights, unsafe ownership, missing DACL and unknown ACEs.

Eleven individually applied then restored mutations failed their intended tests:
old blanket ancestor policy; omitted DELETE_CHILD, WRITE_DAC, WRITE_OWNER or
DELETE; removed ancestor owner check; accepted unknown ACE; discarded native
mask; removed reparse or hardlink guard; accepted foreign final private grants.
These were assertion failures, not compilation failures. All mutations restored.

A trial read-only kernel32.dll final-file control refused with Layout on this
host; Windows system files can be hardlinked. It was not used to force a policy
pass or weaken the final hardlink guard.

PR27 adds read-data access to make no-delete-sharing pins effective; its
blocking-review correction allows ancestor write sharing as described above.
No race-free namespace/ACL freeze or guarantee for a later symbolic
open is established here. ACL changes by privileged writers and runtime service
integration still require the larger service contract. Installed service,
dedicated-account execution, SCM/LSA, owner adapters and M2 acceptance remain
**unverified on real hardware**. No installation, elevation, account, rights,
task, dispatch, settings or old Lab Manager/VM operation was performed.
