# 0020: Linux execution hierarchy, janitor, startup discovery and rights checks

Status: **proposed** (Anthropic / claude-opus-5-5 / high, design sub-agent,
2026-11-12). Not accepted. A fresh independent Sol high review must pass before
acceptance, and implementation additionally waits for ADR 0019 stages C (PR53)
and D (PR55) to merge through all required gates.

Runtime base `fdea1c6` (PR55 Stage D on the **unmerged** PR53 Stage C, on main
after #52 `1604db1`). Stage C and Stage D stay unmerged by this document. This
ADR grants no production constructor, native execution, cgroup or account
operation, owner step, experiment or retry. Production stays
`MechanismUnavailable` on every OS. Native, installed and dedicated M2 paths
remain **unverified on real hardware**, and M2 remains incomplete. The Stage F
`E_PROTECTION` diagnosis stays separate and Stage G activation keeps every gate.

The implementation brief is
[0020-linux-hierarchy-implementation.md](../briefs/0020-linux-hierarchy-implementation.md).
Design evidence, sources and the disposition of the ADR 0019 open items are in
[0020-design-evidence.md](../reviews/0020-design-evidence.md). Nothing here was
built, run or tested. Statements about the kernel or systemd are either quoted
from the primary documents listed there or labelled *source inference*.

## Scope

In scope, exactly ADR 0019 brief §10 stage E: Linux hierarchy setup (the unit
and `runs/` controllers, creating `runs/`), the janitor, startup discovery
wiring, and the ADR 0009 slice 9 rights checks for Linux standard (with pure
classification for the other cells).

Out of scope: any runner (`exec/`) behaviour change, any ADR 0017 `Report`,
`Creation`, row or output-release change, the macOS sweep, Linux admin
execution, Windows, the owner CLI, packaging changes, the Stage F experiment,
Stage G activation and `PROGRESS.md`.

## Context (from source at `fdea1c6`)

1. **The runner needs a prepared parent and never prepares it.**
   `Group::create` (`exec/cgroup.rs:34-115`) requires `cgroup_parent` to be a
   root-owned, not group/other-writable cgroup2 directory (`:52`) whose
   `cgroup.subtree_control` lists `memory` and `pids` and whose `cgroup.procs`
   is empty (`:57-66`). Its module comment forbids enabling controllers
   (`:1-2`). Nothing in the tree creates `runs/` or enables controllers.
2. **systemd does not enable delegated controllers.** The unit sets
   `Delegate=memory pids` and `DelegateSubgroup=supervisor`
   (`packaging/linux/arxd.service`). systemd's delegation document says it
   "will make the requested controllers available … but won't actually enable
   them"; the delegatee writes `cgroup.subtree_control` itself. ADR 0019
   (Relation to ADR 0009) already found that `runs/` needs them too.
3. **Leftover leaves are invisible to the runner's precondition.** A leaf
   holding live processes leaves `runs/cgroup.procs` empty, because processes
   live in leaves, not in `runs/`. `Group::create` would therefore start new
   runs next to a leftover populated leaf. Only a janitor that runs before
   execution becomes available closes this.
4. **Orphan empty leaves exist.** `Group::create` ignores a failed `unlinkat`
   after `mkdirat` succeeded (`cgroup.rs:84-89`, `:108-113`), and `Group`'s
   `Drop` discards removal failure (`:147-153`). Leaf names are
   `arx-<pid>-<serial>` (`:67-73`), so a later instance that reuses a PID can
   collide with a leftover name.
5. **Readiness is a constant today.** `UnixBridge` holds `ready: bool`
   (`service/unix_bridge.rs:42`), checked last in `preflight` (`:105-112`).
   Its `cgroup_parent` parser (`:178-198`) is pure and has no caller that
   reads `/proc/self/cgroup`.
6. **Startup discovery is a no-op.** `ServiceApplication::start_discovery`
   does nothing (`service/application.rs:225-227`). `Host::run` calls it after
   reporting running (`service/mod.rs:147`). There is no daemon
   `discover_tools` row helper; the schema already allows one (daemon actor,
   principal `arxd`; `store.rs:859-861`). `list_hosts` prints constant `tools`,
   `tools_observed_at` and `last_observed_identity` (`mcp.rs:255-257`).
7. **Rights checks are partial.** Static admin-group membership is checked in
   `execution_preflight` (`service/config.rs:337`, groups at
   `native_unix.rs:268`). The runner refuses supplementary groups and sets
   no-new-privileges and clears capabilities per run (`exec/unix.rs:67-157`,
   pre_exec). `service_hardening` is a constant `null`
   (`service/executor.rs:289`). No `sudo -n true` observation exists; ADR 0019
   brief §5 left it to ADR 0009 slice 9.
8. **The bounding set has no `CAP_KILL`, `CAP_DAC_OVERRIDE` or
   `CAP_SYS_ADMIN`** (`50-arxcovo-standard-rights.conf`). By kill(2), signals
   to build-UID processes therefore fail for the service. `cgroup.kill` is a
   file write and is the only kill primitive available (*source inference*
   for the absence of extra capability checks on cgroupfs writes).

## Decision

### D1. Placement and inertness

- A new Linux-only module `exec/hierarchy.rs` holds the setup and janitor
  state machine over a `CgroupFs` port, plus its descriptor-relative Linux
  implementation. It reuses the exec helpers `walk`, `control`, `put` and
  `trusted_root`. Pure parsers and classifiers compile on every OS.
- A bridge-side `Readiness` replaces `ready: bool`. A wiring seam drives setup
  and startup discovery.
- Everything is reachable only from `#[cfg(test)]` constructors and a
  `#[cfg(test)]` `Hooks` implementation. `Execution::configured` keeps
  `Native`, `Hooks::Native::start_discovery` stays a no-op, and
  `capability()` stays `available: false`. Source-pinning tests (D10) fail if
  any of these changes.
- `exec/cgroup.rs`, `exec/unix.rs`, `RunError` and `NotStarted` are unchanged.

### D2. Trust anchors (all checked before any write)

| # | Anchor | Check | On failure |
| --- | --- | --- | --- |
| T1 | service identity | `getuid() == 0 && geteuid() == 0` | `Identity` |
| T2 | process cgroup | open `/proc/self/cgroup` `O_RDONLY\|O_CLOEXEC\|O_NOFOLLOW`; `fstatfs` magic `0x9fa0` (procfs); read at most 4,097 bytes, refuse > 4,096; one shared pure `locate` parser | `Locate` |
| T3 | unit identity | `locate` keeps every Stage D parser rule (exactly one `0::` line, no v1 lines, no `.`/`..`/empty/NUL/CR, last component `supervisor`, not root-only) and additionally requires the component immediately before `supervisor` to be exactly `arxd.service` (any slice depth, including none) | `Locate` |
| T4 | cgroup2 root | `trusted_root("/sys/fs/cgroup")` (every ancestor root-owned, no group/other write, no-follow); `fstatfs` magic `0x63677270` | `Root` |
| T5 | unit directory | descriptor-relative `openat(O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)` per component; every component cgroup2, UID 0, no group/other write | `Unit` |
| T6 | delegation marker | `fgetxattr(unit, "user.delegate")` into a 2-byte buffer returns exactly `"1"` | `Delegation` |
| T7 | unit state | `cgroup.type` is exactly `domain`; `cgroup.procs` empty; `cgroup.controllers` tokens include `memory` and `pids` | `UnitState` |
| T8 | own placement | `supervisor/` opens as T5; its `cgroup.procs` (≤ 4,096 bytes) is exactly one canonical decimal line equal to `getpid()` | `Supervisor` |

- T6 is documented by systemd (kernel ≥ 5.6 with systemd ≥ 251; the unit
  already requires systemd ≥ 254). `trusted.delegate` is not an alternative:
  reading `trusted.*` needs `CAP_SYS_ADMIN`, which the bounding set lacks.
- T8 proves that the parsed path is this process's cgroup on this mount, not
  a lookalike. It runs once, before any child exists.
- Setup never resolves accounts (no NSS), never reads `/proc/<pid>` of other
  processes and never signals by PID or UID.

### D3. Hierarchy setup order (normative)

One blocking task runs these steps in order. Each step either passes or
returns one fixed `SetupFailure` (D5). The shutdown token is checked between
steps and inside every wait.

1. **S0 hardening** (D8 R4). Read-only.
2. **S1–S3** T1–T4.
3. **S4** T5–T8.
4. **S5 enable on the unit.** One write of `+memory +pids` to the unit's
   `cgroup.subtree_control` (the kernel applies multi-token writes all or
   nothing), then a re-read that must contain both tokens. Never write a `-`
   token, never name another controller, never touch `supervisor/` or a
   systemd `.control/` subgroup. → `EnableUnit`.
5. **S6 runs directory.** `mkdirat(unit, "runs", 0o700)`. `EEXIST` means
   open the existing one. Open with T5 flags. It must be cgroup2, UID 0, with
   no group/other write; `cgroup.type` is `domain`; `cgroup.procs` is empty.
   → `Runs`.
6. **S7 janitor** (D4).
7. **S8 enable on runs.** One write of `+memory +pids` to
   `runs/cgroup.subtree_control`, then a re-read with both tokens.
   → `EnableRuns`.
8. **S9 postconditions.** At the `runs/` level, `memory.max`,
   `memory.swap.max` and `pids.max` open `O_RDONLY|O_NOFOLLOW`. These files
   exist only because S5 enabled the controllers, and `memory.swap.max`
   proves swap accounting before any run needs it (`cgroup.rs:95`).
   `cgroup.events` is exactly `populated 0`; `cgroup.procs` is empty; there
   are zero child directories. → `Postcondition`.
9. **S10 Ready.** `Readiness` moves `Pending → Ready` with a release store.
   The typed `SetupEvidence` (counts only) is returned to the caller.

Steps are idempotent. A crash at any point is repaired by the next start.

### D4. Janitor

The janitor runs only inside S7, once per process, before `Ready`. At that
point this process has created no leaf, so everything under `runs/` is a
leftover of an earlier instance.

1. **Kill the whole subtree.** Write `1` to `runs/cgroup.kill`. The kernel
   kills every process in `runs/` and all descendants with SIGKILL and handles
   concurrent forks and migrations. The write also proves the kill interface
   exists. Failure → `KillInterface`.
2. **Wait.** Poll `runs/cgroup.events` every 5 ms for at most 5 s. A strict
   parser requires every line to be `<lowercase key> <decimal>`, no duplicate
   key, and exactly one `populated` line with value `0` or `1`. Other
   well-formed keys (such as `frozen`, or keys a newer kernel adds) are
   ignored. Any other shape is `Postcondition`. Deadline expiry →
   `KillTimeout`. Shutdown → `Stopped`.
3. **List.** Read the entries of `runs/` descriptor-relatively (at most 512
   entries, at most 64 child directories). Only `d_type == DT_DIR` entries are
   children; `DT_UNKNOWN` falls back to `fstatat(AT_SYMLINK_NOFOLLOW)`.
   Exceeding a bound → `LeafStructure`.
4. **Check each child.** Its name must match the leaf grammar
   `arx-<pid>-<serial>`: canonical unsigned decimals without leading zeros,
   `pid` ≤ 10 digits and `serial` ≤ 20 digits. It opens with T5 flags and is
   cgroup2, UID 0, with no group/other write. Its own listing (≤ 512 entries)
   has no child directory. Its `cgroup.events` reads `populated 0`. Any
   mismatch → `LeafStructure`. **Unknown names are never removed.**
5. **Remove.** `unlinkat(runs, name, AT_REMOVEDIR)` per child. Failure →
   `LeafRemoval`.
6. **Verify.** Re-list `runs/`: zero child directories, and `cgroup.events`
   reads `populated 0`.

The janitor removes leaves of every PID, including a name that equals this
process's PID. That resolves the D4 context item: after `Ready`, a fresh
`mkdirat` cannot collide with a leftover.

### D5. Readiness, typed outcomes and latches

```rust
pub(crate) enum SetupOutcome { Ready(SetupEvidence), Unavailable(SetupFailure) }
pub(crate) struct SetupEvidence {
    pub leaves_removed: u8, pub runs_created: bool,
    pub unit_written: bool, pub runs_written: bool,
}
pub(crate) enum SetupFailure {
    Hardening, Identity, Locate, Root, Unit, Delegation, UnitState, Supervisor,
    EnableUnit, Runs, KillInterface, KillTimeout, LeafStructure, LeafRemoval,
    EnableRuns, Postcondition, Stopped, Panicked,
}
enum State { Pending, Ready, Unavailable(Cause) }   // one AtomicU8
enum Cause { Setup, Hardening, Janitor, Containment, Rights, Stopped }
```

- All three enums are fixed variants deriving `Debug, Clone, Copy, PartialEq,
  Eq`. No variant carries a string, path, errno, name, PID, UID or GID.
  `SetupEvidence` holds counts and flags only. None of these values reaches a
  response, a row, a log, stderr or a panic message.
- **Transitions are monotone.** `Pending → Ready`, `Pending →
  Unavailable(c)` and `Ready → Unavailable(Containment | Rights)` happen by
  `compare_exchange`. The first writer wins. Nothing returns to `Ready` in the
  same process. Only a restart resets readiness, and the next janitor is what
  makes that reset honest.

| `SetupFailure` | `Cause` | Bridge preflight (`ExecutionUnavailable`) | Stored reason |
| --- | --- | --- | --- |
| `Hardening` | `Hardening` | `mechanism_unavailable` | `preflight_failed` |
| `Identity` … `Runs`, `KillInterface`, `LeafStructure`, `EnableRuns`, `Postcondition` | `Setup` | `mechanism_unavailable` | `preflight_failed` |
| `KillTimeout`, `LeafRemoval`, `Panicked` | `Janitor` | `quarantined` | `quarantined` |
| `Stopped` | `Stopped` | `stopping` | `shutdown` |
| (state `Pending`) | — | `mechanism_unavailable` | `preflight_failed` |
| run latch, D6 | `Containment` | `mechanism_unavailable` | `preflight_failed` |
| discovery latch, D8 | `Rights` | `rights_mismatch` | `preflight_failed` |

- `quarantined` matches the ADR 0009 meaning "an earlier cleanup is
  unconfirmed". `LeafStructure` is not a quarantine: by then `runs/` reported
  `populated 0`, so no process is alive. Only unexpected directories remain.
- The setup body runs under `catch_unwind(AssertUnwindSafe(..))`. The payload
  is dropped unread and the result is `Panicked`, treated as `Janitor`
  because a panic after the kill write leaves the state unknown.
- Bridge preflight order stays as Stage D has it: config check, then stored
  construction result, then readiness. No new `ExecutionUnavailable` variant
  is added.

### D6. Orphan leaves and the containment latch

| Source | Processes? | Bridge sees | Disposition |
| --- | --- | --- | --- |
| `Group::create` openat or setup write fails, then `unlinkat` fails (`cgroup.rs:84-89`, `:108-113`) | never joined | `NotStarted::Containment` → row D | **latch** `Containment`; next-start janitor removes |
| `Group` dropped after `socketpair` failure, removal fails | never joined | `NotStarted::Resource` → row F | **latch** `Containment` |
| `Group` dropped at the final recheck (cancel), removal fails | never joined | `NotStarted::Cancelled` → row E | no latch (agent-triggerable); residual; next-start janitor |
| same, at shutdown | never joined | `NotStarted::Shutdown` | process is stopping; next-start janitor |
| `kill_and_confirm` removal fails after `populated 0` | none left | cleanup `false` → row H | existing quarantine |
| spawn or post-spawn error | possible | rows G | existing quarantine |
| service crash or SIGKILL | possible | — | systemd `KillMode=mixed` SIGKILL; next-start janitor |

- **The latch.** On any `NotStarted::Containment` or `NotStarted::Resource`
  from the runner port, the bridge moves readiness to
  `Unavailable(Containment)` **before** returning the unchanged ADR 0019
  report. Later requests are refused at preflight as `mechanism_unavailable`.
  So at most `max_concurrent_runs` (≤ 4 on Linux) orphans can exist until the
  next start. The report for the triggering run is unchanged: row D or F, no
  quarantine.
- **Why an orphan is inert** (*source inference* for kernfs ownership): it is
  a root-owned `0700` directory with no processes. Only a writer with write
  access to its `cgroup.procs` (root) can populate it. The delegated subtree
  has one writer (systemd's single-writer rule), and the runner never reuses
  a leaf name. `Group::create` does not look at siblings, so an orphan
  neither blocks nor weakens later runs. It counts toward
  `cgroup.max.descendants` (default `max`).
- The cancel residual needs `rmdir` of an empty, never-populated cgroup to
  fail. The kernel documentation allows removing a cgroup with no children and
  no live processes, so no failure mode is expected. It stays an unverified
  residual: a runner-level residue signal would reopen ADR 0019 D1 (U3).

### D7. Startup ordering and discovery wiring

- `Application::preflight` (before ready) stays the existing config preflight.
  Setup failure is never fatal: inventory still starts, and there is no exit
  code, restart loop or `StartLimit` interaction.
- The wiring reads `/proc/self/cgroup` **once** (T2/T3) at execution
  construction. The resulting `Location` gives `runs()` to `UnixBridge::new`
  (unchanged parameter) and the same value to the setup driver. A `Locate`
  failure constructs the bridge with a refused runner and readiness
  `Unavailable(Setup)`.
- `start_discovery` (after `Running`) starts **one tracked background
  sequence**:
  1. setup and janitor (D3/D4) on the blocking pool;
  2. **only if readiness is `Ready`**, a daemon `discover_tools` row through
     a new store helper `start_daemon_discovery()` (`actor_kind = daemon`,
     principal `arxd`, `identity_mode = dedicated_account`; no schema change);
  3. `Execution::dispatch(None, activity, id)`. That is the ordinary path:
     bridge preflight, busy check, ADR 0017 `assess`, quarantine and drain.
- **Janitor before discovery is strict.** It settles the ADR 0019 open item.
  Discovery never runs, and no daemon row is written, unless the janitor has
  confirmed an empty `runs/`. A setup failure writes no row, because no action
  was attempted. A discovery failure follows ADR 0017 rows D–M for the daemon
  row, including quarantine for G–K. It is never retried at startup.
- **Tool cache.** Only an outcome of `succeeded` (row N) updates it, and the
  update replaces the whole value. It holds the response's `tools`, the
  response's `observed_identity` (ADR 0017 rule 5: built from the redacted
  `Metadata`, never from `Evidence`) and `tools_observed_at` (milliseconds).
  It lives in memory only and is never persisted or logged. Agent
  `discover_tools` row N refreshes it too. `list_hosts` reads it. When no
  cache exists, the current constants stay byte-identical.
- **Drain.** `stop_accepting` already sets the execution shutdown token. The
  setup sequence observes it between steps and inside the janitor wait, and
  returns `Stopped` with no discovery. `drain` awaits `Execution::drain`, then
  the sequence's handle, then `Blocking::drain`. The existing
  stopping/JoinSet serialization in `dispatch` covers a dispatch that races
  shutdown. A synchronous cgroupfs call that stalls has no deadline. The 10 s
  drain budget then fails, as ADR 0009 describes (exit 5).

### D8. Rights checks (ADR 0009 slice 9, Linux standard)

| Layer | When | Fence | Exists |
| --- | --- | --- | --- |
| R1 static membership (`sudo`/`wheel`/GID 0) | every preflight | `rights_mismatch` | yes |
| R2 no supplementary groups, account-named primary group | every run, runner | `PreflightFailed` | yes |
| R3 `PR_SET_NO_NEW_PRIVS`, capabilities cleared | every run, pre_exec | spawn error | yes |
| R4 service hardening | setup S0 | `Hardening` → `mechanism_unavailable` | **new** |
| R5 child-side rights line | every tool probe (startup and agent `discover_tools`) | `Rights` latch → `rights_mismatch` | **new** |

**R4.** Read `/proc/self/status` (procfs magic, `O_NOFOLLOW`, at most 8,193
bytes read, refuse > 8,192). The file must have exactly one `NoNewPrivs:`
line with value `1` and exactly one `CapBnd:` line with 16 hex digits equal
to `00000000000000c1` (`CAP_CHOWN | CAP_SETGID | CAP_SETUID`, the constant
drop-in). The result is `service_hardening = standard`; anything else is
`none`. Linux standard requires `standard`. The value is rendered in
`list_hosts` only at Stage G; until then `capability()` keeps `null`.

**R5.** `tool-probe` appends exactly one line after the `TOOLS` lines:
`{"rights":{"no_new_privs":<true|false|null>,"sudo":"absent"|"refused"|"effective"|"unknown"}}`
(≤ 96 bytes; total output stays below 4,096).

- `no_new_privs` is read by the shim, as the build account, from its own
  `/proc/self/status` with R4's bounds. It is `null` on macOS or on any parse
  failure.
- `sudo` is computed from the fixed candidates `/usr/bin/sudo`, `/bin/sudo`
  and `/usr/local/bin/sudo`, in that order. The shim never searches `PATH`, so
  an agent-planted `sudo` cannot hide the real one. A candidate is accepted
  only if `lstat` shows a regular file with UID 0 and no group/other write.
  The first accepted candidate runs `-n true` with stdin, stdout and stderr
  null, cwd `/`, the inherited environment and a 5 s deadline (kill and reap).
  - exit 0 → `effective`
  - other exit → `refused`
  - signal, timeout or spawn error → `unknown`
  - no accepted candidate → `absent`
- The bridge parses the line only as part of a valid table. If it is missing,
  malformed or extra, the table is invalid: no tools, row K, as today. The
  line is never released, persisted or logged. `tools` keep their ADR 0017
  release rule.
- Classification is a pure `classify_rights` over (rights, hardening,
  `no_new_privs`, `sudo`):

| rights | hardening | `no_new_privs` | `sudo` | result |
| --- | --- | --- | --- | --- |
| standard | `standard` | `true` | `absent`/`refused` | ok |
| standard | `standard` | `true` | `effective`/`unknown` | `rights_mismatch` |
| standard | `standard` | `false`/`null` | any | `rights_mismatch` |
| standard | `none` | any | any | `mechanism_unavailable` (R4) |
| admin | `standard` | any | any | `service_reconfiguration_required` |
| admin | `none` | `false` | `effective` | ok (pure table only) |
| admin | `none` | otherwise | otherwise | `rights_mismatch` |

  Linux admin stays refused before this table, by ADR 0019 D6 and
  `Runner::new`. macOS stays `mechanism_unavailable`. The admin rows are
  tested as pure logic only and stay **unverified**.
- A failing R5 result moves readiness to `Unavailable(Rights)` before the
  discovery report returns. The report itself stays as ADR 0017 classifies it.
  Execution then stays unavailable until restart.
- `unknown` fails closed: a hung `sudo` (for example a slow directory
  service) costs availability, never safety.

### D9. Restart, crash and shutdown recovery

- **Crash during a run.** `KillMode=mixed` makes systemd SIGKILL the processes
  remaining in the unit cgroup after the main process exits (systemd.kill).
  `started` rows reconcile to `interrupted` (existing). Whether systemd also
  removes the delegated subdirectories at unit stop is not stated in the
  documents consulted. The janitor handles both outcomes: `runs/` absent
  means create it; `runs/` present means kill, remove, verify.
- **Crash during setup or janitor.** Every step is idempotent. Enabling an
  enabled controller is a no-op, `EEXIST` is handled, and the janitor reruns.
- **Clean stop.** Drain cancels runs, the runner kills and removes its leaves,
  and the setup sequence returns `Stopped` with no discovery.
- **Quarantine** (any source) lasts until restart. A janitor failure at the
  next start quarantines again. There is no agent-reachable reset, and no
  owner command is added. The owner restarts the service with existing
  owner tooling.

### D10. Staging and pins

- Production pins (tests in the brief): `Execution::configured` contains
  `Self::native(` and no `UnixBridge`. `Native::start_discovery` stays empty.
  `capability()` still has `"available":false` and `"service_hardening":null`.
  `list_hosts` output without a cache is unchanged. No production module
  calls `hierarchy::` or `start_daemon_discovery`.
- The P-SCAN panic-payload set grows by `exec/hierarchy.rs` and the readiness
  module. It also requires them to contain no `kill(`, `killpg(` or
  `setpriority(`, so cgroup.kill stays the only kill path.
- Stage G (activation) additionally requires the ADR 0020 implementation
  reviews, the Stage F acceptance evidence for the privileged items in the
  brief, and rendering of readiness and hardening in `capability()`.

## Bounds

| Read or wait | Bound | Over bound |
| --- | --- | --- |
| `/proc/self/cgroup` | 4,096 bytes | `Locate` |
| `/proc/self/status` (service and shim) | 8,192 bytes | `Hardening` / `null` |
| any cgroup control file | 4,096 bytes (`text`, `cgroup.rs:23-32`) | step failure |
| `user.delegate` | 2-byte buffer, exactly 1 byte `1` | `Delegation` |
| entries per directory listing | 512 | `LeafStructure` |
| child directories in `runs/` | 64 | `LeafStructure` |
| janitor kill wait | 5 s, 5 ms poll, shutdown-aware | `KillTimeout` / `Stopped` |
| rights `sudo` probe | 5 s, kill and reap | `unknown` |
| rights line | 96 bytes | invalid table |

Every open is descriptor-relative with `O_NOFOLLOW|O_CLOEXEC` (directories
also `O_DIRECTORY`). Nothing is path-based after the trusted root, and nothing
is canonicalized.

## Consequences

- The service owns one more privileged code path, limited to its own
  delegated subtree. It never touches `supervisor/`, `.control/`, the slice or
  any other unit.
- Execution becomes available only after a confirmed-empty `runs/`, and
  startup discovery only after that. Agents see `mechanism_unavailable` while
  setup is pending (U5).
- A host fault on the containment path latches execution off until restart.
  This replaces repeated row-D refusals with a stable refusal, with no report
  change.
- `sudo -n true` runs as the build account at each discovery. On many
  distributions that writes an authentication log entry.
- A false `rights_mismatch` is possible (a slow `sudo`). A false pass from a
  planted `sudo` is not, because candidates are fixed root-owned paths.

## Relation to other ADRs

- **ADR 0007.** No change. The runner, `cgroup.rs`, its precondition checks and
  its cleanup stay as they are. The janitor adds a second kill site that uses
  the same primitive (`cgroup.kill`).
- **ADR 0009.**
  - *Clarification:* the hierarchy setup sentence is made exact (unit and
    `runs/`, S5/S8).
  - *Clarification:* the janitor runs after ready, in the background, still
    before execution becomes available.
  - *Extension:* Linux standard requires `service_hardening = standard`.
  - *Deviation:* the `sudo -n true` effect is observed at each discovery, not
    before every run. R1–R3 still apply to every run, and R3 is the kernel
    fence that makes a setuid `sudo` ineffective. Running `sudo` per run would
    log per run and add a child per request.
  - The janitor failure maps to `quarantined`, its existing meaning.
- **ADR 0017.** No change to `Report`, `Creation`, rows, quarantine or output
  release. The daemon discovery row uses the same rows. The tool cache follows
  rules 5 and 6 (redacted `Metadata` only; no protected identifier in any
  cache).
- **ADR 0019 amendments** (to its accepted inactive contract; each needs the
  Sol review):
  - A1: one `locate` parser shared by the bridge and setup. It additionally
    requires `arxd.service` as the unit component (T3).
  - A2: the discovery table is `TOOLS.len()` lines plus one rights line
    (brief §4 discovery and S6 output shape). Tools still come only from the
    `TOOLS` lines.
  - A3: the bridge's `ready: bool` becomes `Readiness` with the D6 and D8
    latches.
  - A4: the P-SCAN set and denylist are extended (D10).

## Alternatives rejected

- **Janitor before ready, synchronously.** A stalled cgroupfs call would burn
  the 30 s startup deadline and exit 7, which `RestartPreventExitStatus`
  makes permanent. Inventory would be lost for an execution-only fault.
- **Per-leaf kill instead of `runs/cgroup.kill`.** It is slower and races
  forks across leaves. The recursive kill is documented as fork- and
  migration-safe.
- **Deleting unknown children of `runs/`.** That is unsafe for objects this
  code did not create. Refusal is fail-closed.
- **A new `execution_starting` token.** It would change the response
  vocabulary for a transient state (U5).
- **A runner-level orphan signal.** It would reopen ADR 0019 D1 and the
  Stage B review. The latch covers the observable sources (U3).
- **Resolving `sudo` through the owner `PATH`.** That allows an agent-planted
  false pass.
- **Signals for cleanup.** They are impossible without `CAP_KILL`, and
  UID-scoped signals are not a tree boundary.

## Unresolved decisions for the independent review

- U1: rights cadence (discovery-time, not per run) and the window between
  `Ready` and the first rights line. The alternative is a `HierarchyReady`
  state that admits only the daemon discovery, which needs an actor-aware
  preflight.
- U2: making `user.delegate` mandatory.
- U3: latching on every `Containment`/`Resource` (including a transient
  `ENOMEM`) versus a runner residue signal.
- U4: requiring CapBnd to be exactly equal versus a subset of the three
  capabilities.
- U5: no distinct pending token.
- U6: refusing, rather than removing, unknown or nested children.
- U7: no startup discovery retry.
- U8: `sudo` `unknown` → `rights_mismatch`.
- U9: systemd's handling of delegated children at unit stop (unknown; both
  cases handled).

## Sources and inference

See [0020-design-evidence.md](../reviews/0020-design-evidence.md) for the
file list at `fdea1c6`, the primary documents (kernel cgroup-v2, systemd
delegation, `systemd.resource-control`, `systemd.exec`, `systemd.kill`,
kill(2)) with fetch details, and the full inference list. Hardware, kernel
and systemd behaviour on the owner's machine is **unverified on real
hardware**.
