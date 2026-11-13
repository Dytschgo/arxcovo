# Brief: Linux execution hierarchy, janitor, startup discovery and rights (ADR 0020)

Status: **accepted inactive staged contract**; see ADR 0020's current
coordinator disposition. The historical checkpoint below records the earlier
proposal, not the current acceptance state. Tests here remain requirements,
not evidence. Stage C PR53 merged on 2026-11-13 as `6dd7467` and Stage D
PR55 as `f91ae5a`; both prerequisite merge gates are complete. Local inactive
implementation may proceed after design review. Publication/integration still
requires this contract PR's merge, an exact re-pin and independent integration
review on that merged base.

## Historical proposal and correction-1 checkpoint

Status: **proposal, correction 1 of 3** (Anthropic / claude-opus-5-5 / high,
design sub-agent, 2026-11-12, correction base `d44035d`, code identical to
`fdea1c6`). It is the implementation contract for
[ADR 0020](../adr/0020-linux-execution-hierarchy-and-janitor.md), which is
**proposed, not accepted**. This revision answers the independent
[Sol review](../reviews/0020-sol-independent-design-review.md); the
[correction disposition](../reviews/0020-correction1-disposition.md) maps each
finding. Nothing here was built, run or tested. Every test, mutation and
acceptance item below is a requirement, not evidence. Current tests are not
evidence of this plan.

## Gates for local work and later integration

1. Local inactive work requires the accepted design and independent review:
   original Opus architecture/correction 1 retain Sol high review, and Sol
   correction 2/publication retain Opus high review. Declare exact reviewed
   dependency pins. Reviewer fallback remains grok-4.7 high.
2. Stage C PR53 (`6dd7467`) and Stage D PR55 (`f91ae5a`) merged on
   2026-11-13 through all required gates. Before implementation publication
   or integration, this contract PR must also merge through all gates.
   Re-pin and re-review the implementation on that merged base. This follows
   the user's product-first direction; it permits local work during CI waits
   and waives no merge check. Slices E4-E7 change prerequisite code.
3. No slice may activate anything. Production stays `MechanismUnavailable`
   (pins in E7).

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
| E4 | shim rights line with bound-executable `sudo` probe | `unix_shim.rs` (+ its test file) | Linux (macOS: `unknown` only) |
| E5 | runner cleanup-residue signal (ADR 0020 A5) | `exec/cgroup.rs`, `exec/mod.rs`, private `exec/unix.rs` flag plumbing | Linux (pure parts all) |
| E6 | bridge readiness, in-slot gate and latches | `service/unix_bridge.rs`, `service/unix_bridge/readiness.rs` (new), `service/executor.rs` (one `cfg(test)` barrier seam only) | all |
| E7 | startup wiring, daemon row, tool cache | `service/application.rs`, `store.rs`, `mcp.rs` | all |

E1 → E2 → E3 is a chain. E4 needs only E1. E5 is independent. E6 needs E1,
E2, E4 and E5. E7 needs E6. At most two builders run at a time.

## Slice E1: pure parsers and classifiers (any OS)

APIs. All are pure. Inputs are bytes, and nothing panics on any input.

- `locate(&[u8]) -> Option<Location>`: the Stage D `cgroup_parent` rules plus
  ADR 0020 T3 (the component before `supervisor` is exactly `arxd.service`).
  `Location::{unit_components(), runs()}`. The bridge's `cgroup_parent` is
  removed; the bridge calls `locate(bytes).map(|l| l.runs())`.
- `leaf_name(&[u8]) -> bool`: `arx-<pid>-<serial>`, canonical decimals with
  no leading zero, at most 10 and 20 digits.
- `events(&str) -> Option<bool>`: every line is `<key> <decimal>` with key
  `[a-z_]+`, no duplicate key, and exactly one `populated` line, `0` or `1`.
  Other well-formed keys are ignored. It returns populated. Every caller
  (janitor wait, leaf check, verify, S9) uses this one parser and requires
  `Some(false)` where the ADR says populated `false`.
- `classify_entry(name, d_type, fstatat_mode: Option<Result<mode>>) -> Entry`
  with `Entry::{Dot, Child, Control, Refused}` per ADR 0020 D4.3: `.`/`..` →
  `Dot`; directory → `Child`; regular file named
  `[a-z0-9_]+(\.[a-z0-9_]+)+` (≤ 64 bytes) → `Control`; anything else,
  including any symlink, device, FIFO, socket or a failed `fstatat` for
  `DT_UNKNOWN` → `Refused`.
- `controllers(&str) -> Controllers`: exact whitespace-split tokens.
  `has(memory) && has(pids)`.
- `pids(&str) -> Option<Vec<u32>>`: newline-separated canonical decimals at
  most 4,096 bytes long. The empty string means none.
- `hardening(&[u8]) -> Hardening` (`Standard | None`): at most 8,192 bytes;
  exactly one `NoNewPrivs:\t1` and exactly one `CapBnd:\t00000000000000c1`
  (16 lowercase hex digits). Anything else is `None`.
- `bin_alias(&[u8]) -> bool`: exactly `usr/bin` or `/usr/bin`, at most 16
  bytes.
- `rights_line(&str) -> Option<RightsFact>`: exactly
  `{"rights":{"no_new_privs":…,"sudo":…}}` with exactly those keys and types,
  `sudo` one of `absent`, `untrusted`, `exit_nonzero`, `exit_zero`,
  `unknown`, and at most 96 bytes.
- `classify_rights(Rights, Hardening, RightsFact) -> Result<(), ExecutionUnavailable>`:
  the ADR 0020 D8 table.

Tests:

- **P1** Stage D T2 is **amended**, not preserved wholesale. Its accepted
  fixture (`0::/system.slice/arxd.service/supervisor\n`) keeps the same
  `runs` path, and every refused fixture stays refused. The A1 tightening
  intentionally refuses inputs Stage D accepted with another unit component;
  T2 gains them as explicit refusal fixtures: `0::/a/supervisor`,
  `0::/system.slice/foo.service/supervisor`,
  `0::/arxd.service/x/supervisor`, `0::/arxd.service.d/supervisor`. New
  accepted fixtures: `0::/arxd.service/supervisor` (no slice) and a
  three-level slice. A source pin asserts that exactly one parser exists
  (no `fn cgroup_parent` remains in the bridge).
- **P2** Leaf grammar: accept `arx-0-0` and `arx-4194304-18446744073709551615`.
  Refuse `arx-01-2`, `arx-1-02`, `arx--1`, `arx-1-`, `arx-1-2-3`,
  `ARX-1-2`, `arx-+1-2`, an 11-digit PID, a 21-digit serial, and a trailing
  newline.
- **P3** Events: `populated 0\nfrozen 0\n` gives `false`;
  `populated 1\nfrozen 1\n` gives `true`; `frozen 0\npopulated 0\n` (order)
  gives `false`; `populated 0\nfuture_key 7\n` gives `false`. Refuse a
  duplicate `populated`, a duplicate other key, a missing `populated`,
  `populated 2`, `Populated 0`, `frozen-x 0`, a non-decimal value, CRLF and
  4,097 bytes.
- **P4** Controllers: `memoryx pids` and `memory` alone fail.
  `cpu memory pids\n` passes.
- **P5** PIDs: refuse a leading zero, `+1`, an embedded space and over
  4,096 bytes. The empty string means none.
- **P6** Hardening: `c1` exact passes. `c0`, `1c1`, uppercase `C1`,
  `NoNewPrivs:\t0`, duplicate lines, a missing line and 8,193 bytes give
  `None`.
- **P7** Rights line: each of the five `sudo` values parses. Refuse an extra
  key, a missing key, a string `"true"`, the retired values `"effective"`
  and `"refused"`, `"sudo":"yes"` and 97 bytes.
- **P8** `classify_rights` is exhaustive over 2 × 2 × 3 × 5 inputs, compared
  with an independently written expected table in the test.
- **P9** `classify_entry`: `.` and `..` → `Dot`; `DT_DIR` → `Child`;
  `cgroup.events`, `memory.swap.max`, `cgroup.pressure` → `Control`;
  `arx-1-2` as a regular file, `Memory.max`, `.hidden`, `nodot`, a 65-byte
  control-like name → `Refused`; `DT_LNK`, `DT_CHR`, `DT_BLK`, `DT_FIFO`,
  `DT_SOCK` → `Refused`; `DT_UNKNOWN` with a directory mode → `Child`, with
  a symlink mode → `Refused`, with an `fstatat` error → `Refused`.
- **P10** `bin_alias`: accept `usr/bin` and `/usr/bin`. Refuse `usr/bin/`,
  `./usr/bin`, `usr//bin`, `/usr/local/bin`, `../usr/bin`, empty and
  17 bytes.

Mutations (each must fail a named test): the bridge keeps its own parser and
skips `arxd.service` (P1 refusal fixtures and source pin); leaf grammar
allows leading zeros (P2); `events` uses `any(l == "populated 0")` or
whole-file equality with `populated 0\n` (P3, duplicate and `frozen` cases);
substring controller match (P4); CapBnd compared as a subset (P6); symlinks
classified as `Child` or `Control` (P9); `bin_alias` uses `ends_with`
(P10); `unknown` or `untrusted` classified as ok (P8); admin row reachable as
ok with `no_new_privs = true` (P8).

## Slice E2: setup and janitor state machine (any OS, fake port)

`trait CgroupFs` has opaque `Dir` handles and these operations:
`proc_cgroup()`, `proc_status()`, `identity()`, `pid()`,
`open_root()`, `open_child(&Dir, name)`, `stat(&Dir) -> {uid, mode, magic}`,
`read(&Dir, file, max) -> bytes`, `write(&Dir, file, bytes)`,
`xattr_delegate(&Dir) -> Option<[u8; ≤2]>`, `mkdir_runs(&Dir) -> Created | Exists`,
`list(&Dir, max_records) -> Vec<RawEntry{name, d_type, fstatat_mode}>`
(each call is a complete listing from the beginning),
`rmdir(&Dir, name)`, `open_ro(&Dir, file)`. A `Clock` port provides `now()`
and `sleep(5 ms)`; tests use a manual clock (no tokio time pausing, which
does not advance a blocking std clock). The driver is
`setup(&dyn CgroupFs, &Location, &Clock, &Cancellation) -> SetupOutcome`, run
under `catch_unwind`.

The `cfg(test)` fake models kernel semantics, not just return values:

- `cgroup.controllers` of a child equals the parent's `subtree_control`.
- A write is all or nothing; a token missing from `controllers` gives `EINVAL`.
- Enabling with resident processes gives `EBUSY`.
- `cgroup.kill` is recursive; populated clears after N polls, or never.
- `cgroup.events` is realistic multi-key (`populated` and `frozen`).
- `rmdir` gives `EBUSY` while populated or while child directories exist.
- Directories list their kernel control files, `.` and `..`.
- Per-operation fault injection; symlink, device, FIFO, socket and
  `DT_UNKNOWN` entries; a hook that mutates state between two named
  operations.
- An operation log.

The fake cannot prove the OS listing property; E3 L9 does.

Tests (no real sleep; the clock is injected):

- **H1 fresh:** Ready. The log has exactly one unit write `+memory +pids`,
  `mkdir runs` at `0700`, a `runs/cgroup.kill` write, then a runs write
  `+memory +pids`. Evidence is `{0, true, true, true}`. Events files are
  multi-key throughout.
- **H2 prepared tree:** Ready, with `runs_created = false` and identical
  writes. It is idempotent.
- **H3 leftovers:** two populated leaves. The kill comes before any listing;
  `rmdir` happens only after populated `false`; `leaves_removed = 2`.
- **H4 kill never completes:** `KillTimeout`, no `rmdir`, cause `Janitor`.
- **H5 rmdir EBUSY on one leaf:** `LeafRemoval`.
- **H6 unknown or invalid name** next to valid leaves: `LeafStructure`, and
  **zero** `rmdir` calls. All children are checked before the first removal.
  The log still shows the `runs/cgroup.kill` write first (processes of
  unknown children are killed).
- **H7** A nested directory in a leaf gives `LeafStructure`.
- **H8** In `runs/` and, separately, in a leaf: a symlink, a character
  device, a FIFO, a socket, a regular file named like a leaf, and a
  `DT_UNKNOWN` entry whose `fstatat` fails each give `LeafStructure` with
  zero `rmdir`. A non-cgroup2 magic, a non-zero UID or group write on a child
  each give `LeafStructure`.
- **H9 bounds:** 65 child directories fail. 512 raw records (510 control
  files plus `.` and `..`) pass and 513 fail. A 4,097-byte control file fails
  at its step.
- **H10 anchors:** with the fake reporting hardening `Standard`, T1–T8 are
  each violated alone (non-root, wrong procfs magic, 4,097-byte cgroup file,
  foreign unit name, untrusted root, symlinked unit component, substituted
  handle of a different magic or owner, `user.delegate` missing, `"0"`,
  `"11"`, a threaded unit, unit processes present, missing controller,
  supervisor PIDs `{}`, `{other}`, `{self, other}`). Each gives its own fixed
  failure with **zero writes and zero mkdir** in the log.
- **H10b precedence:** hardening `None` together with non-root gives
  `Hardening` (S0 before T1), with zero writes.
- **H11** The unit write is rejected (`EINVAL`), or the re-read lacks
  `pids`: `EnableUnit`. No `-` token ever appears in any write.
- **H12** `runs` is threaded, non-root, group-writable or has resident
  processes: `Runs`.
- **H13** `runs/memory.swap.max` is absent: `Postcondition`.
- **H14 shutdown:** during the kill wait gives `Stopped` with no `rmdir`.
  Between S5 and S6 it gives `Stopped` with no `mkdir`. Between S9 and S10
  it gives `Stopped` and readiness never becomes `Ready`.
- **H15 panic** injected in `rmdir` gives `Panicked`. The fixed fake literal
  appears at most once, from the default hook, and no marker from the fake's
  state appears (T10 pattern, re-executed subprocess).
- **H16 PID reuse:** a leftover named `arx-<own pid>-0` is removed and Ready.
- **H17 frozen leftover:** a `frozen 1` leaf with processes is killed and
  removed. The kernel kills frozen tasks with fatal signals; the fake models
  that.
- **H18 verify leftover:** the hook adds a valid-looking leaf after the last
  `rmdir` and before the verify listing: `VerifyFailed`, cause `Janitor`
  (`quarantined`).
- **H19 verify populated:** populated becomes `true` before verify:
  `VerifyFailed`.
- **H20 S9:** multi-key events with `frozen 0` pass; a child directory or a
  refused entry present at S9 gives `Postcondition`.

Mutations: a write before the last anchor (H10); kill per leaf instead of
`runs/cgroup.kill` (H3 log); `rmdir` before populated `false` (H3); unknown
name removed (H6); partial removal before checking all children (H6);
symlink entries ignored (H8); `.`/`..` excluded from the record budget (H9);
verify skipped or reusing the first listing (H18); deadline ignored (H4);
`Stopped` mapped to Ready (H14); `-memory` written (H11); S8 before S5 (H1
log, top-down); identity checked before hardening (H10b); evidence carries a
name (compile-time: field types are integers or bools only, reviewed).

## Slice E3: Linux `CgroupFs` (Linux only)

- Production constructor: `LinuxFs::system()` with expected UID 0 and magics
  `0x9fa0` / `0x63677270`. A `cfg(test)` `LinuxFs::owned(uid)` skips only the
  magic and UID equality; the no-follow, bounds and listing logic is shared.
- Use only `openat` with `O_NOFOLLOW|O_CLOEXEC` (plus `O_DIRECTORY` for
  directories), `fstat`, `fstatat(AT_SYMLINK_NOFOLLOW)`, `fstatfs`,
  `fgetxattr`, `mkdirat`, `unlinkat`, and the exec helpers. Nothing
  path-based after the root.
- **Listing.** Each `list` call opens a new open file description with
  `openat(dir, ".", O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)`, reads it
  from offset 0 to end (`fdopendir` + `readdir`, or `getdents64`), and closes
  it. Never `dup`, `dup2`, `F_DUPFD`, `fdopendir` on the handle itself, or a
  cached stream. A `cfg(test)` observer receives each listing descriptor.

Tests (owned temporary directories, ordinary user):

- **L1** Bounded read: a 4,097-byte file gives `EFBIG` through `text`; 4,096
  bytes passes.
- **L2** A final-component symlink (control file and child directory) is
  refused with `ELOOP`/`ENOTDIR`, and the target is untouched.
- **L3** A listing of 513 raw records fails; 512 passes (counting `.` and
  `..`).
- **L4** Pure `entry_kind(d_type, fstatat_mode)` covers `DT_DIR`, `DT_REG`,
  `DT_LNK` and `DT_UNKNOWN` with a directory or a symlink.
- **L5** Constant test: `system()` uses UID 0 and both magics.
- **L6** Source scan (P-SCAN tokenizer) of `hierarchy/linux.rs` and
  `hierarchy.rs`: no `read_dir`, `File::open`, `canonicalize`, `metadata(`,
  `exists(`, `kill(`, `killpg(`, `Command`, `set_hook`, `dup(`, `dup2(` or
  `F_DUPFD`.
- **L7** `user.delegate` reader on an owned directory after
  `setxattr("user.delegate", "1")`. If the filesystem refuses user xattrs, the
  test prints an explicit skip marker and the evidence must say "skipped",
  never "passed".
- **L8** An unprivileged end-to-end `setup(LinuxFs::system(), …)` behind a
  counting decorator returns **`Hardening`** with zero writes and zero mkdir,
  because S0 runs before T1. It reads only the test process's own
  `/proc/self/status`. Precondition: the test parses that file with the pure
  `hardening` parser; if it is unexpectedly `Standard`, the test prints an
  explicit skip marker (never "passed"). `Identity` is isolated by H10, not
  here. Skip when euid is 0; CI must never run it as root, and no test
  environment is made privileged to satisfy it.
- **L9 fresh descriptions (real OS):** open one directory handle with three
  entries. List it twice: both results contain all three. Create a fourth
  entry, list again: it is present. Remove one, list again: it is absent.
  The observer asserts that every listing descriptor differs from the handle
  descriptor and has `FD_CLOEXEC`, and that the handle's own offset
  (`lseek(handle, 0, SEEK_CUR)`) is still 0.
- **L10 real entry kinds:** in an owned directory create a directory, a
  regular file `cgroup.events`, a regular file `arx-1-2`, a symlink, a FIFO
  (`mkfifo`) and a bound Unix socket. The real listing classifies them as
  `Child`, `Control`, `Refused`, `Refused`, `Refused`, `Refused`, and the
  symlink target is untouched.

Mutations: `O_NOFOLLOW` dropped (L2); `take(4097)` raised (L1); listing bound
removed (L3); listing through `dup` of the handle (L9 second and later
listings, offset and `FD_CLOEXEC` assertions); listing descriptor without
`O_CLOEXEC` (L9); symlink accepted (L10); `system()` built with the test UID
(L5).

## Slice E4: shim rights line with the bound-executable probe (Unix)

- `tool-probe` appends the ADR 0020 R5 line after the `TOOLS` lines. The
  shim's existing no-op hook, `catch_unwind`, euid-0 refusal and `/` cwd are
  unchanged.
- The `sudo` value comes from the ADR 0020 D8 bound-executable probe: anchor
  `/`, candidates C1 `usr/bin/sudo`, C2 `usr/local/bin/sudo`, C3 `bin/sudo`,
  per-component `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` + `fstat` (UID 0,
  no group/other write), the exact `bin` alias rule, final
  `openat(O_PATH|O_NOFOLLOW|O_CLOEXEC)` + `fstat` (regular, UID 0, no
  group/other write), all candidates classified before any execution, then
  `fork` + `execveat(fd, "", ["sudo","-n","true"], [], AT_EMPTY_PATH)` with
  stdio on `/dev/null`, cwd `/`, a close-on-exec error pipe, a 5 s polling
  deadline, SIGKILL of the own child and reap. On non-Linux no probe runs and
  `sudo` is `unknown`.
- `cfg(test)` seams: the anchor directory (an owned temporary root), the
  trusted owner UID (the test UID), the deadline (default 5 s; tests use
  200 ms), the argv and environment handed to fakes, and a hook that runs
  between validation and `fork`. Production values are pinned by S12.
- **Fakes are ELF executables:** copies of the test executable re-entered
  through one fixed helper test (the existing T10 re-execution pattern),
  selecting a behaviour (exit 0, exit 3, sleep 10 s, self-SIGKILL, write a
  marker) and a per-fake marker path through the argv/environment seams. A
  marker file proves which object actually ran. Interpreter scripts are used
  only to show that they do not run.

Tests (owned temporary directories, current user, Linux unless noted):

- **S6 (updated)** `TOOLS` order, then exactly one rights line, last. Total
  under 4,096 bytes. The cwd is `/`.
- **S8 outcomes:** a trusted fake exiting 0 gives `exit_zero` and writes its
  marker once; exit 3 gives `exit_nonzero`; the 10 s sleeper gives `unknown`
  after the seam deadline and is reaped (no live PID afterwards);
  self-SIGKILL gives `unknown`; no candidate gives `absent`; a trusted
  candidate that is a `#!` script gives `unknown` (close-on-exec `execveat`
  fails with `ENOENT`) and its marker is absent; a candidate without execute
  permission gives `unknown`.
- **S9 untrusted, nothing runs:** each case gives `untrusted`, and **no
  marker exists**, including the marker of a valid trusted candidate later in
  the order: (a) final file group-writable; (b) final file owned by another
  UID (fixed via the owner seam); (c) final symlink to a trusted fake;
  (d) an intermediate directory (`usr` or `usr/local/bin`) group- or
  other-writable, i.e. replaceable by a non-owner; (e) an intermediate
  symlink component (`usr` → an owned trusted copy of the tree); (f) `bin` a
  symlink to anything other than exactly `usr/bin` or `/usr/bin`; (g) the
  anchor itself group-writable.
- **S9b alias:** `bin` → `usr/bin`, owned by the trusted owner: C3 is
  dropped, C1 runs, and C1's marker is written exactly once. The same link
  owned by another UID gives `untrusted` with no marker.
- **S9c check-to-exec replacement:** with a trusted C1, the hook between
  validation and `fork` (i) renames a marker-writing replacement over
  `usr/bin/sudo`, and, in a second case, (ii) renames a replacement directory
  over `usr/bin`. In both, the originally validated object's marker is
  written and the replacement's marker is absent.
- **S10** `PATH` holds a directory with a fake `sudo` that exits 0, and no
  candidate exists under the anchor: result `absent`, no marker. `PATH` is
  never consulted.
- **S11 (Linux)** The shim spawned with `prctl(PR_SET_NO_NEW_PRIVS)` in
  `pre_exec` (unprivileged) gives `no_new_privs: true`. Without it the result
  is `false`; if the test process already has NNP, print an explicit skip
  marker. On macOS the result is `null` and `sudo` is `unknown` with no
  spawn.
- **S12** Constant test: production anchor `/`, owner UID 0, the three
  candidate component lists in order, the two alias targets, argv
  `["sudo","-n","true"]`, an empty environment, deadline 5 s, stdio
  `/dev/null`, cwd `/`.
- **S13** Source scan of the probe code in `unix_shim.rs`: the probe
  executes only through `execveat` on a descriptor; no `Command::new`,
  `execv`, `execvp` or `posix_spawn` with a `sudo` path; no `PATH` read.

Mutations: exec by path after validation (S9c); ancestor check dropped (S9 d,
e); `O_NOFOLLOW` dropped on the final open (S9 c); alias accepts any link
target or `ends_with("usr/bin")` (S9 f); execution continues after an
untrusted candidate (S9 markers); `O_CLOEXEC` dropped on the final
descriptor (S8 script case runs and writes its marker); environment
inherited (S12); `PATH` search for `sudo` (S10); timeout returns
`exit_nonzero` (S8); rights line printed before tools (S6); `no_new_privs`
hard-coded `true` (S11).

## Slice E5: runner cleanup-residue signal (ADR 0020 A5)

- Pass only the private flag clone through
  `Runner::run_core -> unix::run -> Group::create`, Linux-gated as needed.
  No static or config-carried flag. Preserve all behavioral ordering,
  cancellation/fence checks, legacy/phased results and each branch's existing
  errno-read/cleanup order (including failed-openat reading after unlinkat). Include `exec/unix.rs` in this slice's manifest.
- `Runner` owns `residue: Arc<AtomicBool>` and exposes
  `cleanup_residue(&self) -> bool` (acquire). `Group::create` takes a clone.
  One private helper `release_leaf(parent, name, residue)` does the
  `unlinkat(AT_REMOVEDIR)` and stores `true` (release) unless it returned 0.
  Both create-path sites use it. `Drop` stores `true` when
  `kill_and_confirm()` returns `false`.
- No change to `RunError`, `NotStarted`, `RunResult`, `Outcome`, legacy
  `run`, `ExecError` text or any existing test expectation.

Tests (Linux owned temporary directories, the existing `cgroup.rs` test
pattern; never a real cgroup or privileged write):

- **R1** A `Group` over temporary directories whose leaf cannot be removed
  (non-empty): after `drop`, the residue flag is `true`. With `removed`
  already set, `drop` leaves it `false`.
- **R2** `release_leaf` on a non-empty directory sets the flag; on an empty
  directory it removes it and leaves the flag clear. A source pin asserts
  that `cgroup.rs` has no `unlinkat` call outside `release_leaf` and
  `kill_and_confirm`.
- **R3** Two isolated test domains, each with a distinct owned subtree, have
  independent runner flags; a new runner starts `false`. Service-domain
  replacement or a second runner for one subtree is forbidden (D6/T20).
- **R4** Every existing Stage B runner test passes with no edited
  expectation (the diff adds tests only).

Mutations: `Drop` ignores the result (R1); a create site keeps the bare
`unlinkat` (R2 pin); a process-global static flag (R3).

## Slice E6: bridge readiness, in-slot gate and latches (any OS)

- `Readiness`: `Arc` around one `AtomicU8`, with `set_ready()`,
  `fail(cause)` and `latch(cause)` (each `compare_exchange`, first writer
  wins), and `check() -> Result<(), ExecutionUnavailable>` using the ADR 0020
  D5 table.
- `UnixBridge::new` (still `cfg(test)`) takes a `Readiness` instead of
  `ready: bool`. `RunnerPort` gains `cleanup_residue()`; the fake port
  controls it.
- **In-slot gate** at the start of `body`: if `cleanup_residue()`,
  `latch(Containment)`; then, if `check()` fails, return
  `Report::Unavailable(token)` with **zero** port calls.
- After the port returns and before `body` returns: `latch(Containment)` if
  `cleanup_residue()` or the error is `NotStarted::Containment |
  NotStarted::Resource`. The report is unchanged. On a discovery `Ok`, parse
  the table and the rights line. An invalid table gives empty tools (row K,
  unchanged). A valid table with a failing `classify_rights` calls
  `latch(Rights)` and then returns the tools (row N, deliberate).
- `executor.rs` gains exactly one `cfg(test)` barrier seam between
  `preflight` and `active.fetch_add`; production code paths are unchanged.
- P-SCAN: per ADR 0020 D10, add `exec/hierarchy.rs`,
  `exec/hierarchy/{parse,setup,linux}.rs` and `readiness.rs` to the scanned
  set with a reviewed inventory, assert `fake.rs` is excluded by name, and
  add `kill(`, `killpg(` and `setpriority(` to the hierarchy denylist, each
  with a mutant self-test.

Tests, through real `dispatch` with a counting fake runner port:

- **T6 (updated)** `TOOLS` lines without a rights line give K and no tools.
  With a valid rights line, N with tools. With two rights lines, K.
- **T11** First request `Containment` gives its unchanged Stage D report
  (row D). After it completes, the next request is refused as
  `mechanism_unavailable` with **zero** port calls.
- **T12** `Resource` gives row F, then refused, zero calls.
- **T13** `Cancelled` with the residue flag clear gives row E, six times in
  a row: every request reaches the port (no latch, no artificial cap).
- **T13b** `Cancelled` with the residue flag set by the fake gives the
  unchanged row E; the next request is refused `mechanism_unavailable` /
  `preflight_failed` with zero port calls.
- **T14** A discovery with `sudo: exit_zero` gives row N with tools. The next
  `run_command` is refused `rights_mismatch` with zero port calls, and
  `list_activity` shows `preflight_failed`. Same for `untrusted` and
  `unknown`. `absent` and `exit_nonzero` do not latch.
- **T15** Each `SetupFailure` gives its refusal token and stored reason.
  The table is driven by an exhaustive `match` with no wildcard.
- **T16** `Pending` gives `mechanism_unavailable`. `Ready` then `fail` is
  ignored (monotone); a second `set_ready` after a latch is ignored.
  Concurrent `latch` from 8 threads keeps the first cause.
- **T17** The rights line text, `no_new_privs` and `sudo` are absent from
  responses, rows and database bytes.
- **T18 admission barrier (limit 1):** W1 passes preflight and is held at the
  executor seam. W2 runs; its port call returns `Containment` (and, in a
  second case, `Cancelled` with residue set); W2 completes. W1 is released,
  takes the slot, and is refused by the gate: W1 makes **zero** port calls;
  total port calls is 1.
- **T19 residue bound (limit 4):** four requests are held inside the fake
  port by a barrier; each returns `Cancelled` with the residue flag set.
  After release, six more requests (more than four) each make zero port
  calls. Total port calls is exactly 4; the four rows are the unchanged
  row E; the six are refusals.
- **T20 single domain / no replacement:** construct the test-only wiring
  domain once for an owned subtree. Clone two bridge handles; latch residue
  via one and require zero runner calls through the other. Both share the
  same slot counter and readiness
  (identity comparisons use boolean assertions only). Dropping/recreating a
  bridge handle from the retained domain does not clear the latch or allocate
  another runner. The factory's constructor is private to the one wiring
  owner; there are no runner/domain replace, reset, rebind or second-runner
  attach methods. E7 extends this test with startup discovery, ticket allocation and wiring
  construction pins. No new runner is allocated by bridge-handle creation. Independent-domain R3 uses distinct owned subtrees only.
  A mutation rebuilding a runner/readiness on handle creation must fail the
  shared-latch test; E7 pins forbid a second factory call in service wiring.
- **T9 (extended)** Production selection stays inert (see E7 pins).

Mutations: gate removed, or readiness checked only in preflight (T18);
residue ignored at the gate or after the call (T13b, T19); latch removed
(T11); latch on `Cancelled` without residue (T13); rights failure withholds
tools (T14 expects N); `untrusted` not latched (T14); `Janitor` mapped to
`mechanism_unavailable` (T15); non-monotone `store` (T16); rights line
released in `tools` (T17). A source pin asserts that the post-call latch is
inside `body` (before slot release), which the D6 bound requires.

## Slice E7: startup wiring, daemon row, tool cache (any OS)

- `store::Activity::start_daemon_discovery()` writes daemon actor, principal
  `arxd`, `discover_tools`, `dedicated_account`. There is no schema change.
- A `cfg(test)` `Hooks` implementation provides `Location`, a fake
  `CgroupFs`, a manual clock and the bridge. `start_discovery` spawns one
  tracked async task: `blocking(setup)` → if Ready and not shutting down,
  take the cache ticket and timestamp, `blocking(start_daemon_discovery)` →
  `execution.dispatch(None, activity, id)` → cache update on `succeeded`.
  `cfg(test)` barriers exist at S9/S10, after `Ready`, after the row insert,
  and after dispatch.
- `drain` awaits `Execution::drain`, then the task handle, then
  `Blocking::drain`.
- The tool cache is `Mutex<Option<ToolCache { ticket: u64, tools: Value,
  observed_identity: Value, observed_at_ms: u64 }>>` on `Activity`. It is
  written only from the dispatch result value of an outcome of `succeeded`,
  and only if the ticket is greater than the cached one. `list_hosts` reads
  it.

Tests:

- **T20b wiring extension:** startup discovery and both cloned execution
  handles use E6 T20's same domain. The ticket allocator is stored alongside
  the cache on `Activity` and shared by all discoveries. Non-test
  `exec::Runner::new` call sites must remain absent while production is
  inactive. Scan those calls explicitly, plus every wiring domain constructor
  site; bridge-handle creation cannot construct a runner or second domain.
  Future activation must retain one domain per subtree until restart.
- **W1 order:** the log shows setup steps, then Ready, then the row insert,
  then the port call.
- **W2** A setup failure writes zero daemon rows and makes zero port calls.
- **W3** Shutdown during the janitor wait (manual clock, cancellation
  barrier; no tokio time pausing) gives `Stopped`, zero rows, zero port
  calls, and drain returns.
- **W3b** Shutdown at the S9/S10 barrier: `Stopped`, readiness never
  `Ready`, zero rows.
- **W3c** Shutdown after `Ready`, before the row insert: zero rows, zero
  port calls.
- **W3d** Shutdown after the row insert, before dispatch: the daemon row ends
  as the existing refused/`shutdown` row, zero port calls.
- **W3e** Reuse E6/T18's single executor test barrier seam. Hold the
  spawned worker immediately before its final
  shutdown/admission check, request shutdown, then release the barrier:
  refused `stopping`, zero port calls. Drain joins the sequence handle and
  leaves the executor set empty. A separate case holds a worker after it
  passed that check: already-admitted work may call the port and must cancel
  and drain with its honest report; do not assert zero calls in that case.
- **W4** The daemon row reads back with actor `daemon` and principal `arxd`,
  and is invisible to an agent's `list_activity`.
- **W5** The cache changes only on row N. Rows K, M, G and refusals keep the
  previous value. An update replaces the whole value. Two discoveries where
  the earlier-ticket one finishes last: the cache keeps the later ticket.
- **W6** `list_hosts` shows the cached `tools`, `tools_observed_at` and
  `last_observed_identity` equal to the response's `observed_identity`.
  Without a cache, the host object is byte-identical to the `d44035d`
  golden.
- **W7** An agent `discover_tools` row N refreshes the cache.
- **W8 production pins** (source tests): `Execution::configured` contains
  `Self::native(` and not `UnixBridge`; `impl Hooks for Native` has no
  `start_discovery` override and `ServiceApplication::start_discovery`
  spawns nothing outside `cfg(test)`; `capability()` keeps `"available":false`
  and `"service_hardening":null`; no non-test module references `hierarchy::`
  or `start_daemon_discovery`.
- **W9** A stuck fake discovery is bounded by the 10 s drain with the existing
  failure semantics. The test asserts the failure path, not that the stuck
  call was interrupted.

Mutations: discovery dispatched before Ready (W1); a row written on setup
failure (W2); shutdown not checked after Ready (W3c); cache updated on row K
(W5); completion-order overwrite without the ticket (W5); cache built from
`Evidence` instead of the response metadata (W6, mismatched marker);
production `start_discovery` wired (W8).

## Hosted compile and CI requirements (every slice)

- `cargo fmt --check`, plus `cargo clippy --workspace --all-targets --locked
  -- -D warnings` and nextest on hosted Windows, macOS and Ubuntu. Linux-only
  code compiles and runs only on Ubuntu; pure and fake tests run on all three.
- No test sleeps longer than 1 s. Deadlines are seams with production-constant
  tests. This protects the 15-minute `ci-time-budget`. No CI change is part of
  this work. E4 fakes copy the test executable once per test into an owned
  temporary directory.
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
  account: under the runner's NNP the result is `exit_nonzero`. Without the
  rule, also `exit_nonzero`. Neither is `exit_zero`. This shows the fence,
  not the policy.
- **A8** Startup discovery runs after Ready and writes one daemon row.
- **A9** Whether systemd removes delegated children at unit stop (U9).
- **A10** That cgroupfs writes and migration need no capability beyond the
  drop-in's bounding set (ADR 0019/0020 inference).
- **A11** On a merged-`/usr` host the probe uses C1 and drops C3 by the
  alias rule, and `execveat` runs the installed ELF `sudo` from an `O_PATH`
  descriptor as the build account.
- **A12** Normal runs and normal cancellation leave the residue flag clear.

Until then, every item stays **unverified on real hardware**.
