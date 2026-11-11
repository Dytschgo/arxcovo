# Service config and owner-plan foundation

## Windows owner transaction prerequisite: incomplete adapter handoff

Superseded design note: ADR 0012 settles the native Windows contract. It defines
the owner record, the lock, compare-and-replace, recovery and uninstall
provenance, and it replaces the `Journal`/`ConfigurationIo` shape and the
compensation path described below. The API text below records the `a8628c6`
candidate only.

Sol / gpt-6.1-sol high, implementation sub-agent, base
`1a45ba15640b608ee4c953a104c06e451a85d07b`. This change implements only an
injected configure transaction prerequisite. It does **not** implement the
requested native owner install/configure/uninstall adapter. No
`owner_windows.rs`, native mutation, CLI/MCP route or auto-install is supplied.
The existing `Plan::execute` remains nontransactional: do not use it for native
Windows configure. Other-provider review remains pending. This candidate does
not amend ADR0009 or establish M2 acceptance.

Recovery obstacle: `configure_plan` orders `BatchRight(add)` before
`AtomicConfig`. The LSA grant and filesystem publication have no common atomic
commit. A process can die after changing the right and before durably recording
success. Neither durable intent nor subsequent right presence proves who added
the right. Automatically revoking in that state violates the ownership fence.
The new engine preserves ambiguous evidence and refuses further mutation.
Native recovery and durable uninstall ownership evidence still need architecture/
security review and implementation; no product approval flow is introduced.

Exact callable API in `service::owner_transactions`:

- `Configure::from_plan(&Layout, &Plan, &impl Resolver) -> Result<Configure, Failure>`:
  Windows configure shapes only; fresh new/expected config validation, build-pin
  equality and grant-record consistency. Read-only `bytes()`, `expected()`,
  `sid()`, `adds_right()` contain private data.
- `configure(&Configure, &mut impl ConfigurationIo) -> Result<(), Failure>`:
  refuse outstanding journal; prepare/flushed staging; durable intent; exact SID
  grant; durable success receipt; publish. Grant/receipt errors preserve intent.
  Publication errors invoke receipt-backed recovery. Pre-existing rights create
  no new receipt and are never compensated.
- `recover(&mut impl ConfigurationIo) -> Result<Recovery, Failure>`:
  `Clean`, `Published` or `Compensated`. Intent always refuses regardless of
  right presence. A granted receipt plus matching new config retains the right;
  matching old config permits compensation. Other bytes refuse; failed
  compensation/cleanup retains the receipt for retry.
- `Journal::{encode, decode}`: bounded (1024 bytes), strict, versioned encoding.
  Parsing is not provenance. Journal/Configure omit Debug; errors are fixed.
- `ConfigurationIo`: `journal`, `prepare`, `write_journal`, `grant_batch`,
  `revoke_batch`, `publish`, `config_hash`, `clear_journal`. Only the recording
  test model implements this trait here.

Coordinator requirements before activation: authenticate a native session,
acquire the fixed transaction lock and validate elevation/current-binary file
identity/layout protections **before any I/O method**, including recovery.
Retain that authority and source/ancestor handles through use. `prepare` must
revalidate accounts, actual right observations and exact expected config bytes.
Implement exclusive protected SYSTEM-owner staging and atomic journal/config
publication, strict ACL/inheritance/owner policy and reparse/hardlink refusal;
never repair foreign objects. Resolve target-handle sharing for atomic replacement
without losing target identity. Define the fixed private journal path, abandoned
staging cleanup, and grant provenance retained after successful configure,
including uninstall/reconfigure/reinstall semantics. Do not use right presence
or `granted_batch_right` alone as a native uninstall receipt. Ambiguous intent
must never invent ownership. Hashes compare config bytes, not filesystem identity,
ACLs, journal origin or durability. Concurrent privileged account/right writers
require explicit treatment; the model does not serialize them.

Still absent: genuine native owner preconditions/file writes, SCM registration
and identical-only verification, fixed service-state mapping, bounded stop/remove,
LSA mutation, uninstall receipt storage and non-Windows native refusal entry.
Do not wire CLI/Application to this candidate until implementation and independent
review. Installed/native paths remain **unverified on real hardware**.

Actual Windows evidence: nine new recording/parser/plan tests and all 16
owner-plan/transaction tests passed. Two restored source mutations were killed
by assertions: bypass intent refusal; bypass old-config hash check before revoke.
Four config and two layout tests also passed. `cargo fmt --all -- --check`,
`cargo test --workspace --no-run` and
`cargo clippy --workspace --all-targets -- -D warnings` passed. These do not
establish native ACL/atomic rename/LSA/SCM behavior. Full workspace runtime
execution is excluded because existing tests launch `service run` (SCM dispatch)
and owner-command arguments, explicitly prohibited on this PC even for tests.

This M2 slice is library-only. It does not install, configure or start a service.
No agent-facing API or CLI selects these plans. The lifecycle production entry
continues to fail closed. No current-user execution config was added.

## API for later coordinator wiring

- `service::layout::Layout::system()` resolves fixed OS paths. Windows uses native
  `SHGetKnownFolderPath` with native x64 Program Files and ProgramData, never
  environment expansion. Its private test constructor is compiled only in unit
  tests; no binary override exists. Native local-disk/ancestor/reparse inspection
  remains a required adapter check, not a guarantee of this path constructor.
- `check_protection` checks explicit observations against separate private,
  binary-integrity and endpoint policies. Windows private objects require exact
  SYSTEM ownership, a protected present DACL and no foreign allow ACE, including
  inherit-only grants. Integrity permits read access and restricts write grants
  and ownership to SYSTEM, Administrators and TrustedInstaller. Unix policies
  require root:root, private dirs 0700/files 0600 and endpoint dirs 0755.
- `service::config::ServiceConfig::parse` caps input at 16 KiB, denies unknown
  fields recursively, requires explicit rights and validates pins, principals,
  hashes, limits, workspace disjointness and environment entries. `Resolver`
  observes user-vs-group identity, current administrative membership and existing
  metadata-only workspace safety. `revalidate` repeats those observations.
  Read-only getters expose validated runtime values for future integration.
  A read-only native resolver is now included (limitations below). The Unix allowlist is explicitly PATH,
  HOME, TMPDIR, CARGO_HOME, RUSTUP_HOME, LANG, LC_ALL and TZ; the ADR0007 source
  is absent from this base and coordinator reconciliation is required before
  wiring that executor. Windows uses ADR0009's exact allowlist.
- `service::owner_plan::{owner_plan, configure_plan, uninstall_plan}` require
  owner-context evidence. Install additionally requires the fixed running binary.
  `Plan::operations` exposes typed operations; `Plan::execute` stops on the first
  fixed-category error through `OsManager`. `MissingNativeAdapter` fails closed.
  The only implemented managers are unit-test recording/in-memory models.
- Install verifies existing trees before creation and registers constant
  artifacts without starting. The future adapter must implement identical-only
  idempotency for bytes AND protections, including SCM registration. Constants
  and manager argv are embedded; no shell text or account values enter argv.
  SCM registration carries the fixed service name, account, privileges, delayed
  start, failure policy and shutdown timeout; its adapter must quote the image
  as a single executable before the two fixed arguments.
- Configure serializes validated values with serde. `AtomicConfig` describes
  same-directory private temp creation, explicit service owner/protection, flush,
  comparison with expected old bytes, and atomic publication. None means
  exclusive create; a concurrent edit must refuse. Previous expected bytes are
  canonical serde encoding, so noncanonical existing files conservatively refuse.
  There is deliberately no native write primitive pretending a test-owned file
  has SYSTEM ownership. Linux adds/removes only the fixed standard drop-in and
  requests daemon-reload. Windows grants only the pinned build SID if the native
  observation says the right was absent, recording its own change in config.
  A prior validated config preserves that record; changing pins requires uninstall.
- Uninstall plans stop and deregister, remove only identical manager constants
  and remove a Windows batch right only through `uninstall_plan` with the prior
  validated ownership record. They never delete config/state/account/workspace.
  Native status adapters must map results to ADR0009's six fixed status tokens;
  raw manager output must never be forwarded.

## Remaining native work and acceptance boundary

Owner context remains injected evidence, not a security attestation. Pure policy
inputs remain injectable; native read-only adapters below obtain local evidence.
Integration must retain handles/no-follow checks, including all ancestors and the
binary directory. They must not trust booleans supplied by agents. No fake
establishes kernel ownership, privileges or account isolation.

A multi-step plan is not a transaction. It stops at the first failure and does
not repair or roll back existing objects. In particular, a Windows batch grant
can succeed before atomic config publication fails. Before enabling a native
configure adapter, implement durable recovery or compensation that preserves
whether this invocation actually added the right. The fake proves intent and
failure stopping, not crash-safe right ownership. Drop-in changes and config
publication likewise need native recovery/reconfiguration evidence. The adapter
must implement secure manager-artifact parent handling and exact existing
registration comparisons before mutating anything.

Installed production resolver/kernel ACL paths, atomic SYSTEM/root writes, SCM/LSA,
systemd/launchd operations, actual identity and installed-service behavior are
**unverified on real hardware**. Owner installation, account and rights setup
remain owner actions after integration/review; this slice authorizes none.

## Verification

Run `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
and `python scripts/test-service-artifacts.py -v`.
The Python tests parse INI and plist bytes using standard parsers. Read-only
systemd-analyze verification requires Linux and the fixed binary to exist;
plutil lint requires macOS. Both are unrun on this Windows host, not installation
acceptance. No UI change requires a UI rerun.

## Read-only native adapter slice

`service::native::{NativeResolver, observe_layout, read_config}` supplies local
observations without wiring the production Application or any CLI/MCP surface.
`observe_layout` returns handles with retained ancestors, not reusable path
attestations. `read_config` holds private-directory observations, validates the
opened config, reads at most 16 KiB + one sentinel byte, and validates protection
again on that same handle before parsing. It never calls foreground `private()`.
Errors remain the existing fixed categories, with no native error text or values.

Windows uses local SAM USER_INFO_23 SID equality, SidTypeUser and normal-user
classification, rejecting domain/group/service identities; local indirect group
membership is observed with NetUserGetLocalGroups (64 KiB preferred response,
1024-entry cap, no partial response accepted). SID/domain/name buffers are fixed
and bounded; native API-owned strings have bounded scans. USER_INFO_23 and
GetSecurityInfo allocate their documented native response before application
validation; no caller-supplied unbounded allocation length is used. Administrators
is recognized by SID rather than a localized name. Files use CreateFileW with
OPEN_REPARSE_POINT, metadata rights, READ_CONTROL and FILE_READ_DATA
(FILE_LIST_DIRECTORY on directories). Every ancestor stays open with read/write
sharing and no delete sharing through observation/read and while pins are held.
Unrelated sibling renames and moves into these ancestors remain possible.
Immutable final leaves use read-only sharing; mutable state leaves share
read/write without delete sharing. Owner/DACL namespace authority remains
essential: the retained handles alone do not exclude all modification.
GetDriveTypeW requires a fixed local drive. GetSecurityInfo supplies exact owner,
protected/present DACL and bounded ACE observations; unknown ACE types refuse.
Native allow ACE observations retain the full access mask. Final binary-integrity
checks still reject foreign write/delete/owner/DACL grants, including inherit-only
ACEs. Final private-object policy is unchanged. For shared ancestors only,
namespace policy rejects effective foreign DELETE_CHILD, DELETE, WRITE_DAC,
WRITE_OWNER and GENERIC_ALL, while permitting sibling creation and directory
EA/attributes, and ignoring inherit-only grants on that parent. Each existing
next component has its own owner/DACL and reparse observation; every successful
ancestor is nonempty. No stock path or ordinary group is allowlisted. Unknown
ACEs and untrusted owners refuse. Hard-linked config/binary files refuse.
See ADR 0013 for semantics and evidence. These are point-in-time observations;
no concurrent ACL freeze or race-free future symbolic reopen is claimed.

Unix uses descriptor-relative openat/O_NOFOLLOW, fstatfs, fstat metadata, and
retained ancestor descriptors. Linux accepts only ext-family, XFS, Btrfs, tmpfs
and F2FS filesystem magic values; unknown/network/overlay filesystems refuse.
macOS requires MNT_LOCAL. Trusted ancestry must satisfy root integrity policy.
Additional Linux access ACLs refuse; macOS uses native extended-ACL observation
and refuses entries rather than pretending mode bits describe their grants.
No symbolic-path canonicalization is performed. In particular, macOS /var and
/etc aliases are refused; the fixed macOS service layout uses explicit paths.

Linux identity retains the protected, bounded (1 MiB) local /etc/passwd
requirement, UID >= 1000, login shell and bounded NSS equality. getgrouplist
is capped at 1024 groups; getgrnam_r uses 64 KiB. No Linux cutoff was lowered.

macOS instead opens only OpenDirectory's `/Local/Default` node. Exact Users
queries cap results at two and require exactly one, with unique canonical
name, decimal UID and generated UUID queries agreeing on the full observed
profile. Requested attributes are limited to seven; dictionary/array/string
counts are checked before application traversal (16 keys, one value per
attribute, 1024 UTF-16 units). Local node location is required; original-node
attributes refuse mobile/cached directory records. Only ordinary login profiles
(UID >= 501, excluding root/reserved nobody, non-system name, nonzero group,
recognized Apple login shell and well-formed nonzero UUID) pass. This is a
conservative native Users/profile classification, not a claim that Apple has
a Windows-style normal-account flag. Aliased/multivalued names, custom shells,
and mobile/directory accounts deliberately refuse. UID 501 can pass only when
these native observations establish the local profile. NSS supplies no authority.
The unique local admin group is selected by its system group ID and canonical
name; ODRecordContainsMember observes direct/nested membership and errors refuse.
No command, authentication attribute, credential or account mutation is used.

Config retains nonserialized incarnation observations for both principals and
compares them on revalidation, covering canonical-name, UUID, group and admin
membership changes even when numeric pins remain equal. Fresh OD objects are
created on each observation; concurrent privileged edits are not transactional.
The optional resolver revision hook defaults to no additional evidence for
existing Windows/Linux adapters; their account and rights checks remain intact.
This observes admin status only. It enables no execution: macOS admin execution
must remain refused until a reviewed boundary contains root descendants. A
build-UID sweep after sudo does not establish descendant cleanup.

macOS ACL observation now uses fstatx_np and the filesec ACL-presence property:
a successfully absent property is empty; a present valid ACL must exhaust its
first-entry lookup with -1/EINVAL. Zero means an entry and refuses; retrieval,
property and validity errors refuse. Native temporary-file tests cover absent,
explicit empty, nonempty and invalid-descriptor cases. User-owned fixtures are
explicitly refused by root private policy. No owner, mode, ACL or ancestry
policy was relaxed. Native allocations are released on all owned paths.

Query limits and application buffer bounds do not bound native internal memory
allocation or elapsed time. ODQueryCopyResults(false), membership resolution,
and filesystem APIs can block; no enforceable deadline or cancellation is
claimed. A future service integration must isolate blocking observations if it
requires a hard deadline. Native macOS identity/ACL tests are gated to macOS
and remain **unverified on real hardware** on this Windows builder. Modern
local-Mac support is an unreviewed implementation candidate until hosted native
link/runtime checks pass; installed-service/M2 acceptance remains open.

These are point-in-time observations. Unix retained directory handles prevent
link traversal into a different object, but do not freeze renames; workspace
metadata is not an authorization for a later symbolic open. Trusted root-owned
ancestry excludes unprivileged replacement during config reads. Privileged
writers, mount changes, account/group edits, and in-place root writes are outside
that guarantee. Identity must be revalidated before use; later execution must
open workspaces after the identity switch. No observation proves a future
path-based store or endpoint open safe. Application wiring must preserve handles
or implement equivalent secure reopen/identity checks, verify strict TokenUser
at startup and all state/journal/lock objects, and cover runtime layout changes.

Remaining integration: production Application config/store/audit/endpoint hooks;
hosted OpenDirectory identity/ACL validation; privileged installed-layout tests; owner-context,
atomic config publication/recovery and SCM/LSA/systemd/launchd adapters. The
PR16 stable Windows checked-path correction at
`8c3856a8856548feeb233cd6960e920763412554` is not imported: this adapter validates
its own lexical drive ordering and uses native handles. Existing foreground
path behavior still depends on PR16. No owner setup, installation, process
launch, account/right/task or service-manager mutation was performed here.

Native API references used during implementation:
[NetUserGetLocalGroups](https://learn.microsoft.com/en-us/windows/win32/api/lmaccess/nf-lmaccess-netusergetlocalgroups)
and [Apple acl_get_entry](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/acl_get_entry.3.html).

## macOS rights policy correction

Configuration parsing and revalidation now reject macOS admin rights with
Error::Rights before account resolution or workspace observations. Configure
therefore returns no operation plan even for an object validated by an older
implementation. Standard mode still refuses an administrative build account.
Linux and Windows retain explicit admin-mode validation. This follows the
corrected ADR0009 containment limit; it does not establish native macOS cleanup.
The portable regression covers all twelve platform/rights/membership cases.

Apple contracts consulted for this revision:
[CFOpenDirectory APIs and ownership](https://github.com/apple-oss-distributions/DirectoryService/blob/main/Server/CFOpenDirectory/CFOpenDirectory.h),
[SDK match constants](https://github.com/phracker/MacOSX-SDKs/blob/master/MacOSX11.3.sdk/System/Library/Frameworks/OpenDirectory.framework/Versions/A/Frameworks/CFOpenDirectory.framework/Versions/A/Headers/CFOpenDirectoryConstants.h),
[filesec properties](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h),
[ACL retrieval](https://github.com/apple-oss-distributions/Libc/blob/main/posix1e/acl_file.c),
[filesec implementation](https://github.com/apple-oss-distributions/Libc/blob/main/gen/filesec.c),
[entry iteration](https://github.com/apple-oss-distributions/Libc/blob/main/posix1e/acl_entry.c),
[ACL text syntax](https://github.com/apple-oss-distributions/Libc/blob/main/posix1e/acl_translate.c),
and [native membership test contracts](https://github.com/apple-oss-distributions/Libinfo/blob/main/membership.subproj/membership.h).


## Fixed service Application candidate (Sol high, 2026-11-11)

Prepared from pinned base `7606c71b35ab906607c00efa96b99a76bca2409f`.
The actual SCM and Unix service entries now instantiate only the concrete native
Application. Test hooks are internal Rust injection points, with no runtime
selector. Foreground CLI/config and current-user development execution remain
separate. No service HTTP listener, config override or development switch exists.

Startup requires root:root or SYSTEM before reading fixed config, validates its
protected handle and client identity, verifies the fixed binary location and
protected layout, opens/reconciles the durable store, commits service_start,
binds the local endpoint, and performs dynamic execution preflight before ready.
Malformed static configuration or client identity is fatal. Missing/mismatched
build accounts produce fixed response-only account_missing (only on native-confirmed
absence), account_identity_changed or rights_mismatch as observed. Ambiguous native
lookup/policy failures remain preflight_failed. Windows LookupAccountName/Sid's
ERROR_NONE_MAPPED establishes absence; all other codes remain generic. Missing
local SAM membership after a successful lookup, group lookup errors, and current
Unix/macOS resolver failures remain generic rather than guessing their cause.
A successful identity/rights preflight still reports mechanism_unavailable.
Service inventory parsing and dynamic preflight perform only lexical workspace
disjointness checks: they never open the build workspace or resolve its PATH.
Workspace observation must run after switching identity in a future backend;
only authorized owner-local configure retains metadata validation today.
Neither outcome selects the development executor. No privileged PATH/tool
discovery is performed. Startup preflight details are held only in memory; refused
run_command/discover_tools responses carry execution_reason and complete with
the exhaustive ADR 0009 stored-reason mapping. The backend remains absent (no
per-run launch or workspace validation occurs), so these are startup observations,
not proof that a future run would pass preflight. SQLite schema 3 is unchanged.
macOS admin rights still refuse before any native account/revision observation.

Store integration deliberately requires owner-provisioned, existing
activity.lock, activity.sqlite3 and activity.sqlite3-journal in the fixed private
state directory. Empty DB/journal files are acceptable for first initialization.
Every file requires root:root/0600 on Unix, or SYSTEM ownership and a protected
SYSTEM/Administrators-only DACL on Windows. A nonempty database must have a
rollback-mode SQLite header before SQLite opens it: WAL headers refuse before
they can create inherited WAL/SHM files. SQLite temporary storage stays in memory.
The existing foreground checks also
run; they are not relaxed. WAL/SHM objects refuse. Service connections use SQLite
TRUNCATE journaling with FULL synchronization so journal deletion/recreation
cannot silently introduce an inherited Windows DACL. No ACL repair, ownership
change or broad ACL creation is attempted. Missing/unsafe state refuses with
service-store-or-drain (exit 5), before SQLite mutations. Provisioning these
files is an open owner-adapter integration requirement, not an owner action
performed by this candidate.

State handles and protected ancestry remain held for the store lifetime. On
Windows, immutable final handles exclude write/delete sharing, while ancestor,
mutable database, journal and lock handles share read/write but exclude deletion.
Ancestor ACL checks reject foreign namespace-control rights; write sharing
permits unrelated sibling renames and moves into the ancestor. Pins alone do
not freeze directory contents or ACLs. All request
FILE_READ_DATA (FILE_LIST_DIRECTORY for directories): metadata-only handles did
not actually prevent rename in the new normal-user regression. The strengthened
regression proves mutable writes succeed while rename/deletion fail, then rename
succeeds after releasing the pin. Unix no-follow descriptor traversal plus
root-owned nonwritable ancestry excludes unprivileged pathname replacement;
retained descriptors alone are not permission for a later unchecked open.
Concurrent privileged changes, mount changes and root in-place writes are
outside that guarantee. Installed SQLite behavior under actual SYSTEM/root
protection is **unverified on real hardware**.

### PR27 hot-journal recovery investigation (2026-11-11)

On prepared base bb077bb45f061c4512d9b2976bed92b7530b2f37, the normal-user
Windows crash fixture reproduced a pinned-store restart failure: SQLite extended
error 2570 (SQLITE_IOERR_DELETE). This is native Windows/SQLite evidence, not an
injected error model. A real child opens a service-mode store, commits activity,
starts another transaction, updates identity/activity, flushes dirty pages, and
signals readiness without committing. The parent terminates it without unwinding,
then verifies rollback-journal magic, a positive page-record count and size above
512 bytes before reopening. No hand-authored journal is substituted.

The fixture reuses the existing cfg(test) current-user service storage policy and
Application private-temp ACL helper. Test-only native adapters use the production
open_impl with identical FILE_READ_DATA, mutable write-sharing, no-delete-sharing,
retained ancestor handles, reparse/type and single-link checks. Actual deletion
attempts against all three pinned files fail before recovery. The fixture checks
current-user leaf ownership/private ACLs, rather than claiming SYSTEM ownership
or trusted production ancestor ACLs. It never selects the fixed installed paths.
This is a test policy only; production state_guard and all its ownership, DACL,
ancestor, header, hardlink, WAL/SHM and service-identity refusals are unchanged.

Moving journal_mode=TRUNCATE before the page reads alone still failed with 2570.
Bundled SQLite's journal_mode pragma has PragFlg_NeedSchema, so it can itself
recover before changing journal mode. The fix selects locking_mode=EXCLUSIVE
before that pragma, retaining the protected journal during recovery, then selects
TRUNCATE and returns to NORMAL before the bounded page checks and schema-3
transaction. The activity lock and native pins remain held throughout. SQLite's
pager_end_transaction uses journal-header invalidation instead of deletion for
DELETE mode under exclusive locking. See the [SQLite locking-mode contract](https://www.sqlite.org/pragma.html#pragma_locking_mode)
and [rollback recovery description](https://www.sqlite.org/lockingv3.html).
No journal is removed/recreated by application code, and no fence is relaxed.

Focused command: cargo test -p arxd --lib pinned_service_recovers_actual_hot_journal
--locked -- --nocapture. It failed on the original ordering and on the attempted
pragma-only reorder, then passed with temporary exclusive recovery. Removing
only the exclusive pragma deliberately restored the same 2570 failure; mutation
restored. Assertions verify stable host identity, rollback of uncommitted changes,
interrupted committed activity (including a child-created pending row), schema 3,
integrity_check=ok, TRUNCATE/NORMAL modes, caller-only readback without private
fields, persistent zero-length journal, no WAL/SHM, concurrent-store refusal and
a second successful reopen. Child output is discarded; readiness is bounded to
10 seconds, child lifetime to 20 seconds, with kill/wait cleanup on parent failure.

Windows checks passed: formatting, workspace/all-target Clippy with warnings
denied, workspace nextest (117/117 passed; two direct-only helpers skipped and
exercised by their parent tests), and git diff --check. This closes the reproduced
normal-user Windows persistence case only. Native Unix/macOS recovery and actual
SYSTEM/root installed recovery remain **unverified on real hardware**. Independent
other-provider review, hosted gates, owner provisioning, installed-manager and
dedicated-account/tool-probe/Claude Code M2 acceptance remain open. No installation,
elevation, owner operation, account/right/task/service/settings change, old Lab
Manager/VM access, push, PR, merge or watch occurred.

Unix service endpoints require the fixed root-owned 0755 parent; service umask
is 0077 and the bound socket becomes client-UID-owned 0600 before serving. The
foreground arbitrary endpoint retains its current-user private parent rule.
Windows retains explicit SYSTEM owner, specific client DACL rights excluding
pipe-instance creation, NETWORK denial and remote-client rejection. Both bridges
require root/SYSTEM as the expected identity for the fixed service endpoint, then
verify the kernel peer/pipe identity before sending credentials. Existing objects
are never reclaimed. The native Unix socket ownership path is unrun here.

Native startup/preflight/store/audit/bind hooks execute in tracked blocking jobs.
Cancellation stops awaiting a job; it does not cancel a stuck OS API. Drain waits
for outstanding jobs, stops the listener, aborts/joins inventory connections,
and durably marks unfinished agent reads failed/shutdown before the stop audit.
The Host retains its separate 10-second drain and 5-second stop-audit budgets;
the service runtime has a further one-second blocking-worker shutdown grace.
If a native call outlives the budget, no completed cleanup or hard API
cancellation is claimed. A late blocking audit may finish after its deadline;
its row is evidence of that commit, not evidence of a clean exit. Dedicated runs
and discovery must join this shutdown barrier when their backend is integrated.

Executed Windows evidence for this candidate: cargo fmt --all -- --check;
cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace
--no-fail-fast (106 passed; one directly ignored subprocess TokenOwner fixture is
exercised by its passing parent); python scripts/test-service-artifacts.py -v
(three passed, two native Unix checks skipped). New tests cover injected startup
failures/order without readiness, durable partial-start lifecycle rows, cancelled
blocking work/drain, actual normal-user named-pipe authentication/SDK inventory,
honest unavailable execution, caller/daemon isolation, persistent service-fixture
journal/restart identity/reconciliation, shutdown rows, strict service schema,
missing/mismatched build-account separation and fixed bridge identity refusal.
The application fixture uses an explicit current-user storage policy and injected
bindings; it is not a production-readiness or SYSTEM-layout acceptance test.

Integration obligations: merge the coordinator's independent credential-hash
uniqueness fix (this branch intentionally does not duplicate it); reconcile the
parent lifecycle/docs refresh; independent other-provider security review; native
Linux/macOS compile/link/runtime and systemd/plutil checks; disposable privileged
SCM/systemd/launchd readiness, protected files, socket/pipe identity and shutdown
experiments; owner adapters that provision the persistent journal safely; then
the separate dedicated-account executor and account-scoped tool probe. All
installed-manager/privileged paths remain **unverified on real hardware**. Full
M2 dedicated-account cargo/Claude Code/activity acceptance remains open. No new
architecture decision or owner-PC privileged operation is claimed.


## ADR 0012 pure owner provenance candidate (PR-A)

This replaces the obsolete Journal/ConfigurationIo compensation engine. The
Windows-only engine uses OwnerRecord (strict serde version 1, canonical ordinary
SID, lowercase RightState, deny unknown/duplicate fields and trailing data,
512-byte decode cap). The record and Configure have no Debug output. Errors and
RightReport display fixed tokens, never private owner data. Decoding bytes does
not establish authority: only the future SYSTEM-private OwnerStore under the
owner lock does. OwnerStore supplies exact-byte hashes for compare-and-replace;
BatchRight observes only the explicit SID's direct SeBatchLogonRight assignment.

API sequence for a future Windows session, all under one lock: recover(store,
right), observe_batch(build_sid, store, right), configure_plan(layout, context,
config, previous, Some(batch), resolver), Configure::from_plan(layout, plan,
resolver, batch), configure(request, store, right). BatchObservation is opaque,
Copy, and obtainable publicly only through observe_batch; neither MCP booleans
nor config mirrors can construct it. configure rereads the record and observation
before staging; pending Intent refuses, protected cross-SID provenance refuses,
and stale presence or recorded-grant evidence conflicts. A successful absent
observation plus successful grant can produce Granted. Intent recovery produces
None or Unproven, never Granted. Neither configure nor recover calls revoke.
Publication errors retain the record and never compensate.

Windows uninstall uses owner_plan(layout, context, Uninstall) without config or
resolver, then the future checked SCM stop/delete, then uninstall_right(store,
right), with recovery first. uninstall_right accepts no config, account name or
workspace; only a Granted record authorizes revoke, using its exact SID. The
compatibility uninstall_plan wrapper ignores previous/resolver on Windows and
emits no BatchRight operations. Plan::execute refuses Windows configure and
uninstall before applying any operation; future wiring must use the session.

Unix configure_plan uses the same position with None instead of the old ignored
batch-present bool. Some is rejected on Unix; None is rejected on Windows. Unix
config validation, rights/drop-in planning, previous-identity checks, uninstall
revalidation and direct Plan::execute semantics remain intact. This deliberately
requires a call-site adjustment to Unix candidate 7e12abf / PR29 by the coordinator;
it does not require Unix LSA or owner provenance. No legacy boolean Windows
configure route remains. The old granted_batch_right config field is used only
as a mirror and to reproduce canonical expected old bytes, never as authority.
This optional typed parameter is an implementation choice within ADR 0012.

The depth-6 model runs the real engine across every call prefix (cutoffs 0..15
plus completion), configure/uninstall with recovery, Ok/Err-with-effect/Err-
without-effect right mutations and external add/remove between commands. The
cutoff stops all subsequent adapter effects, making it equivalent to termination
at that boundary; all completed record publications remain visible. It merges
identical future-relevant states, including ghost history, rather than launching
redundant test processes. Independent ghost attribution records the last
acknowledged record write and whether the latest external add occurred outside
Granted authority; it does not reproduce engine branch decisions. P6b permits
membership rollback to either previously seen value with records retained. P6c
also permits record rollback to any previously seen record value (no lookback
cutoff), reproduces decision 6's exact counterexample and requires every unsafe
revoke to follow a restored Granted over a later non-Granted write. P6c is a
characterization outside the guarantee. Seeded receipt-boundary mutations that
take a pre-existing right or promote recovery to Granted must each produce unsafe
revoke witnesses under P6. These are bounded abstract state results, not physical
NTFS, LSA, power-loss or registry ordering evidence.

No native store, LSA, SCM, filesystem mutation adapter, CLI or MCP activation is
supplied here. D1 ancestor authority, D2 service workspace boundary, native
publication/locking/LSA/SCM tests, installed wiring/provisioning, dedicated-account
execution and installed M2 acceptance remain open. All native/installed paths
are **unverified on real hardware**. Accepted architecture is not implementation
acceptance; a fresh independent Opus review is required.
