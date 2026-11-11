# ADR0015 implementation handoff

Provider: OpenAI gpt-6.1-sol / high. Prepared base verified clean:
`a24926310410239116b0bba0c888457d4c38109e`. Worked alone, no delegates.
Implementation commit: `93b06a8fe4738f7195cece0885594262da13e97a`.
Implementation candidate only; native acceptance and independent Opus review pending.
No push, PR write, merge, watch, manager or installed-service operation.

## APIs and merge ownership

- `layout::check_unix_ancestor(Platform, &Protection)` is pure. Native ACL
  observation must succeed first. Mac ignores only ancestor GID; Linux and all
  leaf policies retain exact GID zero. Windows policy is unchanged.
- `native_unix::open` uses that rule only while descending. `Fs::parent` uses
  the same rule through `Fs::ancestor`; `Fs::check` remains strict for every
  final object, including binaries. UID/GID normalization exists only in the
  private test seam; production creation explicitly uses `(0, 0)`.
- `Fs::new_owner` runs after exclusive directory/file creation and before
  chmod/final checks. Config temporaries use `Fs::create`. Existing objects
  never enter this helper. Portable fchown is used on Linux too, as ADR0015 allows.
- `native::ensure_endpoint_directory(&Layout) -> Result<Observed, Error>` is
  macOS-only. It checks the protected existing generic endpoint grandparent,
  creates only its missing final directory, treats EEXIST as check-only, and
  retains its parent descriptors. Its private `endpoint_child` helper is tested
  only with owned temporary directories and cfg(test) owner IDs.
- Native Application layout invokes creation before observation, mapping its
  failures to exit 7. Application pins survive until release; Listener retains
  its existing independent endpoint guard. Linux still requires manager creation.
- macOS endpoint is the fixed 50-byte ADR0015 address. Workspace checks now also
  exclude its runtime directory and descendants. State/config/binary unchanged.
  Socket chown explicitly preserves configured client UID and sets GID zero.
  Client kernel-root identity-before-credential ordering is unchanged.

Phase 3b owns StartPreflight/ProvisionState. No changes here to their unsupported
scan, plans, manager gates or provision method. Merge overlaps are the shared
`owner_unix.rs` Fs impl and adjacent tests, plus appended PROGRESS history.
Preserve the new ancestor/leaf split and creation-only ownership helper when
integrating 3b; do not duplicate the Mac creator or move it into owner install.
No shared-checkout coordination or writes to the other builder occurred.

## Observed checks

- Final formatting check, workspace/all-target Clippy with warnings denied,
  workspace test compilation passed on Windows.
- Scoped service library suite: 63 passed, zero ignored. Filters excluded
  windows owner fixtures, production normal-user load and non-Unix execution.
  Local bridge suite: seven passed, including fixed server identity refusal.
  Integration service-entry tests were compiled but not run because they invoke
  the production service entry.
- Five layout tests passed after correcting runtime-directory workspace overlap.
- Actual-source Linux x86_64 and macOS arm64 all-target Clippy passed as metadata
  checks. They used LIBSQLITE3_SYS_USE_PKG_CONFIG=1 and SQLITE3_LIB_DIR pointing
  at the checkout for link hints, as in the prepared base. No fake library or
  compiler shim was created, no linker ran, and no Unix runtime/SQLite build is
  claimed. The native tests were checked, not executed.
- Actual source mutations killed by `unix_ancestor_and_leaf_roles`: Linux accepts
  arbitrary GID; ancestor mode mask omits group write; leaf accepts arbitrary Mac
  GID. Original bytes restored, scoped suite green afterward. An initial group
  write mutation anchor did not match; the corrected actual mutation was run.
- **Unrun mutations:** skipping newly-created-object fchown and chowning an
  existing object. On Mac, mutate Fs new_owner/create/directory and endpoint_child
  separately; the foreign-own-group creation assertions must fail when chown is
  omitted, and unchanged-GID/refusal assertions must fail for existing repair.
  Their ordinary-user fixtures require another existing own member group and
  fail explicitly if unavailable, rather than silently skipping or adding groups.

Test-selection incident: the initial broad service filter accidentally invoked
existing `production_refuses_normal_user_before_fixed_config_access` and
`current_process_owner_change_is_isolated_and_real`. The latter launches its
existing ignored TokenOwner subprocess fixture. Both passed; this exceeded the
requested test boundary and was not repeated. The initial run also exposed the
layout regression above (65 passed, one failed, one ignored). This is recorded
rather than claiming no production hook/ignored fixture ran. No actual SCM,
launchd/systemd, owner constructor, installation or elevation was performed.

## Required coordinator/native evidence

Run the existing fixed hosted observation test on the exact final hosted job.
It now reports ten fixed targets, booleans only, including
`per_directory_ancestor_accepts`, and asserts all required positive controls.
Root, Library, Application Support, private, var, Logs, LaunchDaemons and bin
must pass as ancestors; launchctl must pass leaf binary integrity. The old
runtime parent, old protected runtime walk and Application Support leaf must
refuse. Any positive failure stops acceptance and returns to architecture;
retain safe WIP and invent no path relocation or policy allowance.

The historical `docs/evidence/macos-stock-directory-observations.json` is
unchanged. Only the coordinator can replace it from an exact new job capture;
no new image facts or native capture are claimed here. Existing CI already
runs the exact observation fixture with nocapture before the full native suite.

Execute ordinary-user Unix Fs and endpoint fixtures, existing native ACL tests,
old-address refusal and bridge ordering tests, plus the two unrun mutations.
New fixtures cover foreign member groups, no-repair inode/mode/GID checks,
group/other writes, special bits, ACL-bearing Mac ancestor, symlink and regular
file refusal. Never invoke production constructors or ignored privileged
fixtures, and never add group membership or elevation for these tests.

Installed root creation/binding, cross-user traversal and peer authentication,
launchd logs, stale socket recovery, dedicated-account execution and real M2
acceptance remain **unverified on real hardware**. This candidate does not
activate CLI/owner paths, add a fallback, or close phase 3b or M2.
