# Brief: Linux execution hierarchy, janitor, startup discovery and rights (ADR 0020)

Status: **proposal** (Anthropic / claude-opus-5-5 / high, design sub-agent,
2026-11-12, runtime base `fdea1c6`). It is the implementation contract for
[ADR 0020](../adr/0020-linux-execution-hierarchy-and-janitor.md), which is
**proposed, not accepted**. Nothing here was built, run or tested. Every
test, mutation and acceptance item below is a requirement, not evidence.

## Gates before any slice starts

1. A fresh independent Sol high review accepts ADR 0020 and this brief. If
   Sol is limited, the fallback is grok-4.7 high. Opus authored this, so Opus
   cannot review it.
2. ADR 0019 stage C (PR53) and stage D (PR55) are merged with all required
   gates. Both are unmerged at `fdea1c6`. Slices E4 and E5 change their code.
3. No slice may activate anything. Production stays `MechanismUnavailable`
   (pins in E6).

Stage F (`E_PROTECTION` diagnosis, then a separately reviewed disposable
experiment) and Stage G (activation) stay separate. No native experiment
retry, owner, account, service or cgroup operation is authorized by this
brief.

## Slice order (one concern per PR; Sol builds, Opus reviews each)

| Slice | Concern | Files (new or changed) | OS where its tests run |
| --- | --- | --- | --- |
| E1 | pure parsers and classifiers | `exec/hierarchy.rs` (new module root), `exec/hierarchy/parse.rs` (new); bridge parser delegates to `locate` | all |
| E2 | setup and janitor state machine over a fake `CgroupFs` | `exec/hierarchy/setup.rs` (new), `exec/hierarchy/fake.rs` (`cfg(test)`) | all |
| E3 | Linux descriptor-relative `CgroupFs` | `exec/hierarchy/linux.rs` (new, `cfg(target_os = "linux")`) | Linux |
| E4 | shim rights line | `unix_shim.rs` | Linux, macOS |
| E5 | bridge readiness and latches | `service/unix_bridge.rs`, `service/unix_bridge/readiness.rs` (new) | all |
| E6 | startup wiring, daemon row, tool cache | `service/application.rs`, `store.rs`, `mcp.rs`, `service/executor.rs` (test seams only) | all |

E1 → E2 → E3 is a chain. E4 needs only E1. E5 needs E1, E2 and E4. E6 needs
E5. At most two builders run at a time.

## Slice E1: pure parsers and classifiers (any OS)

APIs. All are pure. Inputs are bytes, and nothing panics on any input.

- `locate(&[u8]) -> Option<Location>`: the Stage D `cgroup_parent` rules plus
  ADR 0020 T3 (the component before `supervisor` is exactly `arxd.service`).
  `Location::{unit_components(), runs()}`. The bridge's `cgroup_parent`
  becomes `locate(bytes).map(|l| l.runs())`.
- `leaf_name(&[u8]) -> bool`: `arx-<pid>-<serial>`, canonical decimals with
  no leading zero, at most 10 and 20 digits.
- `events(&str) -> Option<bool>`: every line is `<lowercase key> <decimal>`
  with no duplicate key, and there is exactly one `populated` line, `0` or
  `1`. Other well-formed keys are ignored. It returns populated.
- `controllers(&str) -> Controllers`: exact whitespace-split tokens.
  `has(memory) && has(pids)`.
- `pids(&str) -> Option<Vec<u32>>`: newline-separated canonical decimals at
  most 4,096 bytes long. The empty string means none.
- `hardening(&[u8]) -> Hardening` (`Standard | None`): at most 8,192 bytes;
  exactly one `NoNewPrivs:\t1` and exactly one `CapBnd:\t00000000000000c1`
  (16 lowercase hex digits). Anything else is `None`.
- `rights_line(&str) -> Option<RightsFact>`: exactly
  `{"rights":{"no_new_privs":…,"sudo":…}}` with exactly those keys and types,
  and at most 96 bytes.
- `classify_rights(Rights, Hardening, RightsFact) -> Result<(), ExecutionUnavailable>`:
  the ADR 0020 D8 table.

Tests:

- **P1** Every Stage D T2 fixture keeps its result through `locate`. New
  refusals: `foo.service/supervisor`, `arxd.service/x/supervisor`, and
  `arxd.service.d/supervisor`. Accepted: `/arxd.service/supervisor`
  (root slice) and a three-level slice. Equality: for every fixture, the
  bridge parser equals `locate(..).map(runs)`.
- **P2** Leaf grammar: accept `arx-0-0` and `arx-4194304-18446744073709551615`.
  Refuse `arx-01-2`, `arx-1-02`, `arx--1`, `arx-1-`, `arx-1-2-3`,
  `ARX-1-2`, `arx-+1-2`, an 11-digit PID, a 21-digit serial, and a trailing
  newline.
- **P3** Events: `populated 0\nfrozen 0\n` gives `false`, and so does
  `populated 0\nfuture_key 7\n`. Refuse a duplicate `populated`, a duplicate
  other key, a missing `populated`, `populated 2`, `Populated 0`, a
  non-decimal value, CRLF and 4,097 bytes.
- **P4** Controllers: `memoryx pids` and `memory` alone fail.
  `cpu memory pids\n` passes.
- **P5** PIDs: refuse a leading zero, `+1`, an embedded space and over
  4,096 bytes. The empty string means none.
- **P6** Hardening: `c1` exact passes. `c0`, `1c1`, uppercase `C1`,
  `NoNewPrivs:\t0`, duplicate lines, a missing line and 8,193 bytes give
  `None`.
- **P7** Rights line: refuse an extra key, a missing key, a string `"true"`,
  `"sudo":"yes"` and 97 bytes.
- **P8** `classify_rights` is exhaustive over 2 × 2 × 3 × 4 inputs, compared
  with an independently written expected table in the test.

Mutations (each must fail a named test): the bridge keeps its own parser and
skips `arxd.service` (P1); leaf grammar allows leading zeros (P2); `events`
uses `any(l == "populated 0")` (P3, duplicate case); substring controller
match (P4); CapBnd compared as a subset (P6); `unknown` classified as ok (P8);
admin row reachable as ok with `no_new_privs = true` (P8).

## Slice E2: setup and janitor state machine (any OS, fake port)

`trait CgroupFs` has opaque `Dir` handles and these operations:
`proc_cgroup()`, `proc_status()`, `identity()`, `pid()`,
`open_root()`, `open_child(&Dir, name)`, `stat(&Dir) -> {uid, mode, magic}`,
`read(&Dir, file, max) -> bytes`, `write(&Dir, file, bytes)`,
`xattr_delegate(&Dir) -> Option<[u8; ≤2]>`, `mkdir_runs(&Dir) -> Created | Exists`,
`list(&Dir, max_entries) -> Vec<Entry{name, kind}>`,
`rmdir(&Dir, name)`, `open_ro(&Dir, file)`. A `Clock` port provides `now()`
and `sleep(5 ms)`. The driver is
`setup(&dyn CgroupFs, &Location, &Clock, &Cancellation) -> SetupOutcome`, run
under `catch_unwind`.

The `cfg(test)` fake models kernel semantics, not just return values:

- `cgroup.controllers` of a child equals the parent's `subtree_control`.
- A write is all or nothing; a token missing from `controllers` gives `EINVAL`.
- Enabling with resident processes gives `EBUSY`.
- `cgroup.kill` is recursive; populated clears after N polls, or never.
- `rmdir` gives `EBUSY` while populated or while child directories exist.
- Per-operation fault injection; symlink and `DT_UNKNOWN` entries.
- An operation log.

Tests (no real sleep; the clock is injected):

- **H1 fresh:** Ready. The log has exactly one unit write `+memory +pids`,
  `mkdir runs` at `0700`, a `runs/cgroup.kill` write, then a runs write
  `+memory +pids`. Evidence is `{0, true, true, true}`.
- **H2 prepared tree:** Ready, with `runs_created = false` and identical
  writes. It is idempotent.
- **H3 leftovers:** two populated leaves. The kill comes before any read of
  children; `rmdir` happens only after `populated 0`; `leaves_removed = 2`.
- **H4 kill never completes:** `KillTimeout`, no `rmdir`, cause `Janitor`.
- **H5 rmdir EBUSY on one leaf:** `LeafRemoval`.
- **H6 unknown or invalid name** next to valid leaves: `LeafStructure`, and
  **zero** `rmdir` calls. All children are checked before the first removal.
- **H7** A nested directory in a leaf gives `LeafStructure`.
- **H8** A symlink child, a non-cgroup2 magic, a non-zero UID or group write
  each give `LeafStructure`.
- **H9 bounds:** 65 child directories, 513 entries, and a 4,097-byte control
  file each fail at their step.
- **H10 anchors:** T1–T8 are each violated alone (non-root, wrong procfs
  magic, 4,097-byte cgroup file, foreign unit name, untrusted root, symlinked
  unit component, `user.delegate` missing, `"0"`, `"11"`, a threaded unit,
  unit processes present, missing controller, supervisor PIDs `{}`,
  `{other}`, `{self, other}`). Each gives its own fixed failure with **zero
  writes and zero mkdir** in the log.
- **H11** The unit write is rejected (`EINVAL`), or the re-read lacks
  `pids`: `EnableUnit`. No `-` token ever appears in any write.
- **H12** `runs` is threaded, non-root, group-writable or has resident
  processes: `Runs`.
- **H13** `runs/memory.swap.max` is absent: `Postcondition`.
- **H14 shutdown:** during the kill wait gives `Stopped` with no `rmdir`.
  Between S5 and S6 it gives `Stopped` with no `mkdir`.
- **H15 panic** injected in `rmdir` gives `Panicked`. The fixed fake literal
  appears at most once, from the default hook, and no marker from the fake's
  state appears (T10 pattern, re-executed subprocess).
- **H16 PID reuse:** a leftover named `arx-<own pid>-0` is removed and Ready.
- **H17 frozen leftover:** a `frozen 1` leaf with processes is killed and
  removed. The kernel kills frozen tasks with fatal signals; the fake models
  that.

Mutations: a write before the last anchor (H10); kill per leaf instead of
`runs/cgroup.kill` (H3 log); `rmdir` before `populated 0` (H3); unknown name
removed (H6); partial removal before checking all children (H6); deadline
ignored (H4); `Stopped` mapped to Ready (H14); `-memory` written (H11); S8
before S5 (H1 log, top-down); evidence carries a name (compile-time: field
types are integers or bools only, reviewed).

## Slice E3: Linux `CgroupFs` (Linux only)

- Production constructor: `LinuxFs::system()` with expected UID 0 and magics
  `0x9fa0` / `0x63677270`. A `cfg(test)` `LinuxFs::owned(uid)` skips only the
  magic and UID equality; the no-follow, bounds and listing logic is shared.
- Use only `openat` with `O_NOFOLLOW|O_CLOEXEC` (plus `O_DIRECTORY` for
  directories), `fstat`, `fstatfs`, `fgetxattr`, `mkdirat`, `unlinkat`,
  `getdents64` through `fdopendir` on a `dup`, and the exec helpers. Nothing
  path-based after the root.

Tests (owned temporary directories, ordinary user):

- **L1** Bounded read: a 4,097-byte file gives `EFBIG` through `text`; 4,096
  bytes passes.
- **L2** A final-component symlink (control file and child directory) is
  refused with `ELOOP`/`ENOTDIR`, and the target is untouched.
- **L3** A listing of 513 entries fails; 512 passes.
- **L4** Pure `entry_kind(d_type, fstatat_mode)` covers `DT_DIR`, `DT_REG`,
  `DT_LNK` and `DT_UNKNOWN` with a directory or a symlink.
- **L5** Constant test: `system()` uses UID 0 and both magics.
- **L6** Source scan (P-SCAN tokenizer) of `hierarchy/linux.rs` and
  `hierarchy.rs`: no `read_dir`, `File::open`, `canonicalize`, `metadata(`,
  `exists(`, `kill(`, `killpg(`, `Command`, or `set_hook`.
- **L7** `user.delegate` reader on an owned directory after
  `setxattr("user.delegate", "1")`. If the filesystem refuses user xattrs, the
  test prints an explicit skip marker and the evidence must say "skipped",
  never "passed".
- **L8** An unprivileged end-to-end `setup(LinuxFs::system(), …)` returns
  `Identity` with zero writes. It reads only the test process's own `status`.
  Skip when euid is 0; CI must never run it as root.

Mutations: `O_NOFOLLOW` dropped (L2); `take(4097)` raised (L1); listing bound
removed (L3); `system()` built with the test UID (L5).

## Slice E4: shim rights line (Unix)

- `tool-probe` appends the ADR 0020 R5 line after the `TOOLS` lines. The
  shim's existing no-op hook, `catch_unwind`, euid-0 refusal and `/` cwd are
  unchanged.
- `cfg(test)` seams: the candidate list, the expected owner UID, and the
  deadline (default 5 s; tests use 200 ms).

Tests (owned temporary directories, current user):

- **S6 (updated)** `TOOLS` order, then exactly one rights line, last. Total
  under 4,096 bytes. The cwd is `/`.
- **S8** Fake `sudo` scripts as candidates: exit 0 gives `effective`; exit 3
  gives `refused`; a 10 s sleeper gives `unknown` after the seam deadline and
  is reaped (no live PID afterwards); self-SIGKILL gives `unknown`; no
  candidate gives `absent`.
- **S9** A group-writable candidate is skipped and the next one used. A
  final-component symlink candidate is skipped. A candidate owned by another
  UID (fixed via the seam) is skipped.
- **S10** `PATH` holds a directory with a fake `sudo` that exits 0, and the
  candidate seam is empty: result `absent`. `PATH` is never consulted.
- **S11 (Linux)** The shim spawned with `prctl(PR_SET_NO_NEW_PRIVS)` in
  `pre_exec` (unprivileged) gives `no_new_privs: true`. Without it the result
  is `false`; if the test process already has NNP, print an explicit skip
  marker. On macOS the result is `null`.
- **S12** Constant test: production candidates are exactly the three ADR 0020
  paths, the deadline is 5 s, and stdin, stdout and stderr are null.

Mutations: `PATH` search for `sudo` (S10); ownership check dropped (S9);
timeout returns `refused` (S8); rights line printed before tools (S6);
`no_new_privs` hard-coded `true` (S11).

## Slice E5: bridge readiness and latches (any OS)

- `Readiness`: `Arc` around one `AtomicU8`, with `set_ready()`,
  `fail(cause)` and `latch(cause)` (each `compare_exchange`, first writer
  wins), and `check() -> Result<(), ExecutionUnavailable>` using the ADR 0020
  D5 table.
- `UnixBridge::new` (still `cfg(test)`) takes a `Readiness` instead of
  `ready: bool`.
- `body`: on `Err(NotStarted::Containment | NotStarted::Resource)`, call
  `latch(Containment)` before mapping. On a discovery `Ok`, parse the table
  and the rights line. An invalid table gives empty tools (row K, unchanged).
  A valid table with a failing `classify_rights` calls `latch(Rights)` and
  then returns the tools.
- P-SCAN: add `exec/hierarchy.rs`, `exec/hierarchy/*.rs` (production parts)
  and `readiness.rs` to the scanned set with a reviewed inventory. Add
  `kill(`, `killpg(` and `setpriority(` to the denylist for the hierarchy
  files, with a self-test.

Tests, through real `dispatch` with a counting fake runner port:

- **T6 (updated)** `TOOLS` lines without a rights line give K and no tools.
  With a valid rights line, N with tools. With two rights lines, K.
- **T11** First request `Containment` gives row D. The second request is
  refused at preflight as `mechanism_unavailable` with **zero** port calls.
- **T12** `Resource` gives row F, then refused, zero calls.
- **T13** `Cancelled` gives row E. The next request reaches the port (no
  latch).
- **T14** A discovery with `sudo: effective` gives row N with tools. The next
  `run_command` is refused `rights_mismatch` with zero port calls, and
  `list_activity` shows `preflight_failed`.
- **T15** Each `SetupFailure` gives its preflight token and stored reason.
  The table is driven by an exhaustive `match` with no wildcard.
- **T16** `Pending` gives `mechanism_unavailable`. `Ready` then `fail` is
  ignored (monotone); a second `set_ready` after a latch is ignored.
  Concurrent `latch` from 8 threads keeps the first cause.
- **T17** The rights line text, `no_new_privs` and `sudo` are absent from
  responses, rows and database bytes.
- **T9 (extended)** Production selection stays inert (see E6 pins).

Mutations: latch removed (T11); latch on `Cancelled` (T13); rights failure
withholds tools (T14 expects N); `Janitor` mapped to `mechanism_unavailable`
(T15); non-monotone `store` (T16); rights line released in `tools` (T17).

## Slice E6: startup wiring, daemon row, tool cache (any OS)

- `store::Activity::start_daemon_discovery()` writes daemon actor, principal
  `arxd`, `discover_tools`, `dedicated_account`. There is no schema change.
- A `cfg(test)` `Hooks` implementation provides `Location`, a fake
  `CgroupFs` and the bridge. `start_discovery` spawns one tracked async task:
  `blocking(setup)` → if Ready `blocking(start_daemon_discovery)` →
  `execution.dispatch(None, activity, id)` → cache update on `succeeded`.
- `drain` awaits `Execution::drain`, then the task handle, then
  `Blocking::drain`.
- The tool cache is `Mutex<Option<ToolCache { tools: Value,
  observed_identity: Value, observed_at_ms: u64 }>>` on `Activity`. It is
  written only from the dispatch result value of an outcome of `succeeded`.
  `list_hosts` reads it.

Tests:

- **W1 order:** the log shows setup steps, then Ready, then the row insert,
  then the port call.
- **W2** A setup failure writes zero daemon rows and makes zero port calls.
- **W3** Shutdown during the janitor wait gives `Stopped`, zero rows, and
  drain returns within the budget (paused tokio time).
- **W4** The daemon row reads back with actor `daemon` and principal `arxd`,
  and is invisible to an agent's `list_activity`.
- **W5** The cache changes only on row N. Rows K, M, G and refusals keep the
  previous value. An update replaces the whole value.
- **W6** `list_hosts` shows the cached `tools`, `tools_observed_at` and
  `last_observed_identity` equal to the response's `observed_identity`.
  Without a cache, the host object is byte-identical to the `fdea1c6` golden.
- **W7** An agent `discover_tools` row N refreshes the cache.
- **W8 production pins** (source tests): `Execution::configured` contains
  `Self::native(` and not `UnixBridge`; `impl Hooks for Native` has no
  `start_discovery` override and `ServiceApplication::start_discovery`
  spawns nothing outside `cfg(test)`; `capability()` keeps `"available":false`
  and `"service_hardening":null`; no non-test module references `hierarchy::`
  or `start_daemon_discovery`.
- **W9** A stuck fake discovery is bounded by the 10 s drain with the existing
  failure semantics.

Mutations: discovery dispatched before Ready (W1); a row written on setup
failure (W2); cache updated on row K (W5); cache built from `Evidence`
instead of the response metadata (W6, mismatched marker); production
`start_discovery` wired (W8).

## Hosted compile and CI requirements (every slice)

- `cargo fmt --check`, plus `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and nextest on hosted Windows, macOS and Ubuntu. Linux-only
  code compiles and runs only on Ubuntu; pure and fake tests run on all three.
- No test sleeps longer than 1 s. Deadlines are seams with production-constant
  tests. This protects the 15-minute `ci-time-budget`. No CI change is part of
  this work.
- No test runs as root, writes under `/sys`, `/proc` or `/etc`, creates
  accounts or touches a service manager. Tests that would be unsafe as root
  skip when euid is 0, with an explicit marker.
- Evidence must name the exact command, head and runner OS. A Windows-only
  local run proves nothing about Linux code paths and must say so.

## Privileged acceptance (separate; Stage F scope; NOT authorized here)

To be designed and reviewed only after the `E_PROTECTION` diagnosis, on a
disposable Linux host (systemd ≥ 254, kernel ≥ 5.14, cgroup v2), never the
owner's machine:

- **A1** The unit cgroup layout at start: `supervisor/` holds only the main
  PID, `user.delegate = 1`, and `memory pids` are available in controllers.
- **A2** After setup: unit and `runs/` `subtree_control` include
  `memory pids`; `runs/` is root-owned `0700`; S9 files exist.
- **A3** R4 hardening reads `standard` with the drop-in and `none` without it.
- **A4** The janitor kills a leftover populated leaf (planted by a prior
  instance whose main process received SIGKILL), removes it, and execution
  becomes available only afterwards.
- **A5** An orphan empty leaf present at start is removed.
- **A6** A janitor failure surfaces as `quarantined`; induce it with a leaf
  that is not removable, if a safe induction exists.
- **A7** R5 with an installed `sudo` and a NOPASSWD rule for the build
  account: under the runner's NNP the result is `refused`. Without the rule,
  also `refused`. Neither is `effective`.
- **A8** Startup discovery runs after Ready and writes one daemon row.
- **A9** Whether systemd removes delegated children at unit stop (U9).
- **A10** That cgroupfs writes and migration need no capability beyond the
  drop-in's bounding set (ADR 0019/0020 inference).

Until then, every item stays **unverified on real hardware**.
