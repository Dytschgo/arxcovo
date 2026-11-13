# ADR 0020 E3 independent security, standards and spec review

Status: **CHANGES REQUIRED: one blocking Spec finding (test only); production
adapter has no blocking defect.** This is a code review. It is not acceptance,
it does not approve a merge and it grants no publication or activation.

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
No delegation and no subagents. One reviewer covered both axes, which are
reported separately below.

## Pinned target and scope

- Checkout: isolated worktree `t3/9824ff88`, outside the canonical checkout.
  `git rev-parse --show-toplevel` matched the worktree. HEAD
  `95510a5a898c0622faeb18d59511d717dca45be8`. Status was clean at start.
- Base: merged E2 main `c56b6b5` (PR60). Its tree `5def788` equals reviewed
  E2 head `be629c8`.
- Reviewed `git diff c56b6b5...95510a5` (5 files, 908 insertions), from
  commits `09ce3be`, `7764358` and `95510a5`.
- Rooted port identity: the three source blobs at HEAD are identical to
  source `eb93620` and original target `61b0ec9`: `hierarchy.rs` `0189716`,
  `hierarchy/linux.rs` `667a283`, `stage_b_tests.rs` `c00f33f`. The evidence
  blob `c9101ed` is identical to `61b0ec9`. `git diff --stat be629c8 61b0ec9`
  shows the same four files. `95510a5` adds one PROGRESS line.
- Code in scope: `exec/hierarchy/linux.rs` (697 lines, new), the 3-line
  `exec/hierarchy.rs` module declaration and the 62-line scanner extension in
  `exec/stage_b_tests.rs`.
- Read: AGENTS.md, CONTRIBUTING.md, KICKOFF, the dytschgo-review skill, ADR 0020
  (D2, D3, D4, R4, D10, Bounds), the full brief
  `docs/briefs/0020-linux-hierarchy-implementation.md` (E3 L1–L10 and
  mutations, plus the E2 port and fake contract), E2 `setup.rs` (the
  `CgroupFs` trait and driver), E1 `parse.rs` (`classify_entry`, P9), the
  existing P-SCAN tokenizer and inventory, and the E3 evidence report.

**Independence.** I independently reviewed the Sol-authored E3 work: the
production adapter, every test, the scanner extension and the evidence. ADR 0020
and its brief are Opus-authored. My conformance checks against them are
therefore **not** independent architecture review. Sol's retained design
reviews provide that independence. No independent architecture credit is
claimed here.

## Checks actually run (this review)

All checks ran on a Windows host (`x86_64-pc-windows-msvc`) with rustc
1.95.0 (59807616e 2026-04-14).

| Check | Result | What it proves |
| --- | --- | --- |
| `rustfmt +1.95.0 --check --edition 2024` on the three changed source files | pass | formatting only |
| `cargo test -p arxd --locked --lib -- l6_e3 p_scan` at HEAD `95510a5` | 4 passed, 322 filtered | L6 and the three unchanged P-SCAN tests on Windows. The unchanged `p_scan_checked_nonempty_inventory` still passes. |
| **Scratch stub typecheck, NOT the real arxd crate:** `cargo +1.95.0 check --offline --target x86_64-unknown-linux-gnu --all-targets` | pass | Verbatim `linux.rs` and `setup.rs` (SHA-256 matched to HEAD) typecheck against `libc =0.2.191` and `tempfile =3.23.0` Linux bindings. `parse.rs` was reduced to lines 5–203, with the rights section removed. `development::Cancellation` was stubbed. Nothing was linked or executed. |
| Same stub: `cargo +1.95.0 clippy … --all-targets -- -D warnings` (forced rerun) | pass, no warnings | Same caveats. The stub sets `#![allow(dead_code)]`, which matches the real module's `cfg_attr`. |
| Scratch scanner probe: verbatim copies of `tokens`, `scan_with_hook`, `scan` and `e3_source_allowed` | see A5, A6 | the denylist and inventory behaviour on synthetic inputs |

The stub results reduce the risk of a Linux compile break in these two files.
They are **not** a Linux crate compile, a Clippy run of the real crate or
native evidence. They must not be credited as such.

**Not run:** the real crate `cargo check` or Clippy for Linux (blocked
locally: there is no cross C compiler, and no toolchain, WSL or compiler was
installed); any native execution of L1–L5 or L7–L10; L8's read of its own
`/proc` status (the host is Windows, and no Linux host is available); and every
native mutation kill. No real `/proc`, `/sys` or cgroup access occurred. No
account, root, owner, service, VM, experiment, private-state or ignored-fixture
operation occurred, and no network upload occurred. No full workspace rerun was
needed: no material concern needed one.

**Builder credits, accepted as stated and limited to Windows:** fmt and Clippy,
the focused nextest run of 60 passed, and the single full run of 362 passed and
3 skipped. These ran at the original pin, `eb93620` (source) / `61b0ec9`
(evidence), whose blobs are identical at HEAD. They do not prove any Linux path.
The builder's real-crate Linux check was blocked by the missing
`x86_64-linux-gnu-gcc`. Native paths stay **unverified on real hardware**.

## Standards

Sources: AGENTS.md (meaningful tests, honest evidence, path safety and no
secrets), CONTRIBUTING.md, KICKOFF CI rules and the Fowler smell baseline.
Baseline smells are judgement calls.

No hard standards breach was found:

- The diff touches no Cargo, CI, service, wiring or PROGRESS source files. E4,
  E6 and E7 are untouched.
- There are no secrets, paths or file bytes in errors. `FsError` is a fixed
  enum.
- The evidence labels every unrun path and claims no Linux compile.
- The repo documents no rule requiring `// SAFETY:` comments. `cgroup.rs` has
  none, so their absence is not a breach.

Advisories (smells and judgement):

- **S-A1 Duplicated Code.** `TEXT_MAX` and `RECORD_MAX` are defined in both
  `setup.rs:11-12` and `linux.rs:13-15`. A drift between the port's bound and
  the adapter's bound would go unnoticed. Export the `setup` constants and use
  them in the adapter.
- **S-A2 Mysterious Name.** `last()` (it captures errno), `facts()` (`fstat`
  plus `fstatfs`) and `text()` hide what they do. Possible names:
  `errno_error`, `fd_stat` and `read_bounded_and_close`.
- **S-A3 Stale doc.** The `hierarchy.rs:1` module doc still says "E1 is pure
  data parsing only". The module now also holds the setup driver and the Linux
  adapter.

## Spec

Spec: brief slice E3 (lines 247–305), ADR 0020 D2, D4.3, R4, D10 and Bounds.

### Verified conformant (source analysis)

- **Descriptor discipline.** The only path-based open is `openat(AT_FDCWD,
  "/")` (`linux.rs:142`). Every later operation is relative to a descriptor:
  `openat`, `fstat`, `fstatfs`, `fstatat(AT_SYMLINK_NOFOLLOW)`, `fgetxattr`,
  `mkdirat` and `unlinkat`. Directories use
  `O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC`. Files use
  `O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK`. `O_NONBLOCK` stops an open of a FIFO from
  blocking before `fstat` rejects it. `Dir` wraps a private `OwnedFd`, so it
  cannot be built from an arbitrary fd outside the module.
- **Trust.** The root and the `sys` and `fs` ancestors are checked for a
  directory type, no group or other write, and UID 0. `cgroup` is
  additionally checked for magic `0x63677270`. Every port method re-checks the
  bound directory (type, mode, UID and cgroup2 magic) before acting. Control
  files must be regular files with no group or other write, UID 0 and cgroup2
  magic. That is stricter than the ADR's open-only S9 requirement. The proc
  directory is checked for UID 0 and procfs magic. `/proc/<pid>` and its file
  are checked for type and procfs magic.
- **fd ownership, close and errno.** `OwnedFd` takes ownership at
  `linux.rs:59`. On failure, `fdopendir` leaves the fd owned by `OwnedFd`,
  which closes it on the error return. On success, `into_raw_fd` hands
  ownership to the stream exactly once. `Stream::finish` propagates errors
  from `closedir` and nulls the pointer, so `Drop` cannot close twice. On
  early returns, `Drop` closes the stream after errno has been captured.
  `text`, `write` and `open_ro` close explicitly and propagate close errors
  without retrying (Linux releases the fd even on `EINTR`). `readdir` resets
  errno to 0 before each call. ENODATA and EEXIST are read before any other
  libc call.
- **Bounds.** Reads use `take(max+1)` and then `> max` gives `TooBig`. `read`
  clamps its bound to 4,096, `proc_cgroup` uses 4,096 and `proc_status`
  uses 8,192. Writes accept 1 to 4,096 bytes and make a single `write(2)`; a
  short write is `Io`, with no retry. Listings stop at the 513th raw record,
  with `.` and `..` counted. The xattr buffer is 2 bytes, and `ERANGE` maps to
  `TooBig`. Path components must be 1 to 255 bytes and cannot be `.`, `..`,
  or contain `/`, `\` or NUL.
- **Listing.** Each call does a fresh `openat(dir, ".", DIR_FLAGS)`, re-checks
  the result and then `fdopendir`s that new fd, never the handle. There is no
  `dup`, no cached stream and no reuse. The `cfg(test)` observer sees every
  listing fd.
- **`DT_UNKNOWN`.** Only `DT_UNKNOWN` triggers
  `fstatat(handle, name, AT_SYMLINK_NOFOLLOW)`. A failure gives `Err(())`,
  which E1 classifies as `Refused`.
- **`mkdirat(unit, "runs", 0o700)`.** EEXIST gives `Exists`. The driver then
  opens `runs` with `O_NOFOLLOW|O_DIRECTORY` and checks it.
- **System constants and test constructor.** `system()` sets UID 0,
  `0x9fa0` and `0x63677270`. `owned(uid)` is a private `#[cfg(test)] fn`. It
  bypasses only UID and magic equality; type, mode, no-follow, bounds and
  listing code stay shared. In non-test builds `bypass()` is the constant
  `false` and the `owned` and `observer` fields do not exist. Integration-test
  builds do not set `cfg(test)` on the library. No authority leaks.
- **Inactive.** No module outside `linux.rs` references `hierarchy::linux` or
  `LinuxFs` (checked with grep). Production selection, `capability()` and
  `MechanismUnavailable` are unchanged, because the diff touches only these
  files.
- **Tests that do kill their listed mutations (by source analysis; no kill was
  run):**
  - L2: dropping `O_NOFOLLOW` makes the read and the child open succeed and the
    write change the target. Dropping `O_NONBLOCK` makes the FIFO open block,
    which fails the test by hanging.
  - L3: removing the record bound, or excluding `.` and `..` from it.
  - L5: building `system()` with the test UID. A mode-0755 `mkdirat` would
    also fail the `0o700` check.
  - L7: raising the xattr buffer.
  - L8: checking identity before hardening, or any later observation. Both
    produce `Panicked`, not `Hardening`. The counting decorator never forwards
    a mutation, even when the code is mutated.
  - L9: listing through a `dup` (`dup` clears `FD_CLOEXEC`, and the shared
    offset empties the second listing), or a listing fd without `O_CLOEXEC`.
  - L10: classifying a symlink as `Child`.
- **Root and skip behaviour.** Tests that create fixtures skip as root and
  print `SKIP E3 owned native tests: root`. L8 also skips when its own status
  is unexpectedly `Standard`, and L7 skips on `ENOTSUP`, `EPERM` or `EACCES`.
  Each skip prints a fixed marker. Evidence must report a skip as skipped, not
  passed.
- **Scanner extension.** It is append-only and reuses the real `tokens` and
  `scan`. The existing `p_scan_checked_nonempty_inventory` expectation and the
  scanned set are unchanged (prior inventory preserved). It adds a denylist for
  `read_dir`, `File::open`, `canonicalize`, `metadata(`, `exists(`, `kill(`,
  `killpg(`, `setpriority(`, `Command`, `dup(`, `dup2(` and both `F_DUPFD`
  forms. `set_hook` is covered by `scan`'s hook count. Each token has a mutant
  self-test, and the comment and string positive control passes.

### Blocking

- **B1. L1 cannot kill the brief's mutation "`take(4097)` raised (L1)".** In
  `text()` (`linux.rs:86-99`), the read is followed by a check that
  `bytes.len() > max`. Raising `take(max + 1)` to any larger value, or removing
  `take` entirely, gives the same observable results for L1's fixtures: a
  4,096-byte file returns Ok and a 4,097-byte file returns `TooBig`. The
  allocation bound, which is the property the mutation targets, is never
  observed. The brief requires "each must fail a named test", and the evidence
  maps this mutation to L1. Production is correct, and E2's driver re-checks
  the length, so the security impact is low. The acceptance mapping is still
  unsatisfiable.

  **Fix (test only):** pass `text()` the read end of a pipe holding, say,
  8,192 bytes, with the write end kept open and a `dup` of the read end kept
  in the test. The test module is outside L6's scan. Then assert `TooBig` and
  that `ioctl(FIONREAD)` on the dup reports exactly `8192 - 4097` bytes left
  unread. Removing `take` would block or fail with EAGAIN; raising it would
  change the count left unread.

### Advisory

- **A1. The L10 symlink fixture does not discriminate acceptance as a regular
  file.** The link is named `link` and points to a regular file. If an adapter
  mutation followed symlinks or rewrote `d_type` to `DT_REG`, the E1 name
  grammar would still refuse `link`, so the mutation would survive. E1 P9
  already covers the classifier's `DT_LNK` handling with control-grammar names.
  Suggestion: name the link `memory.max` and add a symlink to the child
  directory.
- **A2. The `DT_UNKNOWN` branch is never exercised natively.** ext4 and tmpfs
  report `d_type`, so the `fstatat(..., AT_SYMLINK_NOFOLLOW)` path at
  `linux.rs:321-338` runs in no test. Dropping `AT_SYMLINK_NOFOLLOW` there
  would survive. L4 covers only the pure classifier. Suggestion: extract a
  `record(dir, name, d_type)` helper and test it on a real symlink with
  `DT_UNKNOWN` forced.
- **A3. Numeric PID binding is not identical to `/proc/self`.** ADR T2 and R4
  name `/proc/self/{cgroup,status}`. The adapter opens `/proc/<getpid()>`
  instead (`linux.rs:170`). The two are equivalent when the procfs mount
  belongs to the caller's PID namespace. Under a mismatched procfs mount, for
  example a container with a host `/proc`, the number names another process.
  T8's `cgroup.procs` equality is a strong backstop, and ADR A1 makes
  privileged mounts a trust assumption. Even so, the evidence says "no
  architectural deviation". Suggestion: fail closed unless a bounded
  `readlinkat(proc, "self")` (at most 11 bytes) equals the decimal PID. Or
  record the substitution and its namespace premise in the ADR.
- **A4. Some bounds are not pinned.** `STATUS_MAX` (8,192, ADR R4) has no test:
  L8's own status is far smaller, so changing it either way survives. L5 does
  not pin `TEXT_MAX`, `RECORD_MAX` or `STATUS_MAX`. Suggestion: add them to
  the L5 constant test. L7 also does not cover a missing `user.delegate`
  (ENODATA gives `None`).
- **A5. L6 runs a parallel scan; it does not extend the inventory.** The
  `scan()` inventory for `linux.rs` and `hierarchy.rs` is computed and then
  discarded. The probe shows that both are currently empty, but injecting
  `unreachable!("fixed")` into production `linux.rs` is still accepted by L6.
  The `*name != "hierarchy/fake.rs"` assertion is vacuous over a literal list.
  This matches the brief, because D10's reviewed inventory belongs to E6.
  Suggestion: pin an empty inventory now.
- **A6. L6 matches whole identifiers and misses several equivalents.** The
  scratch probe accepted all of the following: `symlink_metadata(`,
  `try_exists(`, `libc::dup3(`, `.try_clone()`, `.try_clone_to_owned()`,
  `libc::open(`, `libc::stat(`, `libc::lstat(`, `libc::opendir(`,
  `OpenOptions::new()`, `File::create(` and `std::fs::read(`. Read as
  substrings, the brief's `metadata(` and `exists(` would cover the first two.
  `try_clone` and `try_clone_to_owned` use `F_DUPFD_CLOEXEC` internally, which
  the scan otherwise denies. With a rewind, they also pass L9's `CLOEXEC` and
  offset checks. Suggestion: add these tokens, with mutant self-tests.
- **A7. Fixtures depend on the umask.** Fixture files and directories take
  their modes from the umask (`0666 & ~umask`). Under umask `002`, L1 fails
  loudly (group write is refused), and L2's `O_NOFOLLOW` checks would stop
  killing anything because the targets would already be refused. Suggestion:
  set explicit modes `0o644` and `0o755`. The temporary root is already
  `0700`.
- **A8 (informational).** The file opens do not use `O_NOCTTY`. The parent
  directory's cgroup2 or procfs magic is checked before every file open, and
  `O_DIRECTORY` opens refuse non-directories before any driver code runs. A
  device therefore cannot be reached on a production path, so this is defence
  in depth only.

## Platform limits and open gates

- No Linux crate compile, Linux Clippy, native L1–L5 or L7–L10 run, or native
  mutation kill exists. L7 has neither a pass nor a skip on record. These must
  come from hosted Ubuntu CI, an ordinary non-root user and the exact head.
  CI must never run them as root.
- Source-inference items are **unverified on real hardware**: kernfs ignores
  `O_NONBLOCK` on control writes, cgroupfs supports `user.*` xattrs, and the
  `mkdirat` mode is honoured on cgroup2.
- Publication still needs: B1 fixed and rechecked by a different provider,
  green required checks on the exact re-pinned head, and the coordinator's
  merge gate. Production stays `MechanismUnavailable`. There are no callers
  and the native adapter is not activated.
- M2 is incomplete. Native, installed and dedicated paths are **unverified on
  real hardware**. The dispatcher privacy flake, `E_PROTECTION` and
  `ExecutionFailed` causes remain unknown. No retry, diagnosis or activation is
  claimed or authorized.

**Summary.** Standards: 0 hard findings and 3 advisory smells (worst:
duplicated bound constants). Spec: 1 blocker and 8 advisories (worst: B1, L1
cannot kill the raised-`take` mutation).
