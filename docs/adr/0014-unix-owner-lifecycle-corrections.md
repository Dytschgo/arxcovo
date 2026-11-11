# 0014: Unix owner lifecycle corrections (boot intent, runtime directory, absent jobs, status)

Status: settled contract (Opus 5.5 high, architecture), 2026-11-11. It corrects
the PR29 review (comment 6798330477) at base `a8c98347`. It amends ADR 0009 only
where stated below. This ADR is a documentation contract, not an
implementation. Every manager behaviour named here is **unverified on real
hardware** until native evidence is recorded. ADR numbers 0012 (Windows owner
recovery) and 0013 (Windows ancestor policy) are reserved for those Windows
topics.

Correction, same day, at base `33f753a0`: the independent review in
[docs/reviews/0014-unix-lifecycle-contract-review.md](../reviews/0014-unix-lifecycle-contract-review.md)
found that install could leave a reinstalled service running or enabled.
Decisions 1, 3 and 6, the plans, the matrix and the tests below are corrected
in place, and decision 7 settles install retry and reinstall. No implementation
exists yet, so nothing is superseded in code.

Amended by ADR 0015 for the macOS pass-through policy, created-object group
ownership and fixed endpoint location. Its boot and lifecycle rules otherwise
remain as specified here; the old group-writable runtime parent stays refused.

## Context

The PR29 review found these defects:

- **Endpoint directory.** The owner start preflight requires `/run/arxcovo`.
  systemd's `RuntimeDirectory=` deletes that directory on every stop, and it is
  also gone after a reboot. macOS clears `/private/var/run` at boot, and nothing
  in the code creates `/private/var/run/arxcovo` there, even though ADR 0009
  says the service creates it.
- **Unloaded launchd jobs.** `launchctl bootout` exits nonzero when the job is
  not loaded, so install → uninstall and stop → uninstall halt.
- **Boot behaviour.** It differs by OS. On macOS, `enable` persists across a
  stop. On Linux, the unit is never enabled.
- **Smaller issues.** Status collapses into `Manager` errors. `enable` and
  `disable` bypass the command allowlist. Nothing tests the manager calls.

## Decisions

### 1. Boot intent: the last explicit start or stop wins

There is one ordinary rule for every OS: the last explicit lifecycle command
that succeeded decides boot intent. `install` registers the service, never
starts or stops it, and leaves it off at boot. Install refuses while the
service is active, so it never overrides a running service (decision 7).
`start` means running now and at every boot. `stop` means stopped now and not
started at boot. In both directions the persistence change runs before the
runtime change, so a failed runtime step never leaves the opposite boot intent
behind. A failed command keeps any persistence change it already made:

- if the `start`/`bootstrap` step of `start` fails, the service is left
  enabled but not running;
- if the `stop`/`bootout` step of `stop` fails, the service is left disabled
  but possibly running.

Rerunning either command settles the state. A later successful `install` also
settles a stopped-but-enabled service to off at boot. No setting, prompt or
approval exists for this.

| | install | start | stop |
| --- | --- | --- | --- |
| Linux | drop-in (only if the unit is absent), unit, `daemon-reload`, then `disable`; never `enable` | `enable`, then `start` | `disable`, then `stop` |
| macOS | `disable` (the job is proven not loaded), then plist with `Disabled=true` | `enable`, then `bootstrap` (only if not loaded) | `disable`, then `bootout` (only if loaded) |

The macOS plist keeps `Disabled=true`. The plist key list in ADR 0009 gains
`Disabled`. `launchctl enable|disable system/com.arxcovo.arxd` and
`systemctl enable|disable arxd.service` join the fixed vectors in ADR 0009.

**Windows intent (no code change here).** The same rule applies. The existing
SCM registration in ADR 0009 is auto-start (delayed), and stop does not change
the start type. As a result, an installed but never-started Windows service
starts at the next boot, and a stopped one starts again after reboot. This is a
known divergence. The Windows owner adapter aligns it: register demand-start,
switch to delayed auto-start before `StartService`, and switch back to demand
after a confirmed stop. Until that lands, Windows boot behaviour is not claimed
to match this rule.

### 2. Endpoint runtime directory: the manager or the service owns its lifetime

- **Owner `install`** no longer creates the endpoint directory on either OS.
  The `EnsureDirectory{EndpointDirectory}` step is removed from the plan and
  from the allowlist.
- **Owner `start` preflight.** A missing endpoint directory is accepted. If the
  directory exists, it must still pass the full check: no-follow, root:root,
  `0755`, no ACL, protected ancestry. An existing unsafe directory refuses
  before any manager command. The config, binary, private directories and
  state remain required.
- **Linux service.** Unchanged: systemd creates `/run/arxcovo` before
  `ExecStart`. The service still *requires* the directory and exits 7 without
  it. It never creates it.
- **macOS service.** Before `observe_layout` and bind, the service runs
  `ensure_endpoint_directory`. It walks to `/private/var/run` with
  descriptor-relative no-follow opens under the same ancestor rule as
  `native::checked`, then calls `mkdirat(parent, "arxcovo", 0755)`:
  - **Created:** `fchmod(0755)` on that new handle only (the umask is 0077),
    then check root:wheel, `0755` and no ACL.
  - **`EEXIST`:** open it no-follow and check it. Never chmod, chown, repair or
    remove an existing directory.
  - **Any other failure:** refuse with exit 7.

  The parent is never created. The handle stays pinned with the listener, as
  `endpoint_guard` does today.
- **Not resolved here.** On stock macOS, `/private/var/run` (and
  `/Library/Application Support`) may fail the strict root:root,
  no-group-write ancestor rule. Creating the directory does **not** by itself
  prove macOS startup. The stock-ancestor decision stays a separate open item.

### 3. Typed manager query; known absence is never inferred from exit codes

A query runs a fixed, allowlisted, read-only vector and captures bounded
stdout. stderr is discarded. The query has a 10 s limit and returns a typed
result, never raw text:

`Absent | Ours(state) | Foreign | Unknown`

**Linux.** The vector is
`systemctl show --property=LoadState,ActiveState,FragmentPath,DropInPaths arxd.service`,
with output capped at 4 KiB. Every `Key=Value` line must appear exactly once,
in any order.

- **Absent:** `LoadState=not-found`, an empty `FragmentPath` and
  `DropInPaths`, and `ActiveState=inactive`. A contradictory active,
  transitioning or failed not-found observation is Unknown and cannot prove stop.
- **Ours:** `loaded`, with `FragmentPath=/etc/systemd/system/arxd.service` and
  `DropInPaths` either empty or exactly the fixed drop-in path.
- **Foreign:** any other path, a masked unit or another drop-in.
- **Unknown:** anything else, including a nonzero exit, invalid UTF-8,
  oversize output, a missing or duplicate key, an unrecognised `LoadState`, or
  an `ActiveState` outside `active`, `reloading`, `activating`,
  `deactivating`, `inactive` and `failed`.

**macOS.** The vector is `launchctl print system/com.arxcovo.arxd`, capped at
64 KiB. launchctl(1) says `print` output is not an API, so parsing fails closed.

- **Ours:** exit 0 and exactly one top-level
  `path = /Library/LaunchDaemons/com.arxcovo.arxd.plist`.
- **Foreign:** exit 0 with a different `path`.
- **Absent:** only the exit status pinned in a constant **from captured native
  evidence** (see Remaining evidence). Until that constant is pinned, every
  nonzero exit is Unknown. No exit code is guessed.
- **Unknown:** everything else.

The query is combined with the on-disk check: the fixed artifact must be absent
or byte-identical with root:root `0644` and nlink 1. Linux checks the unit and
the drop-in. A present artifact that is not identical is Foreign.

Derived predicates, used by the expectations and gates below:

- **Active.** On Linux: Ours with `ActiveState` `active`, `reloading`,
  `activating` or `deactivating`. On macOS: Ours, in any state. A loaded
  launchd job can be run by launchd at any time, so it counts as active.
- **Inactive.** On Linux: Ours with `inactive` or `failed`. On macOS: Absent.

New plan operations:

- `Expect(Installable | NotForeign | Installed | Unloaded)`:
  - `Installable`: the install precondition in decision 7.
  - `NotForeign`: the artifacts are absent or identical, and the query is
    neither Foreign nor Unknown.
  - `Installed`: the primary artifact (the unit, or the plist) is identical and
    present. Linux also requires Ours, and accepts the drop-in either absent or
    identical, because `configure` owns it after install.
  - `Unloaded`: Linux reports Absent or Inactive, and macOS reports Absent.
- `Command{program, args, when}`. `when` is one of:
  - `Always`;
  - `Loaded`: the query reports Ours (on Linux the unit is loaded, which does
    not imply that it is active);
  - `NotLoaded`: the query reports Absent;
  - `ArtifactPresent`: the primary artifact is present and identical.

  A gated command re-queries immediately before it runs. If the gate is false,
  the step is skipped and recorded as skipped. Foreign or Unknown halts.
- `Constant{path, bytes, when}`. `when` is `Always` or `UnitAbsent`. Only the
  Linux install drop-in uses `UnitAbsent`, which skips the step when the unit
  is already present (decision 7). The configure drop-in keeps `Always`.
- `StartPreflight` replaces the hidden preflight inside `Command`.
- `ProvisionState`. See decision 6.

New fixed errors: `NotInstalled` (`service-not-installed`), `Foreign`
(`service-foreign-registration`) and `Active` (`service-active`).

Halt rules:

- A query result of Foreign halts with `Foreign`, and Unknown halts with
  `Manager`. A failed `Expect` makes no change of its own.
- `Expect(Installable)` halts with `Active` when the job is Active.
  `Expect(Installed)` halts with `NotInstalled`. `Expect(Unloaded)` halts with
  `Manager`.
- Every mutating vector that exits nonzero, is signalled, or fails to spawn
  halts the plan with `Manager`. No later step runs, so no artifact is removed.
- A timeout means kill, at most 1 s of reaping, then `Manager`. It is not
  evidence that the service stopped.
- `Expect(Unloaded)` after `stop` or `bootout` is the only proof of stop, and
  it is required before any artifact is removed.

### 4. Fixed status

Status is a read-only function that returns a token, not a plan. It still
requires root, like the other owner subcommands.

| Linux artifact + query | Token |
| --- | --- |
| unit absent, Absent | `not-installed` |
| identical, Ours, `active` or `reloading` | `running` |
| identical, Ours, `activating` | `starting` |
| identical, Ours, `deactivating` | `stopping` |
| identical, Ours, `inactive` | `stopped` |
| identical, Ours, `failed` | `failed` |

| macOS plist + query | Token |
| --- | --- |
| absent, Absent | `not-installed` |
| identical, Absent | `stopped` |
| identical, Ours | the top-level `state` mapped through fixture-pinned strings (candidates: `running` → running, `spawn scheduled` → starting, `not running` → failed if the last exit code is nonzero, otherwise stopped); macOS never reports `stopping` unless a fixture shows a distinct state |

Any other combination is a fixed error, never a guessed token. That includes a
partial install, an artifact that is absent while the job is loaded, Foreign,
Unknown, and strings that are not pinned. Raw manager text is never printed.
Status does not report boot intent. A stopped-but-enabled service (decision 1)
reports `stopped` or `failed`, and no seventh token is added.

### 5. Allowlist and bounded execution

- The allowlist is the exact table of (program, args, `when`) tuples for each
  platform, built from the plans below, plus the two query vectors.
- The only path to spawning a process is `apply`, after `allowed`. `Constant`
  no longer runs `launchctl`.
- Both the environment and the output caps are fixed. Mutating vectors get
  35 s and queries 10 s.
- `DROPIN_PATH` and the launchd label and target strings each get one shared
  constant.

### 6. Protected state provisioning (extension of ADR 0009)

`ProvisionState` is an owner operation that ADR 0009 did not list. It is an
install step, placed after `Expect(Installable)` and the private directories
and before any artifact is published, so a provisioning failure leaves nothing
newly registered. It adds no new CLI verb or setting; a retry means rerunning
`install`.

Exact limits:

- **What it creates.** In the verified `0700` root:root state directory only,
  it exclusively creates the missing members of `activity.lock`,
  `activity.sqlite3` and `activity.sqlite3-journal`. Each is zero-length,
  root:root `0600` and nlink 1. It creates no other names.
- **State classes.** Each present member is first checked no-follow: a
  regular file, root:root, `0600`, nlink 1.
  - **Fresh** (no member present): create all three.
  - **Empty partial** (every present member is zero-length): create the
    missing members. This recovers an interrupted provisioning.
  - **Retained** (the database or the journal is nonempty): keep every present
    member unchanged, and create only missing members. This is the state a
    service run leaves behind after `uninstall`, which never removes it. The
    database is accepted only if its first 20 bytes pass the same
    rollback-journal header predicate that the start preflight `state_guard`
    already applies. A nonempty journal is kept for SQLite to roll back at the
    next service start.

  An existing member is never opened for write, truncated, renamed or removed,
  and only the 20-byte header of a nonempty database is read.
- **What it refuses** (`Permissions`, before any file is created):
  - existing `-wal` or `-shm` files, checked *before* the lock is taken; that
    is acceptable because the directory is root-only;
  - a member with the wrong type, owner, mode or link count;
  - a nonempty `activity.lock` (the store opens the lock file but never writes
    to it);
  - a database header that fails the predicate.
- **Locking.** It holds the `activity.lock` flock while it checks and creates
  the database and journal. Concurrent provisioning or a running store
  therefore refuses.
- **Durability.** It fsyncs each created file and then the directory. This
  makes only the **existence of empty files** durable.
- **No SQLite work.** It never opens SQLite and never writes a header, schema,
  journal mode or host identity. `Activity::open_service` initializes those
  transactionally on the first service start (TRUNCATE journal, FULL sync).
  If a crash happens before that, the empty triplet remains, and provisioning
  accepts it again. For Retained state, the start preflight header check and
  the store's own schema and host-identity validation decide at start whether
  the store is usable. Install
  never judges or repairs the contents.
- **Uninstall.** Uninstall never removes these files.

### 7. Install precondition, reinstall and retry

Install is a **converging operation for a registration that is not active**. It
is not a takeover, an upgrade or a restart, and it does not converge from
arbitrary states. The read-only `Expect(Installable)` runs after `G` and before
any mutation, including the private directories and `ProvisionState`:

| Artifacts | Query | Outcome |
| --- | --- | --- |
| any present artifact not identical | any | `Foreign`, no change |
| absent or identical | Foreign | `Foreign`, no change |
| absent or identical | Unknown | `Manager`, no change |
| absent or identical | Active | `Active`, no change: runtime and boot intent stay as they are |
| absent or identical | Absent, or Linux Inactive | proceed |

On macOS, Absent depends on the pinned exit status (decision 3). Until that
constant exists, every macOS install halts here with `Manager`.

When install proceeds, every step accepts its own completed result. It never
touches anything outside its fixed names:

- `EnsureDirectory` verifies existing private directories and creates missing
  ones.
- `ProvisionState` applies the state classes in decision 6.
- `Constant` is identical-only. On Linux, the standard-rights drop-in is
  published only while the unit is absent (`UnitAbsent`), and always before
  the unit. A fresh install therefore never has the unit without the drop-in.
  Once the unit exists, `configure` alone owns the drop-in, so reinstalling a
  host configured with `--rights admin` does not add the drop-in back.
- Boot intent is then asserted off. On Linux, `daemon-reload` then `disable`
  run, which also removes a stale `multi-user.target.wants` link left from an
  earlier enable. On macOS, `disable` runs before the plist is published, which
  clears a stale enable override.
- A final `Expect(Unloaded)` proves the postcondition. If it fails, install
  halts with `Manager` after boot intent is already off.

Consequences:

- **Ordinary reinstall** means rerunning install on a complete registration
  that is not active. It changes no file, keeps config, state, accounts,
  credentials, the workspace and both drop-in choices, and leaves the service
  stopped and off at boot. If the last explicit command was a `start` and the
  service is still active, install refuses with `Active` and changes nothing.
  If a failed `start`, or a unit that ended `failed`, left the service stopped
  but enabled, install turns boot off. That is the only boot change an install
  can make on an existing registration.
- **Idempotent retry.** After any install failure, only the steps before the
  failure are done, and rerunning install resumes from them. The matrix lists
  each failure point. Refusals do not resolve themselves:
  - `Foreign`: the owner removes the foreign registration;
  - `Active`: run `service stop`, or `service uninstall` when stop reports
    `service-not-installed`;
  - `Manager` from Unknown: wait for native evidence, or fix the manager;
  - unsafe directories or state (`Permissions`, `Layout`): the owner fixes
    them by hand.
- **Fresh install versus reinstall.** Fresh and empty-partial state is
  provisioned. Retained state, left after `uninstall` or present on a stopped
  installed host, is kept as is (decision 6). A store that is actually in use
  refuses on the lock, and an active one already refused as `Active`.
- **Upgrade with changed artifacts.** If a newer binary embeds different unit,
  drop-in or plist bytes, install refuses with `Foreign` against the old
  artifact. Before replacing the installed binary, the owner runs `uninstall`
  with that old binary, whose embedded artifact bytes still match. The new
  binary then runs `install`, `configure` if the drop-in choice must change,
  and `start`. Config and state are retained
  across that sequence.
- **Known window (model only).** On Linux, `disable` needs the unit file, so it
  runs after publication. If a stale enable link exists, and the host crashes
  and reboots between `Constant(unit)` and `disable`, the service can start
  once. A rerun then refuses with `Active`, and `service stop` followed by
  `install` settles it. macOS has no such window, because `disable` precedes
  the plist.

## Plans (Unix; `G` = the existing optional guards)

- **Linux install:** `G`, Expect(Installable), EnsureDirectory(private),
  ProvisionState, Constant(drop-in)[UnitAbsent], Constant(unit)[Always],
  `daemon-reload`[Always], `disable`[Always], Expect(Unloaded).
- **Linux start:** `G`, StartPreflight, Expect(Installed), `enable`[Always],
  `start`[Always].
- **Linux stop:** `G`, Expect(Installed), `disable`[Always], `stop`[Always],
  Expect(Unloaded).
- **Linux uninstall:** `G`, Expect(NotForeign), `disable`[ArtifactPresent],
  `stop`[Loaded], Expect(Unloaded), RemoveConstant(unit),
  RemoveConstant(drop-in), `daemon-reload`[Always].
- **macOS install:** `G`, Expect(Installable), EnsureDirectory(private),
  ProvisionState, `disable`[Always], Constant(plist)[Always],
  Expect(Unloaded). `disable` is `Always` because `Expect(Installable)`
  already proved Absent. If the job became loaded in between, `disable` still
  turns boot off, and the final `Expect(Unloaded)` halts with `Manager`.
- **macOS start:** `G`, StartPreflight, Expect(Installed), `enable`[Always],
  `bootstrap`[NotLoaded].
- **macOS stop:** `G`, Expect(Installed), `disable`[Always], `bootout`[Loaded],
  Expect(Unloaded).
- **macOS uninstall:** `G`, Expect(NotForeign), `disable`[Always],
  `bootout`[Loaded], Expect(Unloaded), RemoveConstant(plist).

## State-transition matrix (expected; every row is a required test)

| From → action | Linux vectors after checks | macOS vectors after checks | Result | Starts at next boot |
| --- | --- | --- | --- | --- |
| not installed → install | drop-in, unit, `daemon-reload`, `disable` | `disable`, plist | stopped | no |
| install → start | `enable`, `start` | `enable`, `bootstrap` | running | yes |
| running → stop | `disable`, `stop` | `disable`, `bootout` | stopped | no |
| stopped → start (directory gone) | `enable`, `start`; systemd recreates `/run/arxcovo` | `enable`, `bootstrap`; the service creates the directory | running | yes |
| install → uninstall | `disable`, `stop` (no-op), `daemon-reload` | `disable`; `bootout` skipped (Absent) | not installed | no |
| stop → uninstall | as above | as above | not installed | no |
| running → uninstall | `disable`, `stop`, removals, `daemon-reload` | `disable`, `bootout`, removal | not installed | no |
| running → start | `enable`, `start` (no-op) | `enable`; `bootstrap` skipped | running | yes |
| stopped → stop | `disable`, `stop` (no-op) | `disable`; `bootout` skipped | stopped | no |
| not installed → start or stop | none (`NotInstalled`) | none (`NotInstalled`) | unchanged | unchanged |
| not installed → uninstall | `daemon-reload` | `disable` | not installed | no |
| foreign unit or job → any action | none (`Foreign`) | none (`Foreign`) | unchanged | unchanged |
| Unknown query, or a mutating vector that is nonzero or times out | halt at that step | halt at that step | partial, artifacts kept | as before the step |
| existing unsafe endpoint directory → start | none | none | refused | unchanged |

Install, reinstall and retry rows (decision 7). In every row, install records
no `enable`, `start`, `stop`, `bootstrap` or `bootout`. "Unchanged" for a file
means the same bytes and inode.

| From → install | Linux vectors after checks | macOS vectors after checks | Result | Starts at next boot |
| --- | --- | --- | --- | --- |
| complete, running (Active) | none (`Active`) | none (`Active`) | unchanged, running | unchanged (yes) |
| complete, stopped, disabled (ordinary reinstall) | drop-in skipped, unit unchanged, `daemon-reload`, `disable` | `disable`, plist unchanged | stopped | no |
| complete, stopped but enabled (failed `start`, or unit `failed`) | as above | as above | stopped | no |
| no artifact, stale enable link or override | drop-in, unit, `daemon-reload`, `disable` (removes the link) | `disable` (clears the override), plist | stopped | no |
| primary artifact absent, job still Active | none (`Active`) | none (`Active`) | unchanged | unchanged |
| drop-in only (crash before the unit) | drop-in unchanged, unit, `daemon-reload`, `disable` | — | stopped | no |
| unit published, crash before `daemon-reload` or `disable` | drop-in skipped, unit unchanged, `daemon-reload`, `disable` | — | stopped | no |
| unit present, drop-in absent (`--rights admin`) | drop-in skipped and still absent, unit unchanged, `daemon-reload`, `disable` | — | stopped | no |
| directories or state only (crash before publication) | drop-in, unit, `daemon-reload`, `disable` | `disable`, plist | stopped | no |
| Retained state after `uninstall` | as for not installed; state unchanged | as for not installed; state unchanged | stopped | no |
| empty partial state | missing members created, then as for not installed | same | stopped | no |
| unsafe state (`-wal`/`-shm`, wrong owner, mode or links, nonempty lock, bad header, lock held) | none after the query (`Permissions`) | none after the query (`Permissions`) | unchanged, no artifact published | unchanged |
| non-identical unit, drop-in or plist | none (`Foreign`) | none (`Foreign`) | unchanged | unchanged |
| `daemon-reload` or `disable` nonzero or timed out | halt with `Manager` | `disable`: halt with `Manager` before the plist | artifacts so far kept; a rerun converges | as before the step |
| final `Expect(Unloaded)` false | halt with `Manager` | halt with `Manager` | artifacts kept | no |

Failure rows for start and stop (decision 1):

| From → action, failure | Linux | macOS | Result | Starts at next boot |
| --- | --- | --- | --- | --- |
| stopped → start, `enable` fails | `enable` (`Manager`) | `enable` (`Manager`) | stopped | as before |
| stopped → start, `start`/`bootstrap` fails or times out | `enable`, `start` (`Manager`) | `enable`, `bootstrap` (`Manager`) | stopped but enabled | yes |
| that state → start, stop or install | as in the rows above | as in the rows above | running, stopped, stopped | yes, no, no |
| running → stop, `disable` fails | `disable` (`Manager`) | `disable` (`Manager`) | running | as before (yes) |
| running → stop, `stop`/`bootout` fails or times out | `disable`, `stop` (`Manager`); no post-check | `disable`, `bootout` (`Manager`) | possibly running | no |
| that state → install | none (`Active`) while Active | none (`Active`) | unchanged | no |

macOS rows that need Absent use a scripted typed query result. They prove the
plan logic only. The production parser stays Unknown until the exit status is
pinned (decision 3).

## Sol implementation contract (one PR on the PR29 branch)

Change `owner_plan.rs`, `owner_unix.rs`, `layout.rs` (three error variants:
`NotInstalled`, `Foreign`, `Active`) and
`native.rs`, plus the macOS application `layout` hook. Then update the
foundation document. Add no CLI, settings, public fixture selector or Windows
code.

1. Plans and operations exactly as above. Remove the endpoint
   `EnsureDirectory` from install and from `allowed`. Remove the hidden
   `launchctl disable` inside the macOS `Constant` arm and the hidden `enable`
   inside the `bootstrap` arm; both become plan steps. Change `provision` to
   the state classes in decision 6, reusing the existing
   `rollback_database_header` predicate.
2. Add a `#[cfg(test)]`-only runner seam. It returns
   `Exited(code, stdout) | Signalled | TimedOut | SpawnFailed`, and the manager
   is constructible only in tests with a fixture `Fs` root, a fixture uid/gid,
   a test layout and a scripted runner. Split StartPreflight so that its layout
   part uses `Fs` with the endpoint optional, and its config and state part has
   a test hook. No production selector.
3. Typed query parsers, status mapping and macOS `ensure_endpoint_directory`,
   as specified.

Required tests (ordinary user, hosted Linux and macOS; portable parts on
Windows):

- **Every matrix row** with scripted query results, asserting the exact
  recorded vector order, the skipped steps and the fixture files.
- **Boot persistence.** A stateful fake model of each manager tracks enabled or
  override state, loaded or active state, and plist presence. Reboot is
  modelled as active := enabled. Assert:
  - install→reboot = stopped;
  - install→start→reboot = running;
  - start→stop→reboot = stopped;
  - start→install refuses, and reboot = running;
  - stopped-but-enabled→install→reboot = stopped;
  - stale enable with no artifact→install→reboot = stopped;
  - a crash at each install step, then install again, then reboot = stopped.

  The model also covers the Linux known window in decision 7: a stale link,
  a crash after the unit and a reboot give active, and install then refuses
  with `Active`. Label all of this as a model.
- **Order.** Assert `enable` before `start`/`bootstrap` and `disable` before
  `stop`/`bootout`. The macOS install `disable` must be recorded while the
  plist does not yet exist. The Linux drop-in must be published before the
  unit, and the install `disable` comes after `daemon-reload`.
  `Expect(Installable)` must run before any `EnsureDirectory`,
  `ProvisionState` or `Constant`. Install never records `enable`, `start`,
  `stop`, `bootstrap` or `bootout`.
- **Allowlist.** Every plan command passes. These are rejected:
  `["enable","--now","arxd.service"]`, `["enable","other.service"]`,
  `["disable","system/other"]`, `bootstrap` with `Always`, an
  `EnsureDirectory{EndpointDirectory}`, a drop-in `Constant` with `UnitAbsent`
  outside the Linux install plan, and any query vector other than the two
  fixed ones.
- **Reinstall preserves user objects.** Ordinary reinstall and install over
  Retained state keep config, every state member, the private directories,
  the unit, drop-in or plist, and an absent admin-mode drop-in, all with the
  same bytes and inodes.
- **Unknown manager failure and halting.** Cover an unparseable, oversize,
  duplicate-key or unpinned nonzero query; a foreign `FragmentPath`, drop-in or
  `path`; a mutating nonzero result (`bootout` exits 1, `disable` exits 1); and
  `TimedOut` on `stop`. Each must produce the fixed error and leave no later
  vector, an unchanged artifact, and an unevaluated post-check. Also cover a
  post-stop query that still reports `active`, which must fail.
- **Missing parent.** The start preflight accepts a missing endpoint directory.
  An existing directory with mode `0775`, a symlink, or a file in its place
  refuses before any vector. macOS `ensure_endpoint_directory` with a fixture
  policy must:
  - create a missing directory as `0755` under umask 077;
  - accept an existing correct directory with the same inode;
  - refuse wrong-mode, symlink, file and missing-parent cases unchanged;
  - never remove anything.

  The Linux service path never creates the directory.
- **Uninstall preserves user objects.** Config, the state triplet, private
  directories and the endpoint directory keep the same bytes and inodes.
- **Provisioning.** ProvisionState precedes every Constant. An injected
  provisioning failure publishes no artifact. In ordinary-user fixtures, test
  each state class in decision 6:
  - Fresh and Empty partial create only the missing members;
  - Retained keeps a nonempty database with a valid rollback header, and a
    nonempty journal, with the same bytes and inode;
  - `-wal`, `-shm`, a nonempty lock, a WAL-mode or garbage header, mode
    `0644`, a symlink, nlink 2 and a held lock each refuse with no file
    created.

  The existing `empty_store_prerequisites_are_private_and_never_replace_state`
  assertion that a nonempty database refuses is replaced by the Retained
  assertion. Its byte-preservation check is kept.
- **Native bounded run.** A test-only call of the inner spawn helper runs
  `/bin/sleep 5` with a 100 ms limit. It must return `TimedOut` within 2 s
  with the child reaped. Output above the cap must give `Unknown`.
- **Mutation evidence.** Each of these mutations must fail a test:
  - remove the `enable` step;
  - swap the stop order;
  - treat an unpinned nonzero exit as Absent;
  - remove the install `disable`;
  - accept Active in `Expect(Installable)`;
  - make the drop-in `Constant` ungated;
  - make provisioning refuse, truncate or rewrite a Retained database.

## Remaining runtime evidence (all **unverified on real hardware**)

- **`launchctl print` exit status.** The exit status for an absent
  `system/<label>`, captured as root, pins the Absent constant. One option is a
  read-only query of a deliberately absent label on a disposable hosted
  runner, but only if the coordinator authorizes it. Until that happens, every
  macOS install, the uninstall of an unloaded job, and the start or status of
  an installed unloaded job halt on Unknown. This document does not clear the
  PR29 unloaded-job blocker.
- **`launchctl print` output shape** for loaded running and not-running jobs:
  the top-level `path`, `state` and last exit code. A read-only capture of a
  stock running system daemon on a hosted runner can pin it.
- **Other launchd behaviour:**
  - `disable` succeeds for an unloaded label;
  - the `enable` override takes precedence over `Disabled=true` at boot;
  - `bootout` waits for the exit;
  - `/private/var/run` is cleared at boot;
  - a plist published in `/Library/LaunchDaemons` is not loaded before the
    next boot or an explicit `bootstrap`;
  - `disable` clears an enable override left for a label with no plist.
- **systemd behaviour:**
  - the `show` fields for not-found and loaded units, and the `ActiveState`
    values seen during a `Restart=on-failure` delay;
  - `disable` and `stop` succeed on a loaded inactive unit, and `disable`
    succeeds on a never-enabled unit;
  - `disable` removes a `multi-user.target.wants` link that dangled before the
    unit was published;
  - `daemon-reload` does not start a unit that has a stale enable link;
  - enable symlinks start the service at boot;
  - `RuntimeDirectory` is created and removed across stop, restart and reboot.
- **macOS stock ancestor ownership and mode,** checked against the strict rule.
  This is a separate decision.
- **Known open items:**
  - A macOS crash can leave a stale socket, and bind refuses it because ADR
    0009 never reclaims endpoints.
  - launchd has no equivalent of `RestartPreventExitStatus`, so a config error
    restarts every 10 s.
- **Protected SQLite first start as root.**
- **Windows start-type alignment** (decision 1).
