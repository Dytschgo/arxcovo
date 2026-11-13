# ADR 0020 E4 independent security, standards and spec review

Status: **CHANGES REQUIRED.** There are two blocking findings:

- **Standards B1:** the Linux code does not compile, and it also has two
  Clippy `-D warnings` failures.
- **Spec B2:** the Unix S6 integration test runs the real system `sudo`
  outside the NNP fence.

The descriptor-bound probe design itself has no blocking security defect found
by source analysis. This is a code review. It is not acceptance, it does not
approve a merge and it grants no publication or activation.

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
No delegation and no subagents. One reviewer covered both axes, which are
reported separately below.

## Pinned target and scope

- Checkout: isolated worktree `t3/1aa9fa64`, outside the canonical checkout.
  `git rev-parse --show-toplevel` matched the worktree. HEAD was
  `9fdea584d8b197383976edcb70eaa7d45fcc57b1`, and status was clean at start.
- Explicit target: `e011ec1` (evidence) on source
  `4967b53adb8cad574f90cd9f14c537e6eb6a4ce2`, whose parent is `38b207d`.
  Reviewed `git diff 38b207d...e011ec1`: 7 files, 952 insertions and 2
  deletions.
- Root port identity:
  - The six source blobs and the evidence blob are identical at `e011ec1` and
    at HEAD: `stage_b_tests.rs` `4f376c9`, `unix_shim.rs` `e70d088`,
    `unix_shim_rights.rs` `b269e00`, `unix_shim_rights_linux.rs` `2902061`,
    `unix_shim_rights_linux_tests.rs` `da4d91f`, `tests/unix_shim.rs`
    `437e59e` and `linux-rights-probe-e4.md` `d7de4d0`.
  - The E3 merged main `af9289a` has the same tree as `231c367` (`f0c692a`).
  - The port base `51a4bd0` has the same tree as `38b207d` (`3564133`), so the
    native evidence docs are preserved (`linux-hierarchy-e3-native.md`
    `c147249` is identical in all three places).
  - `git diff e011ec1 HEAD` contains only 8 PROGRESS lines, from `9f166ff` and
    `9fdea58`.
- Evidence hash manifest: the SHA-256 values of the three rights files, taken
  from `git show e011ec1:…` (LF bytes), match the evidence table exactly. The
  blob IDs of all six source files match it too.
- Read: AGENTS.md, CONTRIBUTING.md, KICKOFF, the dytschgo-review skill, ADR 0020
  (D8, R4, the R5 line, the S1 bound-executable probe, classification and
  Bounds), the brief's gates, slice table, E4 (S6, S8–S13 and mutations), E6
  T6, the hosted CI requirements and the privileged-acceptance fence.
  Also read: the existing Stage C shim and its tests, the P-SCAN tokenizer and
  inventory, the bridge's `parse_tools` and the E4 evidence.

**Independence.** I independently reviewed the Sol-authored E4
implementation, tests, scanner extension and evidence. ADR 0020 and its brief
are Opus-authored. My conformance checks against them are therefore **not**
independent architecture review, and no independent architecture credit is
claimed.

## Checks actually run (this review)

All checks ran on a Windows host (`x86_64-pc-windows-msvc`) with rustc
1.95.0 (59807616e 2026-04-14).

| Check | Result | What it proves |
| --- | --- | --- |
| Real crate: `cargo clippy -p arxd --all-targets --locked --target x86_64-unknown-linux-gnu -- -D warnings` (the target std was already installed) | **blocked** | `ring`'s build script needs `x86_64-linux-gnu-gcc`. Nothing was installed. |
| **Scratch stub, NOT the real arxd crate:** the three rights files copied verbatim (SHA-256 matched), plus a 4-line stub `unix_shim.rs` with the real `mod rights` and `append(observe())` call. `cargo +1.95.0 clippy --offline --all-targets --target x86_64-unknown-linux-gnu -- -D warnings` | **FAIL: E0308** | See B1. Nothing was linked or executed. |
| Same stub, after stub-only diagnostic patches (`.cast()` on argv/envp, then `#[allow(clippy::derivable_impls)]` and `_ => ()`), forced rerun | fail, then pass | These are the complete Linux type and Clippy failure sets for these three files, at stub level. The patches are not part of the target. |
| Same stub, verbatim, `--target aarch64-apple-darwin` all-targets Clippy `-D warnings` | pass | The non-Linux `rights` path typechecks and lints clean. It is not a macOS run. |
| `rustfmt +1.95.0 --check --edition 2024` on the original `unix_shim_rights_linux.rs` / `_tests.rs` | pass / **26 diff hunks** | See S-A1. The real `cargo fmt --all --check` passes because `include!` is not followed. |
| `cargo test -p arxd --lib --locked -- s13_ s12_e4 s6_rights p_scan_` at HEAD | 7 passed, 323 filtered | Portable line bounds, pins, scanner and P-SCAN on Windows |
| Portable mutant M1: `O_NOFOLLOW` dropped from the final `O_PATH` open | killed by `s12_e4_portable_production_pins` | It is a token pin only. The behavioural S9 kill is Linux-only and unrun. |
| Portable mutant M2: a production `std::process::Command::new("sudo")` | killed by both S13 tests | Scanner bypass control |

The worktree was restored after each mutant (`git status` empty).

**Not run:** the real crate's Linux or macOS check, Clippy or tests. The
integration file `tests/unix_shim.rs` was not compiled for any Unix target.
No native S6, S8–S11 or S12 run happened, and no native mutation kill. No
system `sudo` and no production tool probe ran. There was no `/proc`, `/sys`,
root, account, owner, service, cgroup, VM, private-state, experiment or
ignored-fixture action, and no network upload. No full workspace rerun was
needed beyond the focused set.

**Builder credits, accepted as stated and limited to Windows:** these ran at
source `4967b53`, whose blobs are identical at HEAD:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- one final `cargo test --workspace --locked`: 366 passed, 3 ignored

They prove no Linux or macOS path. Linux and macOS compile, runtime and
native mutation kills are **zero**. Native paths stay **unverified on real
hardware**.

## Standards

Sources: AGENTS.md (meaningful tests, honest evidence, path safety, no
secrets), CONTRIBUTING.md, the KICKOFF CI rule (fmt, clippy `-D warnings`,
nextest on hosted Ubuntu) and the Fowler smell baseline. Baseline smells are
judgement calls.

### Blocking

- **B1. The Linux code does not compile, and Linux Clippy would fail.** Found by
  the stub (verbatim files, real `libc =0.2.191`). The real hosted Ubuntu job
  `cargo clippy --workspace --all-targets --locked -- -D warnings` would be red.
  1. `unix_shim_rights_linux.rs:308-313`: E0308. The glibc binding is
     `execveat(c_int, *const c_char, *const *mut c_char, *const *mut c_char,
     c_int)` (`libc-0.2.191/src/unix/linux_like/linux/gnu/mod.rs:1205`).
     `argv.as_ptr()` and `env.as_ptr()` are `*const *const c_char`. Fix:
     `.cast()` both, or build `Vec<*mut c_char>`. `execveat` does not mutate
     them.
  2. `unix_shim_rights_linux.rs:72`: `clippy::derivable_impls`. In non-test
     builds `Settings` has no fields, so the manual `Default` is derivable.
     Derive `Default` with the fields' `cfg(test)` defaults moved elsewhere,
     or use a scoped `#[allow]` with a reason.
  3. `unix_shim_rights_linux_tests.rs:193`: `clippy::needless_return`
     (`_ => return,`).

  None of the Linux-only tests can run until this is fixed. Every Linux claim
  in the evidence stays "unrun", and the evidence is honest about that.

### Advisories

- **S-A1. The test file escapes rustfmt.** `unix_shim_rights_linux_tests.rs` is
  reached only by `include!`, so `cargo fmt --check` never sees it. rustfmt
  reports 26 hunks of dense one-line code. The existing included
  `unix_shim_tests.rs` is rustfmt-clean (0 hunks), so this is a new
  departure. Format it, or declare it as a `#[path]` module.
- **S-A2. Primitive Obsession / stringly control flow.** `"absent"`,
  `"untrusted"` and `"unknown"` are both error values and match keys
  (`Err("untrusted") =>`). A misspelt literal falls into `Err(_)` and becomes
  `unknown`. That fails closed but silently loses the distinction. A small
  private enum, rendered once in `append`, removes this risk.
- **S-A3. Mysterious Name.** `fd()` relocates stdio-range descriptors *and*
  classifies errno into wire outcomes. `directory()` also checks trust.
  Possible names: `owned_above_stdio` and `open_trusted_dir`.

## Spec

Spec: ADR 0020 D8 (the R5 line, S1 steps 1–8, classification, Bounds) and the
brief's E4 slice (S6, S8–S13, mutations), CI requirements and gates.

### Blocking

- **B2. The Unix S6 integration test executes the real system `sudo` outside
  the fence.** `tests/unix_shim.rs:239-285` spawns the production `arxd
  tool-probe` without `PR_SET_NO_NEW_PRIVS`. Integration builds do not set
  `cfg(test)` on the library, so `rights::observe()` runs the production probe
  against `/`. On a standard Linux host, or the hosted Ubuntu runner with
  NOPASSWD sudo, C1 `/usr/bin/sudo -n true` runs setuid-effective and executes
  `true` **as root**. With a cached sudo timestamp it also refreshes the
  developer's ticket. The evidence itself calls this the "Unix S6 system-sudo
  test" and leaves it unrun. This conflicts with three things:
  - the brief's E4 test scope: "owned temporary directories, current user";
  - ADR D8: "no check … runs an unfenced privileged probe";
  - this review's authorization, which forbids any actual system sudo.

  There are side effects as well:
  - S6 asserts `< 9 s` while it already waits about 5 s for the `rustup`
    sleeper. A slow `sudo` (directory service) can add up to 5 s and fail it.
  - An elevated child (real UID 0) cannot be SIGKILLed by the user, so the
    deadline guard's blocking `waitpid` is unbounded outside R3.

  Fix: on Linux, spawn S6's shim with `prctl(PR_SET_NO_NEW_PRIVS)` in
  `pre_exec` (R3 parity). Assert `no_new_privs == true` and
  `sudo != "exit_zero"`. The system binary then still runs, but without
  elevation. Alternatively, explicitly accept and document an unprivileged
  real-`sudo` run. The fix also closes Spec A1.

### Verified conformant (source analysis only; nothing native ran)

- **Anchor, ancestors and final object.** The anchor opens with
  `O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` and is checked for UID ==
  owner, no group/other write, and directory type. Each component uses
  `openat(parent, …, same flags)` with the same check, re-anchored through
  `openat(root, ".")` (no pathname re-resolution). The final object uses
  `openat(dir, "sudo", O_PATH|O_NOFOLLOW|O_CLOEXEC)`, and `fstat` must show
  `S_IFREG`, the owner and no group/other write. A final symlink yields a link
  descriptor, `S_IFLNK`, and so `untrusted`.
- **Error classification.**
  - `ENOENT` gives `absent`.
  - `ELOOP` or `ENOTDIR` (a symlink or non-directory component) gives
    `untrusted`.
  - Any other error, or an `fstat` failure, gives `unknown`.
  - An anchor failure gives `untrusted` or `unknown` only.
- **Alias rule.** It applies only to C3's first component:
  `fstatat(root, "bin", AT_SYMLINK_NOFOLLOW)`. The link must be owned by the
  owner, and `readlinkat` uses a 16-byte buffer with an exact byte equality to
  `usr/bin` or `/usr/bin`. A truncated 16-byte read can never equal a 7- or
  8-byte target. The link is never followed. Any other link, or a foreign
  owner, gives `untrusted`. A non-link `bin` falls through to the normal
  checks.
- **Classify-all-then-execute.** All three candidates are evaluated. Any
  `untrusted` wins, then any `unknown`, and only then the first trusted
  candidate runs, otherwise `absent`. The result is fail-closed: an
  observation error blocks execution even when C1 is trusted. ADR step 3 makes
  "any other error → unknown".
- **Bound execution.** argv, envp, `/dev/null` (`O_RDWR|O_CLOEXEC`), the cwd
  descriptor (`/`, trust-checked) and a `pipe2(O_CLOEXEC|O_NONBLOCK)` pipe are
  all prepared before `fork`. The child calls only `fchdir`, `dup2` ×3,
  `execveat(fd, "", argv, envp, AT_EMPTY_PATH)`, `write` and `_exit(127)`: no
  allocation, no destructors, and `_exit` diverges. `dup2` gives fds 0–2
  without CLOEXEC, and every other prepared descriptor is CLOEXEC. A script
  candidate fails with `ENOENT`, and an `E` byte on the pipe gives `unknown`.
  `sudo` output can never reach the shim's stdout table.
- **Closed standard descriptors.** `fd()` moves any 0–2 result above stdio
  with `F_DUPFD_CLOEXEC` and closes the low fd, so `dup2` cannot clobber the
  executable, pipe or cwd descriptors.
- **Single-threaded fork.** The shim's `probe()` spawns tools with no reader
  threads (confirmed in `unix_shim.rs`), and the rights call runs after them.
- **Deadline, kill and reap.** The parent uses `waitpid(WNOHANG)` every 5 ms
  against an `Instant` deadline. The `Child` guard sends SIGKILL to its own
  unreaped PID and then reaps it with a blocking wait (retrying `EINTR`).
  `ECHILD` marks the child reaped and gives `unknown`. Exit 0 gives
  `exit_zero`; another exit gives `exit_nonzero`; a signal or a pipe byte gives
  `unknown`. Only the child's own PID is ever signalled.
- **NNP read.** `/proc/self/status` is opened with `O_NOFOLLOW|O_CLOEXEC` and
  must have procfs magic `0x9fa0`. The read is `take(8193)` and anything over
  8,192 bytes is refused. The file must be UTF-8 with no CR or NUL, and have
  exactly one `NoNewPrivs:\t0|1` line. Anything else gives `null`.
- **Line and bounds.** Key order is literal, the values come from fixed sets,
  and the longest line is 56 bytes, so the 96-byte cap holds. The aggregate
  check `output + line > 4096` gives 125 before stdout. The line is appended
  after the TOOLS loop. Non-Linux Unix returns `(None, "unknown")` with no
  probe code compiled.
- **Shim invariants.** The diff to `unix_shim.rs` is the module line plus a
  one-line swap of the bound check. The silent hook, `catch_unwind`, euid-0
  refusal and `/` cwd are unchanged.
- **Seams have no authority.** `anchor`, `owner`, `deadline`, `argv`, `env`
  and the hook exist only under `cfg(test)`. Production accessors return the
  consts. `S12` pins them by value and by token. The ignored helpers return
  early without their fixture variable.
- **Privacy scanner.** Both rights files are added to the P-SCAN inventory
  *without* the shim's hook exemption. The E4 token denylist adds `Command`,
  exec-family calls, raw exec syscalls, env lookups and `env!`, with 10
  negative controls. M2 confirms a kill.
- **Inactive.** Nothing outside the shim calls `rights`, and there are no
  E6/E7, readiness or production-caller changes. Production stays
  `MechanismUnavailable`.

### Mutation discrimination (by source analysis; only M1 and M2 were run)

| Brief mutation | Expected kill | Assessment |
| --- | --- | --- |
| exec by path after validation | S9c file and directory swap: the `B` marker appears | sound; the `/proc/self/exe` mode byte proves which inode ran |
| ancestor check dropped | S9 cases 3, 4, 5: the `A` marker appears | sound |
| final `O_NOFOLLOW` dropped | S9 case 2 (`A` runs); portable pin M1 | sound |
| alias: any target / `ends_with` | S9 cases 6 (`trusted/usr/bin`) and 7 (`usr/bin/`) | sound |
| execution after untrusted | S9 cases 0 and 3 (untrusted later or earlier, markers) | sound |
| final `O_CLOEXEC` dropped | S8 script case: `/bin/sh` runs and writes `A` | sound |
| environment inherited | S8 `A`: the helper asserts one env var, so 101 gives `exit_nonzero` | sound (via S8, not S12) |
| `PATH` search | S10: the helper's result must be `absent` | sound |
| timeout gives `exit_nonzero` | S8 `S` | sound |
| rights before tools | Unix S6 (last row); portable token order | sound |
| `no_new_privs` hard-coded `true` | S11 child (`false` case) | **partial**, see A1 |

### Advisories

- **A1. S11 tests the `nnp()` helper, not the shim line.** The brief says
  "the shim spawned with `PR_SET_NO_NEW_PRIVS` … gives `no_new_privs: true`".
  A mutant that hard-codes `Some(true)` in `rights::observe()` survives every
  test. The B2 fix (S6 under NNP, plus a non-NNP assertion of `false`) closes
  this.
- **A2. Foreign-owner cases are unit-level.**
  - S9(b), the final file owned by another UID: `s9_final_object_owner_check`
    calls `trusted()` directly.
  - S9b's foreign link owner: `alias()` is called directly with `owner + 1`.
  - S9 case 1 actually tests a foreign *anchor*.

  This is acceptable without chown or root, but the evidence should not map
  these cases to an integrated no-marker run.
- **A3. Fixture cost and flake risk.**
  - The brief says "copy the test executable once per test". The tests copy
    it per fixture and per fake, roughly 51 full copies of the debug test
    binary across S8–S10.
  - With the 200 ms seam deadline, a slow re-entry under parallel load turns
    `A`, `3` or `K` into `unknown`, or leaves the `S` marker unwritten.
  - Under plain `cargo test` (threads, not nextest processes), a concurrent
    fork can hold a fake's write fd and cause `ETXTBSY`.

  Watch the hosted timings. Consider one copy per test with derived fakes.
- **A4. Raised glibc floor.** `libc::execveat` is a strong glibc 2.34 symbol,
  so the whole Linux `arxd` binary would fail to load on glibc < 2.34, not
  just the probe. The repo calls `openat2` via raw `syscall`, but E4's S13
  bans `syscall`. Stage F's systemd ≥ 254 floor makes this likely moot.
  Record the floor, or decide otherwise, in ADR/PROGRESS. musl has no binding.
- **A5. Inherited `SIGCHLD = SIG_IGN`.** With this disposition the kernel
  auto-reaps, and a deadline that falls inside the ≤5 ms poll gap could
  SIGKILL a reused PID. This is not reachable via systemd or the Rust runner as
  far as I can tell. Resetting `SIGCHLD` to `SIG_DFL` before `fork` removes
  it.
- **A6. The deadline guarantee depends on R3.** Kill succeeds only for a
  same-UID child, which R3 provides in production. Outside R3 (see B2) the
  reap is unbounded. The ADR already lists the reap as a non-interruptible
  step.
- **A7. Bridge contract gap until E6.** The current `parse_tools` requires
  exactly `TOOLS.len()` lines, so every post-E4 probe becomes an invalid table
  (row K, no tools). The slice order and E6 T6 intend this, and the bridge is
  inactive. E4 must not reach any activation path without E6.
- **A8. Nits.**
  - ADR/brief say the total stays "below/under 4,096", but the code and test
    accept exactly 4,096 (matching the prior cap and capture size).
  - Both S13 scanners truncate at the first `#[cfg(test)] mod tests`, so code
    placed after an early test module would go unscanned. This is the same
    as the E3 pattern.
  - A group-writable `/usr/local/bin` (legacy `staff` setups) gives
    `untrusted`, then `rights_mismatch`. That is the intended
    availability-not-safety trade, **unverified on real hardware**.

## Assessment of the fake approach

The native fakes are meaningful and verifiable on Linux. Each fake is a copy
of the test ELF with one appended mode byte. The single fixed ignored helper
reads its own `/proc/self/exe` (also valid after unlink or rename), writes a
marker named by that byte, then checks cwd `/`, a one-variable environment
and EOF on stdin. That proves which inode ran and under which conditions.
Unit calls use no owner, system path or privileged command; they touch only
`/` (fchdir), `/dev/null` and their own `/proc/self/*`. Their kill power is
**unexercised** until B1 is fixed and a hosted ordinary-UID Linux runner
executes them.

## Limits and carried state

- M2 remains incomplete. Every native path is **unverified on real
  hardware**. E4 is inactive execution architecture only and provides no E6/E7
  readiness or production caller.
- The dispatcher privacy flake remains unresolved. The `E_PROTECTION` and
  `ExecutionFailed` causes remain unknown, with no experiment retry.
- Publication still needs the B1 and B2 corrections, hosted Linux and macOS
  checks (fmt, Clippy, nextest, with the E4 native fixtures run and any skip
  marker reported as skipped), and a recheck under the normal gates.

Standards: 1 blocking (B1 Linux compile and Clippy failure) and 3 advisories.
Spec: 1 blocking (B2 unfenced real `sudo` in S6) and 8 advisories.
