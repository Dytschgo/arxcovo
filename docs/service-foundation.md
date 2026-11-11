# Service config and owner-plan foundation

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
OPEN_REPARSE_POINT, metadata rights, READ_CONTROL, and read-only sharing. Every
ancestor stays open without write/delete sharing through observation/read.
GetDriveTypeW requires a fixed local drive. GetSecurityInfo supplies exact owner,
protected/present DACL and bounded ACE observations; unknown ACE types refuse.
Generic and specific write/delete/owner/DACL grants count as writes, including
inherit-only ACEs. Hard-linked config/binary files refuse.

Unix uses descriptor-relative openat/O_NOFOLLOW, fstatfs, fstat metadata, and
retained ancestor descriptors. Linux accepts only ext-family, XFS, Btrfs, tmpfs
and F2FS filesystem magic values; unknown/network/overlay filesystems refuse.
macOS requires MNT_LOCAL. Trusted ancestry must satisfy root integrity policy.
Additional Linux access ACLs refuse; macOS uses native extended-ACL observation
and refuses entries rather than pretending mode bits describe their grants.
No symbolic-path canonicalization is performed. In particular, macOS /var and
/etc aliases are refused; the fixed macOS service layout uses explicit paths.

Unix identity is deliberately narrower than the eventual installation contract:
only a unique user in the protected, bounded (1 MiB) local /etc/passwd database,
UID >= 1000 and a login shell, matching bounded getpwnam_r observation, can pass.
getgrouplist is capped at 1024 groups and getgrnam_r at 64 KiB; Linux sudo/wheel
membership is checked in both rights directions by the existing config policy.
This is **not complete macOS identity support**: modern OpenDirectory users and
UID 501 accounts refuse, as does the /etc link. A reviewed local OpenDirectory
adapter with explicit user/service classification, bounded records and native
admin-group membership remains required. No NSS-only or current-user fallback
is used. Linux installations outside the conservative local-user profile also
refuse pending explicit native classification. No claim about arbitrary sudoers
policy or actual elevated execution follows from group membership alone.

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
OpenDirectory identity support; privileged installed-layout tests; owner-context,
atomic config publication/recovery and SCM/LSA/systemd/launchd adapters. The
PR16 stable Windows checked-path correction at
`8c3856a8856548feeb233cd6960e920763412554` is not imported: this adapter validates
its own lexical drive ordering and uses native handles. Existing foreground
path behavior still depends on PR16. No owner setup, installation, process
launch, account/right/task or service-manager mutation was performed here.

Native API references used during implementation:
[NetUserGetLocalGroups](https://learn.microsoft.com/en-us/windows/win32/api/lmaccess/nf-lmaccess-netusergetlocalgroups)
and [Apple acl_get_entry](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/acl_get_entry.3.html).