# 0018: Owner CLI wiring (PR-D): routing, exit classes and client credentials

Status: **accepted contract; implementation remains gated by brief §0**.

S3 review clarification, 2026-11-12 (coordinator OpenAI / gpt-6.1-sol /
medium; pending independent Opus recheck): a stale previous account surfaces
as `service-identity`, exit 3. Existing full validation folds execution
preflight account failures into Identity before workspace checks.
`service-account-missing` comes from new-account lookup in `from_request`,
and natively only on Windows. This corrects brief test 5, preserves the fixed
token set and remedies, and changes neither `parse` nor `revalidate`.
The Windows start seam uses a deferred start closure so SCM construction
follows successful preflight and guard/pin release; the production ordering
is source-reviewed and the fake start sequence must equal a direct PR-C start.
The Opus proposal and first correction retain their independent Sol reviews;
the Sol correction-2 and advisory clarification retain their independent Opus
reviews. The latest clarification passed at dfcb643. Its small follow-up
advisories are included for the published-head recheck. No code was built,
behavior test run or native command executed for this contract. All native
paths remain **unverified on real hardware**; M2 is incomplete.

### Historical original proposal

Opus 5.5 high, 2026-11-11, original base
`2f53332b4f4d3e1584f2efb80317856588576e3a`. This was proposed pending
independent Sol review; the retained review chain above supersedes that gate.

### Historical correction 1

Correction 1 (Opus 5.5 high, design sub-agent), 2026-11-12, carried onto the
prepared root `780ca4164494f6c3da00a340f4c4c67e3ac9d305`. It answers the
independent Sol review
[0018-owner-cli-sol-review.md](../reviews/0018-owner-cli-sol-review.md)
(`6a6faae7`, changes required). Decisions 4 and 7 and the brief are corrected
for blockers A-SP1, A-SP2, A-SP3 and A-ST1 and advisories A-A1 and A-A2. The
dispositions are in
[0018-owner-cli-opus-correction.md](../reviews/0018-owner-cli-opus-correction.md).
The status is still **proposed**. A fresh independent Sol recheck is required
before any implementation. The design scope is unchanged.

### Historical correction 2

Correction 2 (OpenAI / gpt-6.1-sol / medium coordinator), 2026-11-12,
addresses the remaining A-SP1 and A-ST1 findings in the
[Sol recheck](../reviews/0018-owner-cli-corrections-sol-recheck.md).
The wipe owner now precedes RNG; the brief requires shared native-operation
error handling and Windows predicate fixtures. These are proposed contract
corrections, not executed tests. Fresh Opus review of this Sol delta is
required; the retained Sol reviews supply cross-provider review of the
unchanged Opus architecture. No implementation gate is waived.

## Scope

It settles only the wiring choices that ADR 0009, 0012, 0014, 0015 and 0016 leave
open for the `arxd service install|configure|start|stop|status|uninstall` and
`arxd client-config new` commands. The implementation brief is
[docs/briefs/owner-cli-wiring.md](../briefs/owner-cli-wiring.md). ADR 0017
(execution report validation) is unrelated and unchanged.

## Context

The libraries already provide each verb's authority: Windows `OwnerGuard`,
`owner_install_windows::install`, `WindowsOwnerSession::{open, uninstall}`,
`WindowsScm` and `uninstall_flow`; Unix `owner_plan`, `owner_unix::{execute,
status}`. `main.rs` routes only the exact `service run`. Four things are
missing or ambiguous. First, nothing turns owner flags into a `ServiceConfig`,
and the private `Wire` type allows serde construction only inside `config`.
Second, ADR 0012 lets Windows start, stop and status skip the image check, while
Unix `Manager::new()` requires root and the fixed binary for every verb. Third,
owner commands have no exit-code table. Fourth, `client-config new` creation
details are unspecified.

## Decisions

1. **Routing before Tokio.** If the first argument is `service` or
   `client-config`, the owner router owns the whole argv. It never falls through
   to the foreground parser. Exactly `service run` keeps calling
   `service::run()` unchanged. A pure, bounded parser handles every other
   owner argv. Any rejection prints `arxd: usage` and exits 2 before any
   native call, layout resolution or environment read. The foreground
   `mcp-stdio --client-config` and `--config` routes stay as they are. No owner
   command reads environment variables, accepts a path, layout, policy or
   config-file override, or is reachable from MCP or HTTP.
2. **One Windows owner guard for every service verb.** All six verbs start with
   `OwnerGuard::acquire()`: elevated, Administrators enabled, fixed binary chain
   and running-image identity. This is stricter than ADR 0012 step 3 for start,
   stop and status. It matches Unix, where `Manager::new()` already requires
   root and the fixed binary for every verb, including status. Start drops the
   guard and its preflight handles before `WindowsScm::start`.
3. **Start preflight checks inventory only, and it is advisory.** It checks the
   layout, parses the config with `parse_inventory` and checks the state
   members. It never calls `ServiceConfig::parse`, `workspace_safe`, a
   build-account lookup or the execution preflight. A missing build account or
   workspace therefore never blocks start: the service starts, and execution
   reports unavailable. Unix uses the existing plan step `StartPreflight`.
   Windows adds `native::owner_start_preflight`. The service re-verifies
   everything when it starts. The preflight grants no authority.
4. **Configure builds the new config from exact flags, and the previous
   config must still revalidate.** The new config holds exactly the given
   flags plus fixed limits: `max_concurrent_runs` 1, `memory_bytes`
   8589934592, `pids` 512 (ADR 0009's example). No flag changes them, and no
   value is merged from the previous config. A new
   `ServiceConfig::from_request` resolves the two account names, builds `Wire`
   through serde, and returns the result of `ServiceConfig::parse` on those
   bytes.

   The previous config is not ignored. If it exists, configure keeps the
   library behaviour unchanged. The CLI parses it with the full
   `ServiceConfig::parse`, which revalidates accounts, revisions, the
   execution preflight and `workspace_safe`. `configure_plan` then calls
   `old.revalidate` again and compares the build pin. The previous bytes are
   also the compare-and-replace expectation. A previous config that no longer
   revalidates therefore refuses configure, even when the new flags are valid.
   For example, the pinned account is unchanged, the old workspace has been
   removed, and a new valid workspace is given. Configure exits 3
   (`service-workspace`) and makes no store, right or config write.

   The owner has two manual remedies, and neither adds a command or override.
   One is to restore the old workspace so that it passes `workspace_safe`
   again, then rerun configure with the new flags. The other is the existing
   ADR 0009 owner step: uninstall, delete the config by hand, install and
   configure. The same remedies apply when an old account fact no longer
   revalidates (`service-identity` or `service-account-missing`). This CLI
   wiring does not weaken `configure_plan`, `parse` or `revalidate`.
   Replacing a config whose old execution facts are stale would need its own
   accepted API and ADR change.
5. **Uninstall reads no config, account or workspace.** Windows runs
   `uninstall_flow(deregister, |proof| WindowsOwnerSession::uninstall(guard, proof))`.
   Unix runs `owner_plan(Uninstall)` and then `execute`. It never calls
   `uninstall_plan`, which requires a config that revalidates; the ADR 0014
   matrix requires install → uninstall to work with no config.
6. **Owner exit classes.** Stderr carries exactly one line, `arxd: <token>`, with
   a fixed token. The exit code follows the class in the brief: 2 usage; 3
   config, identity, account, rights or workspace; 4 permissions,
   owner-required or elevation; 7 layout, binary or missing adapter; 1 manager,
   registration-state, owner-transaction or client-config I/O. Every match is
   exhaustive, so a new variant fails to compile until it is classified.
   `Permissions` and `OwnerRequired` always exit 4, which preserves ADR 0009's
   code-4 rule that correction `7781239` restores for startup. Stdout is empty
   except for three cases: status prints one of the six tokens; Windows
   uninstall prints one `batch-right-*` token; `client-config new` prints one
   line, `<principal>=<sha256>`.
7. **`client-config new` (ADR 0009) details.**
   - It refuses an elevated or root process: Windows `TokenIsElevated`, or a
     SYSTEM, LocalService or NetworkService user; Unix a real or effective UID
     of 0, or real and effective IDs that differ.
   - The `--out` path must be absolute and normalized. On Windows it must use a
     drive letter, and no component may be a reserved device name.
   - Existing ancestors are walked without following links and must contain no
     link or reparse point.
   - Only the immediate parent may be created. It is created private and owned
     by the current user: Unix `0700`; Windows owner TokenUser with protected
     DACL `D:P(A;OICI;FA;;;<user>)`. An existing parent must already be a
     private directory owned by the current user. It is never chmodded,
     chowned or re-ACLed.
   - The file is created exclusively and private from its first byte: Unix
     `O_EXCL|O_NOFOLLOW`, `0600`, relative to the pinned parent descriptor;
     Windows `CREATE_NEW`, share 0, owner TokenUser, `D:P(A;;FA;;;<user>)`,
     with every ancestor handle held. An existing target refuses and is never
     opened for writing, truncated, re-moded, re-ACLed or overwritten.
   - The contents are serde JSON with exactly three keys: `address` (the fixed
     `Layout::system()` endpoint), `expected_server_identity` (`S-1-5-18` or
     `0`) and `token`. The token is 32 bytes from `getrandom`, written as 64
     lowercase hex characters.
   - **Digest first, then clearing (A-SP1).** The command encodes the token
     text exactly once. It computes `sha256` of exactly those 64 ASCII bytes
     and keeps only that non-secret digest. Only then does it build the JSON.
     The JSON borrows the same token text rather than copying it. A wipe
     owner holds initialized raw, token and JSON storage before RNG is
     invoked. On success, every ordinary error (including RNG partially
     filling storage before Err), and unwinding, that owner clears all
     initialized secret storage before releasing it. Encoding and serialization
     use its storage directly; the non-secret digest alone is retained outside. On
     success it returns the retained digest. The command never hashes a
     buffer after clearing it, and never hashes anything other than the token
     text written to the file.
   - The command does not load the file through `ClientConfig::load`, because
     that load validates the endpoint directory, and the directory exists only
     while the service runs. It checks the created handle instead. Production
     code never reopens the created file by path. The checks use the handle
     from the exclusive create.
   - **The successful exclusive create is the boundary (A-SP2).** A failure
     before it is `client-config-argument`, `-elevated`, `-parent`,
     `-exists` (the target already existed) or `-io`, and it creates no
     credential file. Every failure after it is `client-config-partial`. That includes the Unix
     `fchmod(0600)`, the write, the flush, the Unix parent fsync and every
     handle check. On a partial failure the command prints no hash and no
     token, leaves the private file in place, and rolls nothing back: no
     unlink, delete disposition, delete-on-close or truncation. A parent
     directory the command created earlier stays as well, private and
     unchanged.
   - **What success and partial mean (A-A1).** On success, the file was
     created exclusively, fully written, flushed and checked on its handle,
     and the printed hash equals `sha256` of the token text in that file.
     Success does not register anything. The owner passes the printed line to
     `configure`. After a partial failure, the command supplied no
     registration hash. It makes no claim that the retained file holds a
     complete or usable credential. The file may still hold a whole token,
     and an owner who can read it could compute its hash. That is not an
     authorization fence. The documented remedy is to delete the private file
     by hand and rerun the command with a new path or after the deletion.
   - The token never appears in argv, the environment, stdout, stderr, logs or
     Debug output. Clearing its buffers is best effort, not a guarantee: the
     allocator, serde growth or the OS can keep copies. To rotate, the owner
     creates a new file and reruns configure. Arxcovo never deletes old files.
   - **Windows private rule (A-A2).** Handle checks on the created file and on
     the parent use a strict allow-only rule. The DACL is present. Every ACE
     is `ACCESS_ALLOWED_ACE_TYPE`, for TokenUser, SYSTEM or BUILTIN\
     Administrators only. Any other ACE type refuses, including a deny ACE.
     This is deliberately stricter than the current `security::private`
     helper, which skips deny ACEs. Anything this rule accepts, that helper
     accepts too, so a created file still passes `ClientConfig::load`. The
     command never calls the path-based helper in production.

## Deviations and new strictness

| Item | Source | Kind |
| --- | --- | --- |
| Guard on start, stop and status (Windows) | ADR 0012 step 3 | new strictness; matches Unix |
| Fixed limits, no flag | ADR 0009 example values | settles an unspecified choice |
| Unix uninstall uses `owner_plan`, not `uninstall_plan` | ADR 0014 matrix | uses the existing API, adds no authority |
| Elevated or root `client-config new` refused | ADR 0009 "unelevated" | new strictness |
| Usage text becomes `arxd: usage` | `ExitCode::Usage.category()` | wording change for owner routes only |
| Previous config must revalidate (correction 1) | `configure_plan`, `ServiceConfig::parse` | keeps library behaviour; narrows the original full-replacement claim |
| Allow-only Windows DACL rule for client files (correction 1) | `security::private` skips deny ACEs | new strictness for owner-created client files only |

## Not decided here (explicit gaps, no new authority)

- **Changing the build pin.** Configure refuses a new build identity, and the
  ADR 0012 record refuses a cross-SID change. Uninstall keeps the config (ADR
  0009). The only path is therefore the existing ADR 0009 owner step: uninstall,
  then delete the config by hand, then install and configure. No command,
  prompt or override is added. A supported pin-change command would need its
  own decision.
- **Replacing a config whose old facts no longer revalidate.** Decision 4
  keeps that refusal. The manual remedies there are the only path. Any
  relaxation would need an accepted API and ADR change.
- **Running Windows start.** It is unverified whether the preflight can open
  the state files while the service holds them. If it cannot, start fails
  before any SCM mutation, and stopping the service first settles it. The
  disposable experiment must establish this. It must never run on the owner PC.

## Consequences

PR-D can be built in three bounded slices (see the brief). It adds no product
setting, prompt, approval, recovery verb, account creation, password, S4U
retry or development fallback. Installed SCM, LSA, systemd and launchd
behaviour stays **unverified on real hardware**. macOS owner operations still
stop on the `Unknown` launchd query before they mutate anything.

## Historical coordinator disposition — 2026-11-12

Accepted as a documentation contract after independent Sol review of the Opus
proposal/correction and independent Opus review of the Sol correction at
`d11dcdc`. [Final recheck](../reviews/0018-owner-cli-second-correction-opus-recheck.md)
closes A-SP1/A-ST1; earlier PASS dispositions are retained. This status change
adds no implementation or runtime authority. Before S2 starts, resolve the
final recheck's N1–N5 advisories in its implementation brief: unreachable
serialization-error test, stale tag, borrowed-handle method shapes, Unix
mode/length negative fixtures and escaping-aware no-growth JSON capacity.
Every library, hosted and independent code-review gate remains binding.
E2 stays held on unexplained run bdbf032b; no CLI implementation starts now.
Native, installed and dedicated paths are **unverified on real hardware**.

## Historical advisory clarification candidate — 2026-11-12

The core contract remains accepted; the brief's N1–N5 clarification is pending
independent Opus review before publication/implementation. It removes an
unreachable serialization-error test and a stale tag, names the borrowed-handle
operation shapes and NativeFileOps, adds real Unix mode/length negatives and
requires escaping-aware JSON capacity with unchanged-pointer/capacity tests.
These clarify the final review's recommendations and add no authority or
runtime behavior. Explicit buffer wiping remains best effort; compiler,
allocator or OS copies remain outside that guarantee. JSON growth is forbidden
by the brief rather than treated as acceptable loss of a wiped allocation.

Actual preparation parent is main after #45, 2b805c4. E2 is draft #46 and
implementation gates remain closed. Nothing here starts owner CLI work,
executes a guide command or accepts M2. Native, installed and dedicated paths
remain **unverified on real hardware**.

## Publication checkpoint after E2 merged — 2026-11-12

Actual parent is main 1d828ca after PR46. E2 and the other prerequisite
library PRs passed their hosted gates; no installed/native acceptance follows.
The [advisory clarification review](../reviews/0018-advisory-clarification-opus-review.md)
passed on both axes at dfcb643. The brief now also names the test-only
allocation observer, specifies each deliberately invalid Unix fixture's end
state, maps bound overflow to Io before RNG and fixes historical reviewer
labels. A fresh published-head review covers those follow-ups. Publication
and review do not themselves begin S1–S3; every brief §0 gate still applies.
The unresolved ExecutionFailed remains tracked; no fix or waiver is claimed.


Implementation-gate checkpoint: PR47 merged as d70e808 after all five gates
on d9b6fa3 and independent exact-head Opus review. The brief now names that
publication gate explicitly and states that all unaffected safety assertions
still apply to deliberately invalid Verify fixtures. These wording follow-ups
implement the final non-blocking review notes, with no new authority.

## S3 clarification review checkpoint, 2026-11-12

Independent Opus high recheck 98a5bb5 accepts the coordinator's c41fbb2
clarification: old missing accounts give service-identity, with unchanged
library semantics. On Windows and Linux this occurs through full validation;
on macOS the earlier account lookup in parse can refuse first. The token,
exit class and refusal before the old workspace check hold on all platforms.
The deferred start closure is accepted; native guard release and SCM ordering
remain source-reviewed. No native or installed acceptance is granted.
