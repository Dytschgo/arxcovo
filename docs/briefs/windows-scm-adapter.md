# Brief: Windows SCM adapter (ADR 0012 PR-C)

Status: ready for **Sol, high effort**, after an independent Sol review of this
brief and the matching ADR 0012 SCM amendment. Prepared by Opus 5.5 high
(architecture) from base `feat/windows-owner-store` at
`d6df821dcd2ec827bcf2cd3c86bea32a2c639ca4`. The PR-C implementation is then
reviewed by Opus, a different provider. This document describes no
implementation. Every native SCM behaviour named here is **unverified on real
hardware**.

## 1. Goal

Add a library-only Windows SCM adapter: `observe`, `register`, `start`, `stop`,
`status` and `deregister`. It implements the ADR 0012 **SCM** contract, as
amended for the ADR 0014 decision 1 boot-intent rule, and the ADR 0012 N11
tests. It adds no CLI, MCP route, session change, `Plan::execute` route or
installed activation.

## 2. Requirement sources and deviations

| Item | Source | Status |
| --- | --- | --- |
| Install demand-start; `start` switches to delayed auto before `StartService`; `stop` switches to demand after a confirmed stop | ADR 0014 decision 1, "Windows intent" (accepted). Pinned object: `b052d94058444f56186ed0a75c5e1e79196d35a3:docs/adr/0014-unix-owner-lifecycle-corrections.md` | Adopted. Supersedes `SERVICE_AUTO_START` at creation (ADR 0012 earlier text, ADR 0009 "Windows SCM host entry") |
| Identity: own process, exact quoted image, LocalSystem; a foreign `arxd` is never started, stopped or deleted; config2 values; uninstall order stop → delete → `uninstall_right`; stop bound 30 s | ADR 0012 SCM, ADR 0009 | Kept. "Auto start" is removed from identity and becomes "demand or auto" |
| Install refuses an active service; converges otherwise; the last successful explicit command decides boot; a failed start leaves the service stopped but enabled | ADR 0014 decisions 1 and 7 | Applied to Windows |
| Fixed tokens `service-not-installed`, `service-foreign-registration`, `service-active` | ADR 0014 decision 3 | Same variants and tokens added on this branch |
| **Deviation D-a.** Windows stop changes persistence *after* the runtime change | ADR 0014's explicit Windows text, which conflicts with its general "persistence before runtime" sentence | The Windows text governs. A failed stop keeps the boot intent of the last successful command |
| **Decision D-b.** Accepted start types are exactly demand and auto; boot, system and disabled are Foreign | New, minimal | Arxcovo writes only those two. Anything else is not something Arxcovo can classify safely |
| **Decision D-c.** `ERROR_SERVICE_NEVER_STARTED` (1077) with specific code 0 maps to `stopped` | New | A freshly installed or rebooted demand service reports 1077. ADR 0012's literal "nonzero → failed" would report `failed` for a clean install |
| **Decision D-d.** Failure actions are three entries: restart 5000, restart 30000, none 0 | Clarifies ADR 0009 "then none" | The SCM repeats the last action, so two entries would restart forever |
| **Decision D-e.** Start polls to `RUNNING` with a 35 s bound | New | ADR 0009 has a 30 s cooperative startup deadline. The 35 s matches ADR 0014's mutating-vector bound |
| **Decision D-f.** Start and status require Installed (identity plus all config2 values); stop and uninstall require only Ours | New | Never run with an unrestricted token or missing recovery policy, but keep a half-installed service stoppable and removable |
| **Decision D-g.** Marked-for-delete (1072) is accepted only by `deregister` (on the demand change and on `DeleteService`); everywhere else it is `Manager` | New | Lets an interrupted uninstall rerun to completion. It is never read as "absent" |
| **Decision D-h.** Display name `arxd`, `SERVICE_ERROR_NORMAL`, no group, tag or dependencies; none of these is compared | New | ADR 0009 did not specify them. They add no new string |

## 3. Scope

**Do**

- Add `crates/arxd/src/service/owner_scm.rs`, which builds on all hosts: types,
  constants, classification, status mapping and the lifecycle engine, generic
  over `ScmCalls` and `Clock`.
- Add `owner_scm_tests.rs`, which runs on all hosts.
- Add `owner_scm_windows.rs` under `#[cfg(windows)]`: a bounded buffer parser
  and the `NativeScm` FFI that implements `ScmCalls`, with synthetic-structure
  tests in the same file or in `owner_scm_windows_tests.rs`.
- Add `NotInstalled`, `Foreign` and `Active` to `layout::Error`. Use exactly
  the variant names, tokens and `Display` arms of `b052d940`
  `crates/arxd/src/service/layout.rs`, so that integration is a pure textual
  merge.
- Make `owner_plan::registration` `pub(super)`. The adapter consumes
  `Registration` and does not duplicate its values.
- Update the `service-foundation.md` PR-C section: API, limits and the
  unverified items in section 12. Do not edit `PROGRESS.md`; the coordinator
  records evidence there.

**Do not**

- Do not add a CLI, an MCP route, an `OsManager` implementation for
  `Operation::Scm`, a `Plan::execute` path or a change to
  `WindowsOwnerSession` or `parts()`.
- Do not reference `owner_transactions`, `OwnerStore`, `BatchRight`,
  `WindowsBatchRight`, `uninstall_right` or `parts(` from any `owner_scm*`
  file.
- Do not add a new crate. `Win32_System_Services` and `Win32_Foundation` are
  already enabled.
- Do not add a password, an S4U retry, a fallback account, an approval prompt
  or any owner admin step.
- Do not make any real SCM call in any test. That includes `OpenSCManagerW`
  and read-only queries (ADR 0012 native-test rule). Never run owner
  subcommands on the owner PC.

## 4. Constants (`owner_scm.rs`)

```text
SERVICE_NAME = "arxd"            DISPLAY_NAME = "arxd"
ACCOUNT      = "LocalSystem"     ARGS (from Registration) = service run
TYPE         = 0x10 (SERVICE_WIN32_OWN_PROCESS, exact)
START_AUTO = 2, START_DEMAND = 3           ERROR_CONTROL = 1 (NORMAL)
STOP_DEADLINE = 30 s   START_DEADLINE = 35 s   POLL_INTERVAL = 250 ms
QUERY_BUF = 8192 bytes, 8-byte aligned ([u64; 1024]), one call per query
FAILURE_ACTIONS = [(RESTART,5000),(RESTART,30000),(NONE,0)], reset 86400 s,
                  reboot message "" and command "" on write
```

Mirror the Win32 values used by the pure engine as plain constants, such as
1056, 1060, 1061, 1062, 1066, 1072, 1073, 1077 and 1078, plus the state and
start-type values. A `#[cfg(windows)]` test asserts that each one equals its
`windows-sys` constant.

The command line is built once from `Registration.image`. It must be absolute,
contain no `"` and no NUL, and it is UTF-16 encoded with `encode_wide`. The
result is `"` + image + `"` + ` service run` with no terminator. Any violation
is `Error::Layout`.

## 5. API (exact shape; private fields everywhere)

```rust
pub enum Status { NotInstalled, Stopped, Starting, Running, Stopping, Failed } // token(): ADR 0014 strings
pub enum Boot { Demand, Auto }
pub enum Observation { Absent, Ours { boot: Boot, installed: bool, active: bool }, Foreign }
#[must_use] pub struct Deregistered(());          // only `deregister` constructs it

pub(super) trait Clock { fn now(&self) -> Duration; fn sleep(&mut self, d: Duration); }
pub(super) trait ScmCalls { /* section 6 */ }

pub struct WindowsScm<C: ScmCalls = NativeScm, K: Clock = MonotonicClock> { .. }
impl WindowsScm {                                  // cfg(windows), production
    pub(super) fn system(layout: &Layout) -> Result<Self, Error>; // Windows layout only; no path argument
}
impl<C: ScmCalls, K: Clock> WindowsScm<C, K> {
    #[cfg(test)] pub(super) fn for_test(layout: &Layout, calls: C, clock: K) -> Self;
    pub fn observe(&mut self) -> Result<Observation, Error>;     // read-only
    pub fn register(&mut self) -> Result<(), Error>;
    pub fn start(&mut self) -> Result<(), Error>;
    pub fn stop(&mut self) -> Result<(), Error>;
    pub fn status(&mut self) -> Result<Status, Error>;           // read-only
    pub fn deregister(&mut self) -> Result<Deregistered, Error>;
}
```

`active` means any state other than `SERVICE_STOPPED`. No type
`Debug`-prints a command line, a buffer or a Win32 code. `Error` is the only
error that leaves the module.

## 6. Native seam

```rust
pub(super) struct Win32(pub u32);                  // never displayed
pub(super) enum Fault { Win32(u32), Malformed }    // Malformed = any parse/bounds failure
pub(super) enum Access { Status, Start, Stop, Install, Uninstall }
pub(super) trait ScmCalls {
    fn open(&mut self, a: Access) -> Result<bool, Win32>;          // false = 1060
    fn create(&mut self, cmdline: &[u16]) -> Result<(), Win32>;     // CreateServiceW, demand
    fn config(&mut self) -> Result<BaseConfig, Fault>;              // QueryServiceConfigW
    fn config2(&mut self, level: Level) -> Result<Config2, Fault>;  // QueryServiceConfig2W
    fn state(&mut self) -> Result<Snapshot, Fault>;                 // QueryServiceStatusEx
    fn set_start(&mut self, b: Boot) -> Result<(), Win32>;          // ChangeServiceConfigW, rest SERVICE_NO_CHANGE/NULL
    fn set_config2(&mut self, v: &Config2) -> Result<(), Win32>;
    fn start(&mut self) -> Result<(), Win32>;                       // StartServiceW(0, NULL)
    fn control_stop(&mut self) -> Result<(), Win32>;                // ControlService(STOP)
    fn delete(&mut self) -> Result<(), Win32>;
}
```

`BaseConfig` holds `service_type`, `start_type`, `image: Vec<u16>` and
`account: Vec<u16>`. `Snapshot` holds `state`, `win32_exit` and
`specific_exit`. `Config2` is a typed enum, one variant per level 2 to 7.

`NativeScm` rules:

- **Handles.** Hold one manager handle and at most one service handle, each
  closed by `Drop`. The manager is opened with `SC_MANAGER_CONNECT`, and for
  `Install` also `SC_MANAGER_CREATE_SERVICE`. Service access by operation:
  - Status: `QUERY_CONFIG | QUERY_STATUS`;
  - Start: those plus `CHANGE_CONFIG | START`;
  - Stop: those plus `CHANGE_CONFIG | STOP`;
  - Install: `QUERY_CONFIG | QUERY_STATUS | CHANGE_CONFIG | START`.
    `SERVICE_START` is needed to set restart failure actions;
  - Uninstall: Stop's access plus `DELETE`.
  `CreateServiceW` asks for Install access.
- **`create` arguments.** `SERVICE_WIN32_OWN_PROCESS`,
  `SERVICE_DEMAND_START`, `SERVICE_ERROR_NORMAL`, the command line, and
  `lpServiceStartName` and `lpPassword` NULL. The group, tag and dependencies
  are NULL, and the display name is `arxd`.
- **Bounded queries.** Each config query passes the fixed 8 KiB aligned buffer
  once. `ERROR_INSUFFICIENT_BUFFER` or any failure becomes `Fault`, and the
  buffer is never grown. The parser is safe code. It reads fields at
  `offset_of!` offsets of the `windows-sys` structs from the byte slice. It
  turns each pointer into `ptr - base` and requires, otherwise `Malformed`:
  - the range is within `[0, 8192)`;
  - UTF-16 strings are 2-byte aligned with a NUL inside the buffer;
  - the multi-string has at most 16 entries of 1 to 64 units each, with a
    double NUL inside the buffer;
  - `SC_ACTION` arrays are 4-byte aligned, `cActions ≤ 8` and the array is
    in bounds.

  A NULL string pointer reads as empty. The status query uses
  `SC_STATUS_PROCESS_INFO` into one `SERVICE_STATUS_PROCESS`, and requires
  the `dwServiceType` own-process bit.
- **Raw values.** Raw pointers are never dereferenced outside this checked
  slice view.
- **Clock.** `MonotonicClock` uses `Instant` and `thread::sleep`.

## 7. Classification

- **Ours**: type == 0x10, `image == cmdline` (exact `u16` equality), `account
  == "LocalSystem"` (exact) and start type in {2, 3}. Anything else is
  **Foreign**. A parse `Fault` is `Manager`, never Foreign or Absent.
- **Installed**: Ours plus these config2 values:
  - delayed == TRUE;
  - SID type == 1 (unrestricted);
  - privileges == the `Registration` set: exactly 4, no duplicates, order
    ignored;
  - preshutdown == 30000;
  - failure actions exact as in section 4, with reboot message and command
    empty or NULL;
  - non-crash flag == FALSE.

## 8. Procedures (exact call order; every listed failure halts at once)

`O` means: open (false → the absent branch), then `config` → classify. Foreign
→ `Foreign`, and a fault → `Manager`.

**observe** (read-only; PR-D's `Expect(Installable)` source): open `Status`, `O`.
If Ours, read all six `config2` levels and the `state`.

**register**

1. Open `Install`.
   - **Absent.** `create`:
     - Ok → step 3;
     - 1073 → open again once, then continue at step 2 (a second absent is
       `Manager`);
     - 1078 → `Foreign`;
     - 1072 or anything else → `Manager`.
   - **Present.** Continue at step 2.
2. Run `O`, then `state`. Anything but stopped → `Active`, with no mutation.
   Then `set_start(Demand)`. That is the first mutation, so boot is off before
   anything else.
3. Run `set_config2` in this order: delayed TRUE, SID unrestricted,
   privileges, preshutdown, failure actions, non-crash flag FALSE.
4. Postcondition: run `config` and the six `config2` reads, then require
   Installed with `Boot::Demand`. If that fails, the result is `Foreign` when
   the identity changed and `Manager` otherwise. Then `state` must be stopped,
   else `Manager`. Boot is already demand at that point.

**start**

1. Open `Start`. Absent → `NotInstalled`.
2. Run `O` and the `config2` reads. Ours but not Installed → `NotInstalled`.
3. Read `state`:
   - stopped, start pending or running → continue;
   - anything else → `Manager`, with no mutation.
4. Run `set_start(Auto)`, then `set_config2(delayed TRUE)`.
5. If the state from step 3 was stopped, call `start`. Ok and 1056 both
   continue; anything else → `Manager`.
6. Poll `state` until running → Ok. Stopped → `Manager` at once.
   `START_DEADLINE` reached → `Manager`. A `Fault` → `Manager`.

**stop** (also the first part of `deregister`)

1. Open `Stop`. Absent → `NotInstalled`. Then run `O`; Ours is sufficient.
2. Poll loop, with the deadline set at the first `state` read:
   - stopped → go to step 3;
   - running → `control_stop`, at most once per running observation.
     Ok, 1061 and 1062 continue polling; any other code → `Manager`;
   - start pending or stop pending → keep polling;
   - paused or continue states → `control_stop`, the same as running;
   - an unknown state → `Manager`.

   Reaching `STOP_DEADLINE` → `Manager`. The start type stays unchanged and
   nothing is deleted.
3. Confirmed stop → `set_start(Demand)`. Any error → `Manager`. The service is
   then stopped but auto, and a rerun of `stop` or `install` settles it.

**status**

1. Open `Status`. Absent → `Ok(NotInstalled)`.
2. Run `O` and the `config2` reads. Not Installed → `Err(NotInstalled)`.
3. Map `state`, as in ADR 0012 as amended:
   - stopped with win32 ∈ {0, 1077} and specific 0 → `Stopped`;
   - stopped otherwise → `Failed`;
   - start pending → `Starting`;
   - running → `Running`;
   - stop pending → `Stopping`;
   - paused, pause pending or continue pending → `Failed`;
   - an unknown value → `Manager`.

**deregister**

1. Open `Uninstall`. Absent → `Ok(Deregistered)`.
2. Run `O`. Foreign → `Foreign`.
3. Run the stop loop from `stop` step 2.
4. Run `set_start(Demand)`. Ok and 1072 both continue; anything else →
   `Manager`.
5. Run `delete`. Ok and 1072 both continue; anything else → `Manager`.
6. Read `state` on the same handle. A stopped state, or a `Fault::Win32(1060)`
   (the service is already gone), → `Ok(Deregistered)`. Anything else →
   `Manager`. That includes a restart that the SCM's recovery scheduled after
   a crash during stop.

A rerun of `deregister` after any failure converges. For a stopped,
marked-for-delete service the rerun is: the demand change returns 1072,
which is accepted; the delete returns 1072, which is accepted; then the
status check.

## 9. Error table

| Native result | observe | status | register | start | stop | deregister |
| --- | --- | --- | --- | --- | --- | --- |
| open → 1060 | Absent | `not-installed` | create path | `NotInstalled` | `NotInstalled` | Deregistered |
| identity mismatch or start type ∉ {2,3} | Foreign | `Foreign` | `Foreign` | `Foreign` | `Foreign` | `Foreign` |
| Ours, config2 incomplete | `installed: false` | `NotInstalled` | converges | `NotInstalled` | proceeds | proceeds |
| state ≠ stopped | `active: true` | mapped | `Active` | see 8 | see 8 | see 8 |
| create 1073 / 1078 / 1072 | — | — | reopen once / `Foreign` / `Manager` | — | — | — |
| set_start 1072 | — | — | `Manager` | `Manager` | `Manager` | accepted |
| start 1056 | — | — | — | treated as started | — | — |
| control_stop 1061, 1062 | — | — | — | — | keep polling | keep polling |
| delete 1072 | — | — | — | — | — | accepted |
| deadline reached | — | — | — | `Manager` | `Manager` | `Manager` |
| insufficient buffer, `Malformed`, any other Win32 code | `Manager` | `Manager` | `Manager` | `Manager` | `Manager` | `Manager` |

## 10. Expected transitions (each row is a required fake-SCM test)

| From → action | Calls after checks | Result | Starts at next boot |
| --- | --- | --- | --- |
| absent → register | create(demand), 6× set_config2 | stopped | no |
| installed stopped → start | set_start(auto), delayed, start, poll | running | yes |
| running → stop | control_stop, poll, set_start(demand) | stopped | no |
| running → start | set_start(auto), delayed; no `start` | running | yes |
| stopped → stop | set_start(demand); no control | stopped | no |
| stopped but auto (failed start) → register | set_start(demand), 6× set_config2 | stopped | no |
| running or pending → register | none (`Active`) | unchanged | unchanged |
| Ours, config2 partial (crash after create) → register | set_start(demand), 6× set_config2 | stopped | no |
| config2 partial → start, status | none (`NotInstalled`) | unchanged | unchanged |
| absent → start or stop | none (`NotInstalled`) | unchanged | unchanged |
| absent → deregister | none | Deregistered | no |
| running → deregister | control_stop, poll, set_start(demand), delete, state | Deregistered | no |
| marked for delete, stopped → deregister | set_start → 1072, delete → 1072, state | Deregistered | no |
| foreign (each mismatch) → any | none (`Foreign`) | unchanged | unchanged |
| start: `start` fails or the deadline passes | set_start(auto), delayed, start or poll (`Manager`) | stopped (or late running) | yes |
| start: set_start fails | set_start (`Manager`) | stopped | as before |
| stop: the deadline passes | control_stop, poll (`Manager`); no set_start | possibly running | as before (yes) |
| stop: set_start(demand) fails after a confirmed stop | `Manager` | stopped | yes, until a rerun |

## 11. Required tests

All of these are unprivileged and make no real SCM call.

1. **Fake-call ordering.** Every row in section 10, using a stateful
   `FakeScm`. It records each call and models `exists`, `marked`, start type,
   delayed, config2, state and exit codes, and it can inject failures by call
   index. A `FakeClock` advances only on `sleep`, so no real time passes.
   Assert the exact recorded sequence. After a failure, assert that no call
   follows except reads already listed.
2. **Boot-persistence model** (label it a model). Reboot is `running :=
   exists && !marked && start == auto`. Assert:
   - install→reboot = stopped;
   - install→start→reboot = running;
   - start→stop→reboot = stopped;
   - start→register refuses, and reboot = running;
   - a failed `start` → reboot = running, and then register→reboot = stopped;
   - stop deadline → reboot = running;
   - a crash at each register call, then register again, then reboot =
     stopped.

   A crash means the call sequence is cut off after call *k*.
3. **Ordering invariants.**
   - `register` never calls `set_start(Auto)`, `start` or `control_stop`.
   - In `start`, `set_start(Auto)` and the delayed write come before `start`.
   - In `stop`, `set_start(Demand)` comes only after a stopped observation.
   - In `register` on an existing service, `set_start(Demand)` is the first
     mutation, after the `Active` check.
   - Reads run before any mutation in every operation.
4. **Classification.** Each single-field mismatch is Foreign:
   - type 0x110, 0x20 or 0x50;
   - start type 0, 1 or 4;
   - an image that differs by case, by a missing quote, by a trailing space,
     by a different argument or by an unquoted form;
   - an account of `localsystem`, `.\LocalSystem` or `NT AUTHORITY\SYSTEM`.

   Each config2 field mismatch, two failure actions, and a non-empty reboot
   message or command are not Installed. A parse fault is `Manager`.
5. **Status mapping.** Every state value, plus the exit pairs (0,0) and
   (1077,0) → stopped, and (1066,7), (2,0) and (1077,5) → failed. An unknown
   state is `Manager`.
6. **Deadlines.** A service that never stops gets `Manager` after at most
   `ceil(30 s / 250 ms) + 2` state reads and no `set_start` or `delete`. A
   start that never runs gets `Manager` at 35 s. 1061 during start pending
   followed by running → exactly one more control.
7. **Provenance boundary.**
   - Every `deregister` failure injection (each call × {each Win32 code in
     section 9, `Malformed`}) returns `Err` and no `Deregistered`.
   - A source scan asserts that the `owner_scm*` files contain none of
     `owner_transactions`, `OwnerStore`, `BatchRight`, `uninstall_right` or
     `parts(`.
   - A source scan asserts that `Deregistered(` is constructed only inside
     `deregister`.
   - Test files never name `NativeScm`, `WindowsScm::system`,
     `OpenSCManagerW` or `CreateServiceW`.
8. **Synthetic native structures** (`#[cfg(windows)]`). Build
   `QUERY_SERVICE_CONFIGW`, `SERVICE_FAILURE_ACTIONSW`,
   `SERVICE_REQUIRED_PRIVILEGES_INFOW`, `SERVICE_DELAYED_AUTO_START_INFO`,
   `SERVICE_SID_INFO`, `SERVICE_PRESHUTDOWN_INFO`,
   `SERVICE_FAILURE_ACTIONS_FLAG` and `SERVICE_STATUS_PROCESS` inside the
   aligned 8 KiB buffer, with real internal pointers. Cover:
   - the valid case;
   - a pointer below the base or at or beyond the end;
   - an odd-aligned string;
   - a missing NUL or double NUL;
   - 17 privileges or a 65-unit privilege;
   - `cActions = 9`;
   - a misaligned action array;
   - NULL strings;
   - the constant mirrors equal to `windows-sys`.
9. **Command line.** The exact UTF-16 sequence for `Layout::system`-shaped and
   `for_test` Windows layouts. An image containing `"`, a NUL or a relative
   path gives `Layout`.
10. **Mutation evidence.** Each of these must fail at least one test. Report
    them in the PR body:
    - create with auto;
    - start without `set_start(Auto)`, or with it after `start`;
    - stop calling `set_start(Demand)` before a confirmed stop;
    - accepting disabled as Ours;
    - 1077 mapped to failed;
    - two failure actions;
    - a case-insensitive image compare;
    - `deregister` returning Ok on a delete error other than 1072;
    - removing the deadline;
    - register not refusing start pending.

**Checks to run and report with exact commands.**

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo nextest run -p arxd service::owner_scm` on Windows and on one Unix
  host for the pure part
- the full `cargo nextest run -p arxd` on Windows

CI must stay under the 15-minute budget, and these tests are pure.

## 12. Remaining evidence (all **unverified on real hardware**)

PR-C cannot establish any of these. They need the separately reviewed
disposable hosted-runner experiment in ADR 0012, never the owner PC.

- `lpServiceStartName` reads back exactly `LocalSystem` after a NULL account.
  The image reads back unchanged.
- `ChangeServiceConfig2W(DELAYED_AUTO_START_INFO)` succeeds on a demand-start
  service, and `ChangeServiceConfigW` preserves or resets the flag.
- A never-started or rebooted demand service reports 1077.
- A crash during stop schedules a recovery restart, and `DeleteService` blocks
  or allows it.
- `OpenServiceW`, `ControlService`, `QueryServiceStatusEx` and
  `ChangeServiceConfigW` behave on a marked-for-delete service as section 8
  assumes.
- The SCM preserves the privilege multi-string order, and the 8 KiB buffers
  are sufficient.
- Auto-start versus demand behaviour across a real reboot.
- The sufficiency of the four required privileges (ADR 0009).

If any assumption fails, the adapter fails closed with `Manager` or
`Foreign`. A fix is a contract amendment, never a fallback.

## 13. PR-D prerequisites (not part of PR-C)

- Narrow `WindowsOwnerSession::parts()`. Windows uninstall must be one
  function that takes a `Deregistered` by value before it calls
  `uninstall_right`. A fake-call test must show that no store or right call
  follows any SCM `Err`.
- Windows install order: elevation and the session preconditions; then
  `observe()`, which must be Absent or Ours and inactive, before any
  directory, lock or state is created; then layout creation; then
  `register()` last. Start runs the ADR 0009 start preflight before `start()`.
- Windows protected-state provisioning, the counterpart of ADR 0014 decision
  6, is still open and outside ADR 0012.
- `Operation::Scm` and `Plan::execute` stay fail-closed
  (`NativeAdapterMissing`) until PR-D replaces them.
- The coordinator merges the `layout::Error` variants with the Unix branch.
  The text is identical. Unifying `owner_scm::Status` with
  `owner_query::Status` is optional and changes no behaviour.
