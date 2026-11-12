# 0020: Linux execution hierarchy, janitor, startup discovery and rights checks

Status: **accepted inactive staged contract**, coordinator OpenAI /
gpt-6.1-sol / medium, 2026-11-12. This records the design disposition only.
The original Opus proposal and correction 1 retain independent Sol reviews;
Sol correction 2 `07475b8` passed the independent Opus check `f08a510`.
[Final recheck](../reviews/0020-correction2-opus-recheck.md) closes P2/P6/P7.
The advisory-only follow-up clarifies existing errno ordering, splits T20
across E6/E7, names non-test Runner construction pins and shares W3e's seam.
This publication delta still requires exact-head independent review and all
required checks. No implementation starts until this contract PR and Stage C
(PR53) / Stage D (PR55) merge through all gates. No privileged operation or
activation is authorized. M2 remains incomplete and native paths remain
**unverified on real hardware**.

## Historical proposal and correction-1 checkpoint

Status: **proposed, correction 1 of 3** (Anthropic / claude-opus-5-5 / high,
design sub-agent, 2026-11-12). Not accepted. The original proposal
(`e3928f5`, retained as `54032bf`) received an independent Sol high review
with **changes required** ([review](../reviews/0020-sol-independent-design-review.md),
`8ff4196`, retained as `8ff3a54`). This revision corrects blockers S1 and
P1–P4 and applies the review's U1–U9, A1–A4, S2 and P5 decisions. The
[correction disposition](../reviews/0020-correction1-disposition.md) maps
every finding to its change. A fresh independent Sol high recheck must pass
before acceptance. Implementation additionally waits for ADR 0019 stages C
(PR53) and D (PR55) to merge through all required gates.

Correction base `d44035d`. Its code is unchanged from the original runtime
base `fdea1c6` (PR55 Stage D on the **unmerged** PR53 Stage C, on main after
#52 `1604db1`); the commits in between are docs only. Stage C and Stage D
stay unmerged by this document. This ADR grants no production constructor,
native execution, cgroup or account operation, owner step, experiment or
retry. Production stays `MechanismUnavailable` on every OS. Native, installed
and dedicated M2 paths remain **unverified on real hardware**, and M2 remains
incomplete. The Stage F `E_PROTECTION` diagnosis stays separate and Stage G
activation keeps every gate.

The implementation brief is
[0020-linux-hierarchy-implementation.md](../briefs/0020-linux-hierarchy-implementation.md).
Design evidence, sources and the disposition of the ADR 0019 open items are in
[0020-design-evidence.md](../reviews/0020-design-evidence.md). Nothing here was
built, run or tested. Statements about the kernel, libc or systemd are either
taken from the primary documents listed there or labelled *source inference*.

## Scope

In scope, exactly ADR 0019 brief §10 stage E: Linux hierarchy setup (the unit
and `runs/` controllers, creating `runs/`), the janitor, startup discovery
wiring, and the ADR 0009 slice 9 rights checks for Linux standard (with pure
classification for the other cells).

This correction also proposes one **narrow ADR 0019 runner-contract delta**
(A5, D6): a payload-free cleanup-residue signal. It changes no `RunError`,
`NotStarted`, `RunResult`, legacy `run` result, ADR 0017 `Report`,
`Creation`, row or output.

Out of scope: any other runner (`exec/`) behaviour change, any ADR 0017
change, the macOS sweep, Linux admin execution, Windows, the owner CLI,
packaging changes, the Stage F experiment, Stage G activation and
`PROGRESS.md`.

## Context (from source at `d44035d`, identical to `fdea1c6`)

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
4. **Leaves can outlive their `Group` without a report.** `Group::create`
   ignores a failed `unlinkat` after `mkdirat` succeeded (`cgroup.rs:84-89`,
   `:108-113`). `Group`'s `Drop` calls `kill_and_confirm` and discards its
   result (`:147-153`); that result is `false` when the kill write, an events
   read, the 2 s wait or the `unlinkat` fails (`:117-145`). A `Group` is
   dropped this way after a `socketpair` failure (`unix.rs:223-224`), at the
   final cancel/shutdown recheck (`:344-352`) and after a spawn error
   (`:353-355`). Leaf names are `arx-<pid>-<serial>` (`:67-73`), so a later
   instance that reuses a PID can collide with a leftover name.
5. **Admission is checked before the concurrency slot.** `dispatch`
   (`service/executor.rs:337-352`) calls `preflight` and only then
   increments `active`. A worker can pass preflight, pause, and take a slot
   later. The slot counter bounds simultaneous runner calls, not the set of
   workers that passed an earlier readiness check.
6. **Readiness is a constant today.** `UnixBridge` holds `ready: bool`
   (`service/unix_bridge.rs:42`), checked last in `preflight` (`:105-112`).
   Its `cgroup_parent` parser (`:178-198`) is pure and has no caller that
   reads `/proc/self/cgroup`.
7. **Startup discovery is a no-op.** `ServiceApplication::start_discovery`
   does nothing (`service/application.rs:225-227`). `Host::run` calls it after
   reporting running (`service/mod.rs:147`). There is no daemon
   `discover_tools` row helper; the schema already allows one (daemon actor,
   principal `arxd`; `store.rs:859-861`). `list_hosts` prints constant `tools`,
   `tools_observed_at` and `last_observed_identity` (`mcp.rs:255-257`).
8. **Rights checks are partial.** Static admin-group membership is checked in
   `execution_preflight` (`service/config.rs:337`, groups at
   `native_unix.rs:268`). The runner refuses supplementary groups and, in
   `pre_exec`, sets no-new-privileges and clears capabilities on every run
   (`exec/unix.rs:242-310`). `service_hardening` is a constant `null`
   (`service/executor.rs:289`). No `sudo -n true` observation exists; ADR 0019
   brief §5 left it to ADR 0009 slice 9.
9. **The bounding set has no `CAP_KILL`, `CAP_DAC_OVERRIDE` or
   `CAP_SYS_ADMIN`** (`50-arxcovo-standard-rights.conf`). By kill(2), signals
   to build-UID processes therefore fail for the service. `cgroup.kill` is a
   file write and is the only kill primitive available (*source inference*
   for the absence of extra capability checks on cgroupfs writes).
10. **The shim executes by path today.** `tool-probe` and `exec-child` spawn
    resolved paths with `Command` (`unix_shim.rs:60-70`, `:210-218`). The
    shim inherits the runner's no-new-privileges, cleared capabilities, leaf
    and owner environment (ADR 0019 D2).

## Decision

### D1. Placement and inertness

- A new Linux-only module tree `exec/hierarchy.rs` with
  `exec/hierarchy/{parse,setup,linux}.rs` holds the setup and janitor state
  machine over a `CgroupFs` port, plus its descriptor-relative Linux
  implementation. It reuses the exec helpers `walk`, `control`, `put` and
  `trusted_root`. Pure parsers and classifiers compile on every OS.
- A bridge-side `Readiness` (`service/unix_bridge/readiness.rs`) replaces
  `ready: bool`. A wiring seam drives setup and startup discovery.
- Everything is reachable only from `#[cfg(test)]` constructors and a
  `#[cfg(test)]` `Hooks` implementation. `Execution::configured` keeps
  `Native`, `Hooks::Native::start_discovery` stays a no-op, and
  `capability()` stays `available: false`. Source-pinning tests (D10) fail if
  any of these changes.
- `RunError` and `NotStarted` are unchanged. A5 permits only private flag
  plumbing through `Runner::run_core -> unix::run -> Group::create`
  (Linux-gated where needed), plus the cgroup cleanup signal (D6).
  `exec/unix.rs` keeps its behavioral order, cancellation checks, fences,
  legacy/phased results and each branch's existing errno-read/cleanup order
  unchanged. In particular, the current failed-openat branch reads errno
  after unlinkat; A5 does not silently correct that existing behavior.

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

- T6 is mandatory (U2). The marker, root ownership, cgroup2 type and exact
  `arxd.service/supervisor` placement are cumulative checks. systemd
  documents the marker (kernel ≥ 5.6 with systemd ≥ 251; the unit already
  requires systemd ≥ 254). There is no fallback to `trusted.delegate`:
  reading `trusted.*` needs `CAP_SYS_ADMIN`, which the bounding set lacks. A
  missing marker disables execution; inventory stays available.
- T8 is **consistency evidence** on the trusted mounted hierarchy: the parsed
  path holds exactly this process on this mount. It is not proof against a
  privileged namespace or mount adversary. No unprivileged build can mount a
  substitute; privileged mounts and privileged writers are a stated trust
  assumption (A1). It runs once, before any child exists.
- Setup never resolves accounts (no NSS), never reads `/proc/<pid>` of other
  processes and never signals by PID or UID.

### D3. Hierarchy setup order (normative)

One blocking task runs these steps in order. Each step either passes or
returns one fixed `SetupFailure` (D5). The shutdown token is checked between
steps and inside every wait, including immediately before S10.

1. **S0 hardening** (D8 R4). Read-only. It runs before T1, so an unhardened
   process reports `Hardening` even when it is also not root.
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
   `cgroup.events` passes the strict `events` parser (D4.2) with populated
   `false`; extra well-formed keys such as `frozen` are allowed.
   `cgroup.procs` is empty. One fresh listing (D4.3) classifies zero child
   directories and no refused entry. → `Postcondition`.
9. **S10 Ready.** If shutdown has not been requested, `Readiness` moves
   `Pending → Ready` by `compare_exchange` (release). Otherwise the result is
   `Stopped`. The typed `SetupEvidence` (counts only) is returned to the
   caller.

Steps are idempotent. A crash at any point is repaired by the next start.

Parent enablement (S5) exposes the controller interface files in `runs/`;
`runs/` enablement (S8) exposes them in leaves. systemd delegation makes the
controllers available without enabling them. The `supervisor/` placement
keeps the unit's internal node empty, so S5 cannot meet resident processes.

### D4. Janitor

The janitor runs only inside S7, once per immutable admission domain, before `Ready`. At that
point this process has created no leaf, so everything under `runs/` is a
leftover of an earlier instance. The trust boundary is the verified,
exclusive `runs/` subtree, not the leaf-name grammar.

1. **Kill the whole subtree.** Write `1` to `runs/cgroup.kill`. The kernel
   kills every process in `runs/` and all descendants with SIGKILL, including
   processes in children with unknown or nested names, and handles concurrent
   forks and migrations. Failure → `KillInterface`. A failed kill write
   proves nothing about live processes; it only proves that this janitor
   cannot confirm cleanup. Execution stays refused until restart, which is
   safe, but no "no live process" claim follows.
2. **Wait.** Poll `runs/cgroup.events` every 5 ms against a 5 s logical
   deadline from the injected clock. The strict `events` parser requires
   every line to be `<key> <decimal>` with a lowercase `[a-z_]` key, no
   duplicate key, and exactly one `populated` line with value `0` or `1`.
   Other well-formed keys (`frozen`, or keys a newer kernel adds) are
   ignored. It returns the populated value. Any other shape is
   `Postcondition`. Deadline expiry → `KillTimeout`. Shutdown → `Stopped`.
3. **List.** Every listing call opens a **fresh open file description** of
   the already-verified directory with
   `openat(dir, ".", O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` and reads it
   from offset 0 to end (`fdopendir`/`readdir` or raw `getdents64` on that
   new descriptor, then close). `dup`, `dup2`, `F_DUPFD` and reuse of a
   previous stream are forbidden for listing: a duplicate shares the
   original's file offset, and `dup` clears close-on-exec. A listing reads
   at most 512 raw records; `.` and `..` count toward that budget and are
   then excluded from classification. Each other record is classified by
   `d_type`, with `DT_UNKNOWN` resolved by
   `fstatat(dir, name, AT_SYMLINK_NOFOLLOW)`:

   | Kind | Classification |
   | --- | --- |
   | directory | child cgroup (at most 64 per listing) |
   | regular file whose name matches `[a-z0-9_]+(\.[a-z0-9_]+)+`, at most 64 bytes | kernel control file; ignored |
   | regular file with any other name | refused |
   | symlink, character, block, FIFO, socket, any other type | refused |
   | `DT_UNKNOWN` whose `fstatat` fails (including `ENOENT`) | refused |

   Exceeding a bound or any refused entry → `LeafStructure`. Nothing is
   removed. (cgroupfs offers no user file or symlink creation; the refusal
   rows are fail-closed classification, not a claim that such entries occur
   on a normal host.)
4. **Check each child.** Its name must match the leaf grammar
   `arx-<pid>-<serial>`: canonical unsigned decimals without leading zeros,
   `pid` ≤ 10 digits and `serial` ≤ 20 digits. It opens with T5 flags and is
   cgroup2, UID 0, with no group/other write. Its own fresh listing (same
   rules and bounds) classifies **zero** child directories and no refused
   entry. Its `cgroup.events` parses with populated `false`. Any mismatch →
   `LeafStructure`. **All children are checked before the first removal, and
   unknown or nested names are never removed** (their processes were killed
   in step 1; only removal is withheld).
5. **Remove.** `unlinkat(runs, name, AT_REMOVEDIR)` per child. Failure →
   `LeafRemoval`.
6. **Verify.** A fresh listing of `runs/` classifies zero child directories
   and no refused entry, and `cgroup.events` parses with populated `false`.
   Any failure, including a child that reappeared → `VerifyFailed`.

Populated `false` and successful removal are confirmed separately; neither
alone proves complete cleanup. The janitor removes leaves of every PID,
including a name that equals this process's PID, so a fresh `mkdirat` after
`Ready` cannot collide with a leftover.

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
    VerifyFailed, EnableRuns, Postcondition, Stopped, Panicked,
}
enum State { Pending, Ready, Unavailable(Cause) }   // one AtomicU8
enum Cause { Setup, Hardening, Janitor, Containment, Rights, Stopped }
```

- All enums are fixed variants deriving `Debug, Clone, Copy, PartialEq, Eq`.
  No variant carries a string, path, errno, name, PID, UID or GID.
  `SetupEvidence` holds counts and flags only. None of these values reaches a
  response, a row, a log, stderr or a panic message.
- **Transitions are monotone.** `Pending → Ready`, `Pending →
  Unavailable(c)` and `Ready → Unavailable(Containment | Rights)` happen by
  `compare_exchange`. The first writer wins. Nothing returns to `Ready` in the
  same process. Only a restart resets readiness, and the next janitor is what
  makes that reset honest. There is no agent-reachable reset.

| `SetupFailure` | `Cause` | Bridge refusal (`ExecutionUnavailable`) | Stored reason |
| --- | --- | --- | --- |
| `Hardening` | `Hardening` | `mechanism_unavailable` | `preflight_failed` |
| `Identity` … `Runs`, `KillInterface`, `LeafStructure`, `EnableRuns`, `Postcondition` | `Setup` | `mechanism_unavailable` | `preflight_failed` |
| `KillTimeout`, `LeafRemoval`, `VerifyFailed`, `Panicked` | `Janitor` | `quarantined` | `quarantined` |
| `Stopped` | `Stopped` | `stopping` | `shutdown` |
| (state `Pending`) | — | `mechanism_unavailable` | `preflight_failed` |
| run latch, D6 | `Containment` | `mechanism_unavailable` | `preflight_failed` |
| discovery latch, D8 | `Rights` | `rights_mismatch` | `preflight_failed` |

- `quarantined` keeps the ADR 0009 meaning "an earlier cleanup is
  unconfirmed". `LeafStructure` is not a quarantine: at that point `runs/`
  reported populated `false`, so no process was alive then; only unexpected
  entries remain. `KillInterface` is a missing or failing mechanism: it maps
  to `mechanism_unavailable` (A3), and D4.1 states what it does not prove.
- Pending maps to the existing `mechanism_unavailable` (U5). Setup failure is
  never fatal to inventory.
- The setup body runs under `catch_unwind(AssertUnwindSafe(..))`. The payload
  is dropped unread and the result is `Panicked`, treated as `Janitor`
  because a panic after the kill write leaves the state unknown. An aborting
  panic is not caught; ADR 0019 G-P2 (unwinding release profile) stays an
  activation gate.
- Bridge preflight order stays as Stage D has it: config check, then stored
  construction result, then readiness. No new `ExecutionUnavailable` variant
  is added. The in-slot gate (D6) repeats the readiness and residue checks.

### D6. Cleanup residue, admission and the containment latch

**Residue model.** A *residue* is a leaf this process created that may still
exist after its `Group` was released without a confirmed `unlinkat`. Every
residue is empty of live processes at release except where an existing
quarantine already applies.

| Source (`d44035d`) | Live processes possible? | Report today | Disposition |
| --- | --- | --- | --- |
| `Group::create`: `openat` or limit write fails, `unlinkat` fails (`cgroup.rs:84-89`, `:108-113`) | no (never joined) | `NotStarted::Containment` → refused `mechanism_unavailable` | residue signal + latch |
| `Group` dropped after `socketpair` failure, cleanup unconfirmed | no | `NotStarted::Resource` → row F | residue signal + latch |
| `Group` dropped at the final recheck (cancel), cleanup unconfirmed | no | `NotStarted::Cancelled` → row E, **unchanged** | residue signal + latch |
| same, at shutdown | no | `NotStarted::Shutdown` → refused `stopping` | residue signal + latch; process is stopping |
| `Group` dropped after `SpawnFailed`, cleanup unconfirmed | possible | row G | existing quarantine; residue signal |
| `kill_and_confirm` false after a run | possible before kill | cleanup `false` → row H | existing quarantine; residue signal |
| service crash or SIGKILL | possible | — | systemd `KillMode=mixed` SIGKILL; next-start janitor |

"Cleanup unconfirmed" covers every `false` from `kill_and_confirm`: a failed
kill write, a failed events read, the 2 s wait, or a failed `unlinkat`. The
original proposal's claim that only an `rmdir` failure matters, and its
global "at most four orphans" claim, are **withdrawn**.

**A5: runner cleanup-residue signal (proposed ADR 0019 delta).** `Runner`
owns one `Arc<AtomicBool>` residue flag. `Group::create` receives a clone. A
leaf released without a confirmed `unlinkat == 0` stores `true` (release):
in both create-path unlink sites and in `Drop` when `kill_and_confirm`
returns `false`. `Runner::cleanup_residue(&self) -> bool` loads it (acquire).
The flag is per runner (not a process static, so parallel tests cannot share
it), monotone, payload-free and never reset in-process. Nothing else
changes: no `RunError`, `NotStarted`, `RunResult`, `Outcome`, legacy `run`
result or `ExecError` text; no `Report`, `Creation`, row, quarantine rule or
output. A cancelled run still produces the unchanged row E. The bridge's
`RunnerPort` gains `cleanup_residue()`; the test fake controls it.

**One immutable admission domain per subtree.** The bound below belongs to
one shared admission domain, not automatically to every runner in a process.
For the exclusive service `runs/` subtree, wiring owns exactly one domain for
its entire lifetime until service restart. That domain owns one runner/port,
one executor slot-accounting state, one readiness state and one cache ticket
allocator. Its runner and these shared states are bound once, are private,
and have no replacement, reset, rebind or second-runner attachment API.
Bridge handles and startup/agent discovery share that same domain by `Arc`;
cloning a handle cannot construct another runner or slot counter. A failed
runner/domain cannot be rebuilt under the same subtree before restart.
Independent runners with fresh flags are allowed only in independent test
domains with distinct owned subtrees; R3 is not permission to replace the
service runner. No process-global static is required. The still-test-only
wiring factory must encode this ownership, and future production wiring must
preserve it before activation. T20 tests shared handles after latching and
source/visibility pins forbid constructing replacement domains in wiring.

**Admission: the in-slot gate (A3).** `UnixBridge::run` executes inside the
executor's concurrency slot (after `active.fetch_add`, before
`active.fetch_sub`, `executor.rs:340-351`). Its body begins with a gate:

1. If `runner.cleanup_residue()` is set, `latch(Containment)`.
2. If readiness is not `Ready`, return `Report::Unavailable` with the D5
   token, **without calling the runner**. This is the existing report shape
   the body already returns for a refused runner (`unix_bridge.rs:74`).

After the runner returns, and before the body returns, the bridge latches
`Containment` if `cleanup_residue()` is set **or** the error was
`NotStarted::Containment` or `NotStarted::Resource` (the review-accepted
conservative U3 latch, including transient `ENOMEM`). The triggering run's
report is unchanged. Preflight remains an early refusal; the gate is the
binding check.

**Bound (with stated assumptions).** Let `L` be `max_concurrent_runs`
(at most 4 on Linux). Then at most `L` runner calls over the lifetime of that one immutable
shared admission domain can leave a residue. The service-subtree claim relies
on the one-domain/no-replacement ownership rule above. Sketch: every residue-producing call stores the flag,
then latches, before its slot release (both in the same thread, sequenced
before `fetch_sub`). Let C₁ be the residue-producing call whose slot release
is earliest in the modification order of `active`. Every `fetch_add` after
that release synchronizes with it (all operations on `active` are `AcqRel`
read-modify-writes, so they extend its release sequence), so any call taking
a slot later sees the latch at its gate and never calls the runner. Every
other residue-producing call therefore took its slot before C₁'s release and
released after it, so it held a slot at that point together with C₁: at most
`L - 1` calls. The bound assumes: the gate runs inside the slot; the latch
precedes slot release; `active` keeps `AcqRel` read-modify-writes; one
`Group` per runner call; and an unwinding (not aborting) panic. It bounds
empty leaves, not live processes; live-process sources keep their existing
quarantine.

**Already admitted work continues.** A latch does not cancel runs that have
already passed the gate. They finish with their own ordinary reports and
are counted in the bound. An empty residue leaf is not a live-process escape
(*source inference* for kernfs ownership: it is a root-owned `0700`
directory, and only a writer with write access to its `cgroup.procs`, i.e.
root, can populate it; the single-writer rule is a systemd convention, not a
kernel lock). The runner never reuses a leaf name. `Group::create` does not
look at siblings, so a residue neither blocks nor weakens later runs. It
counts toward `cgroup.max.descendants` (default `max`).

**Availability.** Cancellation alone never latches: a cancel whose cleanup
was confirmed leaves the flag clear, so the next request reaches the runner
(T13). An agent can cancel, but whether cleanup fails is a host or kernel
fault, not an agent choice (*source inference*). The latch therefore trades
availability only on a real fault, and the next-start janitor removes every
residue.

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
  2. **only if readiness is `Ready` and shutdown is not requested**, a daemon
     `discover_tools` row through a new store helper
     `start_daemon_discovery()` (`actor_kind = daemon`, principal `arxd`,
     `identity_mode = dedicated_account`; no schema change);
  3. `Execution::dispatch(None, activity, id)`. That is the ordinary path:
     bridge preflight, busy check, in-slot gate, ADR 0017 `assess`,
     quarantine and drain.
- **Janitor before discovery is strict.** Discovery never runs, and no daemon
  row is written, unless the janitor confirmed an empty `runs/`. A setup
  failure writes no row, because no action was attempted. A discovery
  failure follows ADR 0017 rows D–M for the daemon row, including quarantine
  for G–K. It is never retried at startup (U7); agent `discover_tools` can
  refresh later only while readiness permits it. A terminal latch still
  needs a restart.
- **Discovery is not the security gate** (U1). No-new-privileges and
  capability clearing apply to every run in `pre_exec`. `Ready` before the
  first discovery is acceptable only on that basis. A previous discovery
  cannot prove the current sudo policy.
- **Tool cache.** Only an outcome of `succeeded` (row N) updates it. It holds
  the response's `tools`, the response's `observed_identity` (ADR 0017 rule
  5: built from the redacted `Metadata`, never from `Evidence`) and
  `tools_observed_at`. Ordering: the caller takes a domain-local monotone
  ticket (an `AtomicU64` alongside the tool cache on `Activity`) and a wall-clock millisecond timestamp immediately
  before the start helper. Under the cache mutex an update replaces the
  whole value only if its ticket is greater than the cached ticket, so a
  slower, earlier-started discovery never overwrites a later one.
  `tools_observed_at` therefore means "the observing run started at or after
  this time", not a stronger freshness claim. The cache lives in memory only
  and is never persisted or logged; it never holds Evidence-derived
  identifiers, rights facts or raw stdout. Agent `discover_tools` row N
  refreshes it too. `list_hosts` reads it. When no cache exists, the current
  constants stay byte-identical.
- **Shutdown.** `stop_accepting` already sets the execution shutdown token,
  an execution cancellation gate independent of readiness. The setup
  sequence checks it between steps, inside the janitor wait, immediately
  before S10, after `Ready` and before the row insert. A late `Ready`
  therefore never dispatches a child: before the row insert it returns with
  no row; after the row insert, `dispatch` makes the existing synchronous
  `stopping` refusal under its workers lock (an honest terminal row, no
  runner call); after spawn, the worker's preflight, the in-slot gate and the
  runner's rechecks refuse with `stopping`. `drain` awaits `Execution::drain`,
  then the sequence's handle, then `Blocking::drain`, so drain owns all
  spawned work.
- **Deadlines are logical.** The janitor wait (5 s) and the sudo probe (5 s,
  D8) are polling deadlines checked between non-blocking steps. One cgroupfs
  `open`, `read` or `write`, a `fork`, or a reap after SIGKILL can block past
  them. Nothing claims that every syscall or task is interrupted. The
  backstops are unchanged: the shim runs inside the runner's 60 s discovery
  timeout and leaf `cgroup.kill`; the service keeps ADR 0009's 10 s drain
  failure (exit 5) and systemd's stop timeout and `KillMode=mixed`.

### D8. Rights checks (ADR 0009 slice 9, Linux standard)

| Layer | When | Fence | Exists |
| --- | --- | --- | --- |
| R1 static membership (`sudo`/`wheel`/GID 0) | every preflight | `rights_mismatch` | yes |
| R2 no supplementary groups, account-named primary group | every run, runner | `PreflightFailed` | yes |
| R3 `PR_SET_NO_NEW_PRIVS`, capabilities cleared | every run, `pre_exec` | spawn error | yes |
| R4 service hardening | setup S0 | `Hardening` → `mechanism_unavailable` | **new** |
| R5 in-fence rights observation | every tool probe (startup and agent `discover_tools`) | `Rights` latch → `rights_mismatch` | **new** |

**What these layers establish, and what they do not (P5).** R1 is the only
account-level policy observation, and only of group membership. R5 is an
observation **inside the fence**: what the shim saw while running as the
build account under R2/R3. It observes an exit status, not a root identity
and not a sudoers policy. The kernel's no-new-privileges document says that
with the bit set, `execve` "promises not to grant the privilege to do
anything that could not have been done without the execve call", and that
"the setuid and setgid bits will no longer change the uid or gid". So under
R3 a setuid `sudo` cannot elevate, and its refusal is identical with and
without a NOPASSWD rule (brief A7). The same document says NNP "does not
prevent privilege changes that do not involve `execve()`". Slice 9
therefore does **not** detect sudoers grants by user name, polkit rules, or
other non-exec privilege channels, and nothing here claims to. R2 removes
group-based channels; the remaining ones are a documented limitation. No
check removes NNP or runs an unfenced privileged probe to obtain a policy
proof.

**R4.** Read `/proc/self/status` (procfs magic, `O_NOFOLLOW`, at most 8,193
bytes read, refuse > 8,192). The file must have exactly one `NoNewPrivs:`
line with value `1` and exactly one `CapBnd:` line with 16 hex digits equal
to `00000000000000c1` (`CAP_CHOWN | CAP_SETGID | CAP_SETUID`, the constant
drop-in). The result is `service_hardening = standard`; anything else is
`none`. Exact equality is the packaged profile (U4): a subset is not
equivalent, because missing `SETUID`/`SETGID` can prevent the identity drop.
R4 does not establish the effective or permitted sets or that cgroup writes
succeed; Stage F keeps those checks. The value is rendered in `list_hosts`
only at Stage G; until then `capability()` keeps `null`.

**R5 line.** `tool-probe` appends exactly one line after the `TOOLS` lines:
`{"rights":{"no_new_privs":<true|false|null>,"sudo":"absent"|"untrusted"|"exit_nonzero"|"exit_zero"|"unknown"}}`
(≤ 96 bytes; total output stays below 4,096).

- `no_new_privs` is read by the shim, as the build account, from its own
  `/proc/self/status` with R4's bounds. It is `null` on macOS or on any parse
  failure.
- `sudo` comes from the **bound-executable probe** below. Values:
  `absent` (no candidate exists), `untrusted` (some candidate exists but
  fails a trust check; nothing is executed), `exit_nonzero`, `exit_zero`,
  and `unknown` (signal, deadline, spawn or exec error, or any other error).
  On macOS no probe runs and the value is `unknown`; macOS execution stays
  `mechanism_unavailable` before classification.

**R5 bound-executable probe (S1).** The trust boundary is the validated
executable **object**, reached only through protected system ancestors. It
rests on a stated **privileged-writer assumption**: a writer with UID 0 or
`CAP_DAC_OVERRIDE`/`CAP_FOWNER` (the package manager, the owner as root) is
trusted. Such a writer could equally change sudoers, the `arxd` binary or
the unit, so the probe adds no claim against it.

1. *Anchor.* Open `/` (`O_RDONLY|O_DIRECTORY|O_CLOEXEC`); `fstat` must show
   UID 0 and no group/other write.
2. *Candidates.* Fixed component lists, never `PATH`: C1 `usr/bin/sudo`,
   C2 `usr/local/bin/sudo`, C3 `bin/sudo`.
3. *Ancestors.* Each directory component is opened descriptor-relatively
   with `openat(parent, name, O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`
   and `fstat`ed: UID 0 and no group/other write. `ENOENT` at any component
   makes the candidate *missing*. A symlink or non-directory component makes
   it *untrusted*, except the alias rule. Any other error makes the result
   `unknown`.
4. *Normal `/bin` alias.* Only C3's first component has an alias rule. If
   `bin` is a symlink (`fstatat(anchor, "bin", AT_SYMLINK_NOFOLLOW)`) owned
   by UID 0 whose `readlinkat` value (at most 16 bytes) is exactly `usr/bin`
   or `/usr/bin`, then C3 is the merged-`/usr` alias of C1 and is dropped:
   neither opened nor executed. The link is classified, **never followed**.
   Any other symlink at `bin`, or any symlink elsewhere, is untrusted.
5. *Final object.* `openat(dir, "sudo", O_PATH|O_NOFOLLOW|O_CLOEXEC)`.
   `O_PATH` needs no read permission (some distributions ship `sudo` without
   it); with `O_NOFOLLOW` a symlink yields a descriptor of the link itself.
   `fstat` on that descriptor must show a regular file, UID 0 and no
   group/other write; otherwise the candidate is untrusted.
6. *Decision.* All three candidates are classified first. Any untrusted
   candidate → `untrusted`, and **no candidate is executed**. Otherwise the
   first trusted candidate in C1, C2, C3 order runs; none → `absent`.
7. *Execution bound to the object.* A forked child executes the validated
   descriptor with `execveat(fd, "", argv, envp, AT_EMPTY_PATH)`, never a
   path: argv `["sudo", "-n", "true"]`, an **empty environment**, stdin,
   stdout and stderr on `/dev/null`, cwd `/`. All argv, envp and descriptors
   are prepared before `fork`; the child uses only async-signal-safe calls
   and reports an exec failure through a close-on-exec error pipe, so an
   exec failure (→ `unknown`) is distinguishable from an exit status. The
   shim is single-threaded at this point (it dispatches in `main` before any
   runtime). A rename of any path entry after validation cannot redirect the
   exec; an in-place change of the validated inode is within the
   privileged-writer assumption. Because the descriptor is close-on-exec,
   an interpreter script fails with `ENOENT` (execveat(2)) and yields
   `unknown`; `sudo` is an ELF executable.
8. *Deadline.* The parent polls `waitpid(WNOHANG)` every 5 ms up to 5 s,
   then sends SIGKILL to its own child (same UID) and reaps it. Exit 0 →
   `exit_zero`; another exit → `exit_nonzero`; signal, deadline or
   spawn/exec error → `unknown`.

The dynamic loader and shared libraries of the validated executable are
resolved by path from system directories. They are covered by the same
privileged-writer assumption and are not validated individually. The empty
environment removes `LD_*` influence, which matters because under NNP the
setuid bit is not applied, so the loader does not treat `sudo` as a secure
exec (*source inference* from the NNP document's loader remark).

**Bridge parsing.** The bridge parses the line only as part of a valid
table. If it is missing, malformed, duplicated or extra, the table is
invalid: no tools, row K, quarantine and zero released tools, as today. The
line is never released, persisted or logged. A **valid** table with a
failing classification is deliberate (A2): the report stays ADR 0017 row N
with its tools, and readiness latches `Unavailable(Rights)` before the
report returns. Tools are released only after ADR 0017 assessment and full
pinned-identity match, never directly from bridge output.

**Classification** is a pure `classify_rights` over (rights, hardening,
`no_new_privs`, `sudo`):

| rights | hardening | `no_new_privs` | `sudo` | result |
| --- | --- | --- | --- | --- |
| standard | `standard` | `true` | `absent`/`exit_nonzero` | ok |
| standard | `standard` | `true` | `exit_zero`/`untrusted`/`unknown` | `rights_mismatch` |
| standard | `standard` | `false`/`null` | any | `rights_mismatch` |
| standard | `none` | any | any | `mechanism_unavailable` (R4) |
| admin | `standard` | any | any | `service_reconfiguration_required` |
| admin | `none` | `false` | `exit_zero` | ok (pure table only) |
| admin | `none` | otherwise | otherwise | `rights_mismatch` |

- "ok" means only: R4 matched, the shim had NNP, and no trusted `sudo`
  candidate exited zero inside the fence. It is not a sudoers or root-identity
  statement.
- `unknown` and `untrusted` fail closed (U8). A hung `sudo` (for example a
  slow directory service) or an unprotected system path costs availability,
  never safety, and proves no sudo policy fact.
- Linux admin stays refused before this table, by ADR 0019 D6 and
  `Runner::new`. macOS stays `mechanism_unavailable`. The admin rows are
  tested as pure logic only and stay **unverified**.
- A failing R5 result moves readiness to `Unavailable(Rights)`. Execution
  then stays unavailable until restart.

### D9. Restart, crash and shutdown recovery

- **Crash during a run.** `KillMode=mixed` makes systemd send SIGTERM to the
  main process and SIGKILL to the remaining unit processes (systemd.kill).
  `started` rows reconcile to `interrupted` (existing). Whether systemd also
  removes the delegated subdirectories at unit stop stays **unresolved** for
  the installed versions (U9); the upstream text documents process
  termination, not directory removal. The janitor handles both outcomes:
  `runs/` absent means create it; `runs/` present means kill, remove, verify.
  Stage F must verify the actual package.
- **Crash during setup or janitor.** Every step is idempotent. Enabling an
  enabled controller is a no-op, `EEXIST` is handled, and the janitor reruns.
- **Clean stop.** Drain cancels runs, the runner kills and removes its leaves
  (a failure raises the residue signal), and the setup sequence returns
  `Stopped` with no discovery.
- **Quarantine and latches** (any source) last until restart. A janitor
  failure at the next start quarantines again. There is no agent-reachable
  reset, and no owner command is added. The owner restarts the service with
  existing owner tooling.

### D10. Staging and pins

- Production pins (tests in the brief): `Execution::configured` contains
  `Self::native(` and no `UnixBridge`. `Native::start_discovery` stays empty.
  `capability()` still has `"available":false` and `"service_hardening":null`.
  `list_hosts` output without a cache is unchanged. No production module
  calls `hierarchy::` or `start_daemon_discovery`.
- **One panic-scan inventory (S2, A4).** The P-SCAN payload set grows by
  every production file of the new code: `exec/hierarchy.rs`,
  `exec/hierarchy/parse.rs`, `exec/hierarchy/setup.rs`,
  `exec/hierarchy/linux.rs`, `service/unix_bridge/readiness.rs`, and the
  changed `unix_shim.rs` and `exec/cgroup.rs` (already scanned). `fake.rs` is
  `cfg(test)` and excluded by name, which the inventory test asserts. The
  hierarchy files also must contain no `kill(`, `killpg(` or `setpriority(`,
  so `cgroup.kill` stays the only kill path there; the sudo probe's
  own-child SIGKILL lives in `unix_shim.rs`. A mutant self-test covers each
  new denylist token. Payload privacy rests on fixed payloads and the H15
  subprocess hook evidence, never on `catch_unwind` alone (ADR 0019
  C2/P1–P7); no service hook changes.
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
| raw records per listing (including `.` and `..`) | 512 | `LeafStructure` (S9: `Postcondition`) |
| child directories in one listing | 64 | `LeafStructure` |
| control-file name | 64 bytes | refused entry |
| janitor kill wait | 5 s logical, 5 ms poll, shutdown-aware | `KillTimeout` / `Stopped` |
| `bin` alias `readlinkat` | 16 bytes | untrusted |
| rights `sudo` probe | 5 s logical, kill and reap | `unknown` |
| rights line | 96 bytes | invalid table |
| residue-producing calls per immutable shared admission domain | `max_concurrent_runs` (D6 assumptions) | latched before |

Every cgroupfs open is descriptor-relative with `O_NOFOLLOW|O_CLOEXEC`
(directories also `O_DIRECTORY`; listings use a fresh description). Nothing
is path-based after the trusted root, and nothing is canonicalized. The
worst-case probe time is 9 tools × 5 s + 5 s = 50 s, inside the runner's
60 s discovery timeout; exceeding it is the runner's existing timeout path.

## Consequences

- The service owns one more privileged code path, limited to its own
  delegated subtree. It never touches `supervisor/`, `.control/`, the slice or
  any other unit.
- Execution becomes available only after a confirmed-empty `runs/`, and
  startup discovery only after that. Agents see `mechanism_unavailable` while
  setup is pending (U5).
- A containment fault or an unconfirmed leaf cleanup latches execution off
  until restart, with no report change. At most `max_concurrent_runs` runner
  calls can leave a residue leaf per immutable shared admission domain (D6);
  service wiring permits exactly one such domain for its exclusive subtree
  until restart, with no replacement.
- `sudo -n true` runs as the build account, bound to a validated root-owned
  executable, at each discovery. On many distributions that writes an
  authentication log entry.
- A false `rights_mismatch` is possible (a slow `sudo`, an unprotected
  candidate). A false pass from an agent-planted `sudo` needs a write to a
  root-owned, not group/other-writable system ancestor, or to the validated
  inode, which is outside the build account's reach under the
  privileged-writer assumption. A pass is never a sudoers-policy proof.

## Relation to other ADRs

- **ADR 0007.** The runner, its precondition checks and its cleanup are
  unchanged except for the A5 residue signal. The janitor adds a second kill
  site that uses the same primitive (`cgroup.kill`).
- **ADR 0009.**
  - *Clarification:* the hierarchy setup sentence is made exact (unit and
    `runs/`, S5/S8).
  - *Clarification:* the janitor runs after ready, in the background, still
    before execution becomes available.
  - *Extension:* Linux standard requires `service_hardening = standard`.
  - *Deviation:* the `sudo -n true` effect is observed at each discovery, not
    before every run, and its meaning is narrowed to an in-fence observation
    (D8). R1–R3 still apply to every run, and R3 is the kernel fence that
    makes a setuid `sudo` ineffective. Running `sudo` per run would log per
    run and add a child per request.
  - The janitor failure maps to `quarantined`, its existing meaning.
- **ADR 0017.** No change to `Report`, `Creation`, rows, quarantine or output
  release. The daemon discovery row uses the same rows. The tool cache follows
  rules 5 and 6 (redacted `Metadata` only; no protected identifier in any
  cache).
- **ADR 0019 amendments** (to its accepted inactive contract; each needs the
  Sol recheck):
  - A1: one `locate` parser shared by the bridge and setup. It additionally
    requires `arxd.service` as the unit component (T3). This intentionally
    refuses inputs that Stage D accepted with another unit component; the
    T2 fixtures are amended to assert the new refusals (brief P1).
  - A2: the discovery table is `TOOLS.len()` lines plus one rights line
    (brief §4 discovery and S6 output shape). Tools still come only from the
    `TOOLS` lines. The shim's sudo probe executes by descriptor (D8).
  - A3: the bridge's `ready: bool` becomes `Readiness` with the D6 and D8
    latches and the in-slot gate.
  - A4: the P-SCAN set and denylist are extended (D10).
  - A5: the runner cleanup-residue signal (D6). It is the only runner change
    and leaves every runner result, report, row and output as it is.

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
- **Latching on every cancellation.** It would let an agent switch execution
  off at will. The residue signal latches only on unconfirmed cleanup.
- **Documenting the cancel residue as unbounded.** Rejected: the narrow A5
  signal plus the in-slot gate bounds it without changing any report.
- **Moving the executor's preflight inside the slot.** It would change the
  busy-versus-refusal precedence for every backend; the bridge-local in-slot
  gate achieves the bound without it.
- **Executing `sudo` by path after `lstat`.** A replaceable ancestor or a
  check-to-exec rename could run a different object (S1).
- **Resolving `sudo` through the owner `PATH`.** That allows an agent-planted
  false pass.
- **Removing NNP or adding an unfenced privileged probe** to prove sudoers
  policy. It would weaken the fence the check exists to observe.
- **Signals for cleanup.** They are impossible without `CAP_KILL`, and
  UID-scoped signals are not a tree boundary.

## Review decisions applied (from the Sol review)

U1 discovery-time diagnostics with narrowed meaning (D7, D8). U2 mandatory
exact `user.delegate` (D2). U3 conservative `Containment`/`Resource` latch,
with the bound now carried by A5 and the in-slot gate (D6). U4 exact `CapBnd`
(D8). U5 Pending → `mechanism_unavailable` (D5). U6 kill the whole subtree,
refuse unknown names without removing them (D4). U7 no startup retry (D7).
U8 `unknown` → `rights_mismatch`, with `untrusted` distinct from `absent`
(D8). U9 systemd directory removal stays unresolved for Stage F (D9).

## Residual decisions for the recheck

- R-1: accept A5 as the only runner change, or prefer documenting an
  unbounded empty-leaf residual (rejected above).
- R-2: accept dropping C3 as an alias of C1 when `bin` is the exact
  merged-`/usr` link, and treating every other link as `untrusted`
  (availability cost on unusual layouts).
- R-3: accept the control-file name grammar in D4.3 as fail-closed
  classification (a future kernel file outside it would refuse setup).
- R-4: accept `VerifyFailed` as a quarantine (cleanup unconfirmed).

## Sources and inference

See [0020-design-evidence.md](../reviews/0020-design-evidence.md) for the
file list, the primary documents (kernel cgroup-v2 and no-new-privileges,
systemd delegation, `systemd.resource-control`, `systemd.exec`,
`systemd.kill`, kill(2), open(2), dup(2), opendir(3), execveat(2)) with fetch
details, and the full inference list. Hardware, kernel and systemd behaviour
on the owner's machine is **unverified on real hardware**.
