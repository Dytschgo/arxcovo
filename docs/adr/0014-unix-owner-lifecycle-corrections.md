# 0014: Unix owner lifecycle corrections (boot intent, runtime directory, absent jobs, status)

Status: settled contract (Opus 5.5 high, architecture), 2026-11-11. It corrects
the PR29 review (comment 6798330477) at base `a8c98347`. It amends ADR 0009 only
where stated below. This ADR is a documentation contract, not an
implementation. Every manager behaviour named here is **unverified on real
hardware** until native evidence is recorded. ADR numbers 0012 (Windows owner
recovery) and 0013 (Windows ancestor policy) are reserved for those Windows
topics.

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

There is one ordinary rule for every OS. `install` registers the service but
never starts it, and the service does not start at boot. `start` means running
now and at every boot. `stop` means stopped now and not started at boot. In both
directions the persistence change runs before the runtime change, so a failed
runtime step never leaves the opposite boot intent behind. No setting, prompt
or approval exists for this.

| | install | start | stop |
| --- | --- | --- | --- |
| Linux | unit + drop-in, `daemon-reload`, never `enable` | `enable`, then `start` | `disable`, then `stop` |
| macOS | `disable` (only if not loaded), then plist with `Disabled=true` | `enable`, then `bootstrap` (only if not loaded) | `disable`, then `bootout` (only if loaded) |

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

- **Absent:** `LoadState=not-found` and an empty `FragmentPath`.
- **Ours:** `loaded`, with `FragmentPath=/etc/systemd/system/arxd.service` and
  `DropInPaths` either empty or exactly the fixed drop-in path.
- **Foreign:** any other path, a masked unit or another drop-in.
- **Unknown:** anything else, including a nonzero exit, invalid UTF-8,
  oversize output, a missing or duplicate key, or an unrecognised `LoadState`.

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
the drop-in.

New plan operations:

- `Expect(NotForeign | Installed | Unloaded)`. `NotForeign` means the artifacts
  are absent or identical and the query is not Foreign or Unknown. `Installed`
  means the artifact is identical and present; Linux also requires the query to
  report Ours. `Unloaded` means Linux `ActiveState` is `inactive` or `failed`
  (or `not-found`), and macOS reports Absent.
- `Command{program, args, when}`, where `when` is one of `Always`, `Loaded`,
  `NotLoaded` or `ArtifactPresent`. A gated command re-queries immediately
  before it runs. If the gate is false, the step is skipped and recorded as
  skipped. Foreign or Unknown halts.
- `StartPreflight` replaces the hidden preflight inside `Command`.
- `ProvisionState`. See decision 6.

New fixed errors: `NotInstalled` (`service-not-installed`) and `Foreign`
(`service-foreign-registration`).

Halt rules:

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
install step, placed after the private directories and before any artifact is
published, so a provisioning failure leaves nothing registered. It adds no new
CLI verb or setting; a retry means rerunning `install`.

Exact limits:

- **What it creates.** In the verified `0700` root:root state directory only,
  it exclusively creates the missing members of `activity.lock`,
  `activity.sqlite3` and `activity.sqlite3-journal`. Each is zero-length,
  root:root `0600` and nlink 1. It creates no other names.
- **What it refuses.** It refuses existing `-wal` or `-shm` files. Those are
  checked *before* the lock is taken, not under it; that is acceptable because
  the directory is root-only. It also refuses any existing member of the three
  that is nonempty or has the wrong owner, mode or link count.
- **Locking.** It holds the `activity.lock` flock while creating the database
  and journal. Concurrent provisioning or a running store therefore refuses.
- **Durability.** It fsyncs each created file and then the directory. This
  makes only the **existence of empty files** durable.
- **No SQLite work.** It never opens SQLite and never writes a header, schema,
  journal mode or host identity. `Activity::open_service` initializes those
  transactionally on the first service start (TRUNCATE journal, FULL sync).
  If a crash happens before that, the empty triplet remains, and provisioning
  accepts it again.
- **Uninstall.** Uninstall never removes these files.

## Plans (Unix; `G` = the existing optional guards)

- **Linux install:** `G`, EnsureDirectory(private), ProvisionState,
  Expect(NotForeign), Constant(unit), Constant(drop-in), `daemon-reload`[Always].
- **Linux start:** `G`, StartPreflight, Expect(Installed), `enable`[Always],
  `start`[Always].
- **Linux stop:** `G`, Expect(Installed), `disable`[Always], `stop`[Always],
  Expect(Unloaded).
- **Linux uninstall:** `G`, Expect(NotForeign), `disable`[ArtifactPresent],
  `stop`[Loaded], Expect(Unloaded), RemoveConstant(unit),
  RemoveConstant(drop-in), `daemon-reload`[Always].
- **macOS install:** `G`, EnsureDirectory(private), ProvisionState,
  Expect(NotForeign), `disable`[NotLoaded], Constant(plist).
- **macOS start:** `G`, StartPreflight, Expect(Installed), `enable`[Always],
  `bootstrap`[NotLoaded].
- **macOS stop:** `G`, Expect(Installed), `disable`[Always], `bootout`[Loaded],
  Expect(Unloaded).
- **macOS uninstall:** `G`, Expect(NotForeign), `disable`[Always],
  `bootout`[Loaded], Expect(Unloaded), RemoveConstant(plist).

## State-transition matrix (expected; every row is a required test)

| From → action | Linux vectors after checks | macOS vectors after checks | Result | Starts at next boot |
| --- | --- | --- | --- | --- |
| not installed → install | `daemon-reload` | `disable` | stopped | no |
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

## Sol implementation contract (one PR on the PR29 branch)

Change `owner_plan.rs`, `owner_unix.rs`, `layout.rs` (two error variants) and
`native.rs`, plus the macOS application `layout` hook. Then update the
foundation document. Add no CLI, settings, public fixture selector or Windows
code.

1. Plans and operations exactly as above. Remove the endpoint
   `EnsureDirectory` from install and from `allowed`.
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
  modelled as active := enabled. Assert
  install→reboot = stopped, install→start→reboot = running and
  start→stop→reboot = stopped. Label this as a model.
- **Order.** Assert `enable` before `start`/`bootstrap` and `disable` before
  `stop`/`bootout`. The macOS install `disable` must be recorded while the
  plist does not yet exist. Install never records `enable`, `start` or
  `bootstrap`.
- **Allowlist.** Every plan command passes. These are rejected:
  `["enable","--now","arxd.service"]`, `["enable","other.service"]`,
  `["disable","system/other"]`, `bootstrap` with `Always`, an
  `EnsureDirectory{EndpointDirectory}`, and any query vector other than the two
  fixed ones.
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
  provisioning failure publishes no artifact.
- **Native bounded run.** A test-only call of the inner spawn helper runs
  `/bin/sleep 5` with a 100 ms limit. It must return `TimedOut` within 2 s
  with the child reaped. Output above the cap must give `Unknown`.
- **Mutation evidence.** Remove the `enable` step, swap the stop order, or
  treat an unpinned nonzero exit as Absent. Each mutation must fail a test.

## Remaining runtime evidence (all **unverified on real hardware**)

- **`launchctl print` exit status.** The exit status for an absent
  `system/<label>`, captured as root, pins the Absent constant. One option is a
  read-only query of a deliberately absent label on a disposable hosted
  runner, but only if the coordinator authorizes it. Until that happens, macOS
  install→uninstall halts on Unknown.
- **`launchctl print` output shape** for loaded running and not-running jobs:
  the top-level `path`, `state` and last exit code. A read-only capture of a
  stock running system daemon on a hosted runner can pin it.
- **Other launchd behaviour:**
  - `disable` succeeds for an unloaded label;
  - the `enable` override takes precedence over `Disabled=true` at boot;
  - `bootout` waits for the exit;
  - `/private/var/run` is cleared at boot.
- **systemd behaviour:**
  - the `show` fields for not-found and loaded units;
  - `disable` and `stop` succeed on a loaded inactive unit;
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
