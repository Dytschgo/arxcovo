# Brief: Unix native execution bridge (ADR 0019)

Current status (2026-11-12): accepted inactive contract under the current
[ADR 0019 disposition](../adr/0019-unix-native-execution-bridge.md).
Implementation waits for this docs PR's independent exact-head review and
all required hosted checks, then merge. Staged reviews, test-only construction,
cgroup setup/cleanup, the E_PROTECTION diagnosis and all activation gates
remain mandatory. No native experiment retry or owner operation is authorized.
All tests and mutations below are requirements, not executed evidence.

## Historical proposal and clarification checkpoints

Author: Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-11. Base
`07ae3c89`. Docs only: nothing was built, run or tested. A fresh independent
Sol review of [ADR 0019](../adr/0019-unix-native-execution-bridge.md) and this
brief must pass before implementation. Production stays
`MechanismUnavailable`. Native, installed and dedicated M2 remain **unverified
on real hardware** and incomplete.

Line numbers refer to `07ae3c89`. The cited files are byte-identical at
current main `1d828ca7` (see ADR 0019 "Sources").

Clarified 2026-11-12 by the same provider and model; this is not an
independent review. It settles Sol advisories B-A1 and B-A2 through ADR 0019
C1–C3. The new sections are §3a, §7a and the additions to §3, §5, §8, §9 and
§10. The brief remains a proposal until a fresh independent Sol review passes.
Everything below is a requirement. No test, mutation or build was run.

## Scope

In scope: the bridge from the accepted `ServiceConfig` and the immutable `Pin`
to `exec::OwnerConfig`, `RunSpec` and cancellation. Also the mapping from every
runner result and error to an ADR 0017 `Report`, the Unix shim, and staging.

Out of scope: the owner CLI and ADR 0018, any ADR 0017 validator redesign,
Windows backends and S4U, the macOS sweep, Linux admin, cgroup hierarchy setup
and the janitor (named only as prerequisites), privileged or hosted runs, and
`PROGRESS.md`.

## 1. Derivation (pure; no I/O at construction)

`UnixBridge::new(layout, &ServiceConfig, cgroup_parent: PathBuf, runner_port)`
builds everything once. It never touches the workspace, `PATH` or tool paths.
A test must pass with a workspace path that does not exist.

| `OwnerConfig` field | Source | Refusal (bridge preflight returns it every time) |
| --- | --- | --- |
| platform gate | `layout.platform()` | macOS or Windows → `MechanismUnavailable` (Windows production never builds it) |
| `admin` | `config.rights()` | `Admin` → `MechanismUnavailable` |
| `account` | `config.build_name()` | `Runner::new` rejects (not `[A-Za-z0-9_-]{1,128}`) → `PreflightFailed` |
| `uid` | `config.build_identity()` parsed as `u32` | parse failure → `PreflightFailed` |
| `gid` | `config.build_gid()` | `None` or `0` → `PreflightFailed` |
| `workspace` | `config.workspace()` | `Runner::new` lexical rejection → `PreflightFailed` |
| `protected_directories` | `layout.private_dirs()` (Linux: `/etc/arxcovo`, `/var/lib/arxcovo`) | — |
| `environment` | `config.environment()` (same Unix key list as ADR 0007) | — |
| `cgroup_parent` | input; see below | — |
| `memory_bytes`, `pids` | `config.limits().1`, `.2` | outside 16 MiB..=64 GiB or 1..=1024 → `PreflightFailed` |

- `ServiceConfig` accepts wider values than `Runner::new` (any name except NUL
  up to 256 bytes, `memory_bytes ≥ 1`, any `pids ≥ 1`, `gid 0`). That
  mismatch is resolved by refusing at bridge construction, with no schema
  change. The existing config fixture (`memory_bytes: 1024`) is refused, so
  bridge tests need their own fixture.
- Do not add the binary directory or `/run/arxcovo` to the protected list. Both
  are `0755` by design, and `account()` (`unix.rs:128`) would refuse them.
- `cgroup_parent` is a pure parser over a bounded `/proc/self/cgroup` byte
  string (at most 4,096 bytes). It accepts exactly one `0::/…/supervisor` line
  and returns `/sys/fs/cgroup/<unit path>/runs`. It refuses v1 lines, extra
  lines, NUL, `.` or `..`, a missing `supervisor` leaf, and a root-only path.
  Reading the file, enabling `+memory +pids` in the unit cgroup **and** in
  `runs/`, creating `runs/`, and the janitor all belong to the separate setup
  slice (stage E).
- Bridge preflight = `config.execution_preflight(layout, &NativeResolver)`,
  then the stored construction result, then the setup-slice readiness flag.
  It runs per request in the worker, before the busy check and before
  creation, exactly where `Native::preflight` runs today. `Pin` is unchanged
  and stays independent: the dispatcher still computes the match.

## 2. Request → `RunSpec` (pure)

| Request | `executable` | `arguments` | `cwd` | `timeout` | `output_bytes` |
| --- | --- | --- | --- | --- | --- |
| `{executable: p, args, cwd}` | `layout.binary()` | `exec-child`, `path`, `p`, `cwd`, args… | empty | `timeout_s` s | request |
| `{tool: n, args, cwd}` | `layout.binary()` | `exec-child`, `tool`, `n`, `cwd`, args… | empty | `timeout_s` s | request |
| `None` (discovery) | `layout.binary()` | `tool-probe` | empty | 60 s | 4,096 |

`Request::validate` has already bounded the agent part. `validate_spec`
accepts the ADR 0019 D5 allowance. If `validate_spec` still fails, that is
`NotStarted::Invalid`, row F. *Deviation, C1:* the allowance lives only in
`validate_phased_spec`, which `run_phased` calls. It is the exact `exec-child`
prefix, and the tail keeps the ADR 0007 bound. Legacy `validate_spec` and
`run` are unchanged.

## 3. Runner error sites → `RunError` → `Report`

Every `?` and `return Err` in `Runner::run_phased`, `unix::run`, `account` and
`Group::create` gets exactly one class. The bridge maps each class with an
exhaustive `match` (no wildcard).

| Site (`07ae3c89`) | Class | Report | Row | Quarantine |
| --- | --- | --- | --- | --- |
| `mod.rs:136` `validate_spec` | `NotStarted::Invalid` | `Failed { NotInvoked }` | F | no |
| `mod.rs:141-148` non-Unix | `NotStarted::Unsupported` | `Unavailable(PlatformUnsupported)` | D | no |
| `unix.rs:142-146` not root | `NotStarted::ServiceIdentity` | `Unavailable(MechanismUnavailable)` | D | no |
| `unix.rs:65` account C string | `NotStarted::AccountPolicy` | `Unavailable(PreflightFailed)` | D | no |
| `unix.rs:78-82` lookup failed or missing | `NotStarted::AccountPolicy` | `Unavailable(PreflightFailed)` | D | no |
| `unix.rs:84-86` UID/GID changed | `NotStarted::AccountChanged` | `Unavailable(AccountIdentityChanged)` | D | no |
| `unix.rs:91-101` supplementary groups | `NotStarted::AccountPolicy` | `Unavailable(PreflightFailed)` | D | no |
| `unix.rs:113-117` primary group | `NotStarted::AccountPolicy` | `Unavailable(PreflightFailed)` | D | no |
| `unix.rs:122-130` and `463-478` private roots | `NotStarted::PrivateRoots` | `Unavailable(PreflightFailed)` | D | no |
| `unix.rs:169-181` macOS descriptor limit | `NotStarted::Containment` | `Unavailable(MechanismUnavailable)` | D | no |
| `cgroup.rs:35-114` every `Group::create` error | `NotStarted::Containment` | `Unavailable(MechanismUnavailable)` | D | no |
| `unix.rs:186` `socketpair` | `NotStarted::Resource` | `Failed { NotInvoked }` | F | no |
| moved recheck, shutdown | `NotStarted::Shutdown` | `Unavailable(Stopping)` | D | no |
| moved recheck, cancel | `NotStarted::Cancelled` | `CancelledBeforeSpawn` | E | no |
| `unix.rs:305` `command.spawn()` Err | `SpawnFailed` | `Failed { Unknown }` | G | **yes** |
| `unix.rs:319` identity read | `AfterSpawn` | `Failed { Created }` | G | **yes** |
| `unix.rs:322-326` identity mismatch | `AfterSpawn` | `Failed { Created }` | G | **yes** |
| `unix.rs:327-338` pipe take, nonblocking | `AfterSpawn` | `Failed { Created }` | G | **yes** |
| `unix.rs:342-343`, `352` drain, `waitid` | `AfterSpawn` | `Failed { Created }` | G | **yes** |
| `unix.rs:363`, `368-369` reap, final drain | `AfterSpawn` | `Failed { Created }` | G | **yes** |
| panic caught around the runner call | — | `Failed { Unknown }` | G | **yes** |

What the source can and cannot prove:

- A `NotStarted` class is provable: each site returns before `spawn`. The one
  residue is `Group::create` ignoring a failed `unlinkat` (`cgroup.rs:85`,
  `109`). That can leave an empty leaf with no process, for the janitor. It is
  not a creation, and the report claims nothing about it.
- `SpawnFailed` cannot be split. Pipe creation (inside `spawn`), fork, the
  `pre_exec` steps (several of which run as root before the ID drop) and exec
  all arrive as one `Err`. The child-side errno does not say which step failed,
  or whether the cgroup join happened. So no containment or cleanup claim is
  possible: `Unknown`, never `ProvenNone`.
- `AfterSpawn` means a child exists. On the identity-read and mismatch sites
  the target image may already be running. `ChildGuard` drop cleanup is best
  effort and its result is discarded (`unix.rs:431-435`). The report carries
  no cleanup, boundary, observation, stop or exit fact. A mismatched UID or GID
  never leaves the runner.
- No observation never means no child: `SpawnFailed` and `AfterSpawn` are
  never mapped to `NotInvoked`.
- The panic catch wraps `run_phased` only, with `AssertUnwindSafe`. It returns
  `Failed { Unknown }` so the row finishes and quarantines, instead of staying
  `started`. The workspace profile has no `panic = "abort"` at `07ae3c89`.
  *Deviation, ADR 0019 C2 P7:* the catch wraps the bridge's whole per-run body
  (spec derivation, `run_phased`, mapping, discovery parse), not only
  `run_phased`. A caught panic maps to `Failed { Unknown }` with its payload
  dropped unread. The catch classifies; it does not sanitize (§7a).

## 3a. Shared private failure and exact legacy errors (ADR 0019 C1)

`run` and `run_phased` both call `run_core`. Its error type is the private
`RunFailure { phase, legacy }`, with no `From` impls, and each site builds it
explicitly. The table shows the value legacy `run` must still return (as at
`1d828ca7`) and the class that `run_phased` returns.

| Site | Legacy `ExecError` (unchanged) | `RunError` |
| --- | --- | --- |
| `mod.rs:136` spec check | `Invalid("absolute executable and workspace-relative cwd required" \| "argv bounds" \| "timeout/output bounds")`, from legacy `validate_spec` only | `NotStarted::Invalid`, from `validate_phased_spec` |
| `mod.rs:144` non-Unix | `Unavailable("execution backend unavailable on this OS")` | `Unsupported` |
| `unix.rs:143` not root | `Unavailable("root service identity required; no owner fallback")` | `ServiceIdentity` |
| `unix.rs:65` | `Invalid("account")` | `AccountPolicy` |
| `unix.rs:79` | `Unavailable("pinned account missing or lookup failed")` | `AccountPolicy` |
| `unix.rs:85` | `Unavailable("pinned identity changed")` | `AccountChanged` |
| `unix.rs:98` | `Unavailable("supplementary group membership forbidden")` | `AccountPolicy` |
| `unix.rs:114` | `Unavailable("dedicated account-named primary group required")` | `AccountPolicy` |
| `unix.rs:122`, `125` | `OperatingSystem(errno)` | `PrivateRoots` |
| `unix.rs:129` | `Unavailable("private directory is not root-only")` | `PrivateRoots` |
| `unix.rs:149` early check, and the new final recheck | `Unavailable("run already cancelled or shutting down")` | `Shutdown` if either shutdown token is set, else `Cancelled` |
| `unix.rs:172`, `176` (macOS) | `OperatingSystem(errno)`, `Unavailable("descriptor limit exceeds safe inheritance closure")` | `Containment` |
| `cgroup.rs:35-114`, every `Group::create` error | its existing value, unchanged | `Containment` |
| `unix.rs:186` | `OperatingSystem(errno)` | `Resource` |
| `unix.rs:305` | `OperatingSystem(errno or -1)` | `SpawnFailed` |
| `unix.rs:319`, `337-338`, `342-343`, `352`, `363`, `368-369` | `OperatingSystem(errno or -1)` | `AfterSpawn` |
| `unix.rs:323` | `Unavailable("child identity attestation mismatch")` | `AfterSpawn` |
| `unix.rs:331`, `336` | `Unavailable("stdout pipe")`, `Unavailable("stderr pipe")` | `AfterSpawn` |

- `RunError` and `NotStarted` derive `Debug, Clone, Copy, PartialEq, Eq` over
  fixed variants only. `RunFailure` derives nothing and is never formatted.
- The legacy deviations are exactly the three in ADR 0019 C1: the
  race-window refusal, the merged `Cancellation` identity, and panic payload
  text. Existing `exec/tests.rs` and `exec/disposable.rs` tests stay
  unmodified. `disposable.rs` stays ignored and unrun.
- Legacy `run` never receives a caller shutdown token. It passes `None`.

## 4. `RunResult` → `Completed` / `Discovered`

| `Evidence` field | Value |
| --- | --- |
| `observation` | `Unix { uid: identity.uid, gid: identity.gid }`. That is the child-sent value; the bridge never copies the pin. |
| `point` | `PreExec` |
| `stop` | `Exited → Exited`, `TimedOut → Timeout`, `Cancelled → Cancelled`, `Shutdown → Shutdown` |
| `exit_code` | `result.exit_code` only when `outcome == Exited`, otherwise `None` |
| `tree_boundary` | `LinuxCgroupV2 → LinuxCgroup`; `MacProcessGroupBestEffort → ProcessGroup`, never `MacUidSweep` |
| `cleanup_confirmed` | `Some(result.tree_cleanup_confirmed)`, copied unchanged |

- **Exit code with a non-exit stop.** The loop checks stop reasons before
  leader exit (`unix.rs:344-354`). So a target that exits at the deadline can
  return `TimedOut` with `Some(code)`, which ADR 0017 makes row K. The bridge
  reports the runner's stop decision and withholds the incidental code. That
  withholds an observed fact and invents none. Pin it with a test.
- **`Exited` without a code** (the shim itself was killed, or the 2 s reap
  window, `unix.rs:360-367`, expired) stays row K and quarantines. That is
  honest; do not map it to anything else.
- **Output.** Move `exec::CapturedOutput` fields into
  `development::CapturedOutput` without reading, trimming, measuring or
  formatting them. Release stays the dispatcher's job (ADR 0017). The streams
  hold only bytes from the observed child's pipes (the shim and its
  descendants). The bridge adds nothing. Note: `dropped_bytes` counts
  discarded bytes that were **read**. Bytes still unread after the bounded
  final drain (`unix.rs:368-369`) are not counted, so a released count is a
  lower bound. Document this; do not change it here.
- **Discovery.** Parse tools only when the stop is `Exited`, the code is 0,
  `stdout.dropped_bytes == 0`, and `head ++ tail` is valid UTF-8 with exactly
  `TOOLS.len()` lines that each decode as `{name, found, version}` in table
  order. Otherwise return `Discovered { tools: vec![] }`, which the dispatcher
  turns into row K (invalid table). Ignore stderr unread. Tools leave only on
  row N (ADR 0017).

## 5. Shim (`arxd exec-child`, `arxd tool-probe`)

- Dispatch both in `main` before the tokio runtime and before any config,
  layout or state access, with exact argv shapes. They write no stderr text,
  refuse euid 0 (code 125), and are Unix-only. On Windows they are absent.
- Panic silence (ADR 0019 C2 P6): the first statement of each shim
  subcommand installs one no-op panic hook. That is safe because the shim is
  its own single-threaded process and the hook is never restored. The rest of
  the body runs under `catch_unwind`, and a caught panic exits `125`. The
  service process never does this.
- `exec-child <path|tool> <target> <cwd> <args…>`: open `cwd` beneath `.` (Linux
  `openat2` `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`; macOS one
  `openat(O_DIRECTORY | O_NOFOLLOW)` per component), then `fchdir`. An empty
  cwd stays put. The lexical rules match `Request::validate`. Then:
  - `path`: `Command::new(<absolute target>)`;
  - `tool`: `<target>` must be in `TOOLS`. Try `entry/target` for at most 64
    absolute, clean `PATH` entries in order. Skip empty and relative entries.
    `ENOENT`/`ENOTDIR` means continue; `EACCES` is remembered; any other error
    gives 126.
  - stdin null, stdout and stderr inherited, environment inherited (it is
    already exactly the owner environment), no PATH search (absolute program
    only), wait, then exit with the code or `128 + signal`.
  - Codes: 125 bad argv, cwd refused or missing, or euid 0; 126 found but not
    startable; 127 not found.
- `tool-probe`: `chdir("/")`. For each `TOOLS` name, resolve as above and run
  `--version` with stdin null, stdout piped (4 KiB cap), stderr null and a 5 s
  timeout (kill and reap on expiry). Print one line
  `{"name":…,"found":…,"version":…}`. The version is the first stdout line,
  trimmed, kept only if it is 1–128 printable ASCII bytes without `/` or `\`,
  otherwise `null`. Total output stays below 4,096 bytes by construction.
  `sudo -n true` (ADR 0009 `rights_mismatch`) is **not** in this slice. It
  stays with ADR 0009 slice 9.

## 6. Cancellation, shutdown, joining

- Unify the two `Cancellation` types (re-export). Pass the executor's `cancel`
  and `shutdown` straight to `run_phased`. The runner polls them every 5 ms
  after the identity read, never between `spawn` and the end of that read.
- Build the runner once, inside the bridge. Never construct or drop a `Runner`
  per run. Never pass `Execution`'s token to `Runner::new`.
- Joining is unchanged: `Execution::drain` signals shutdown and joins the
  `spawn_blocking` workers. The runner's cleanup is bounded (cgroup kill wait
  up to 2 s, reap up to 2 s, then bounded drains). A stalled synchronous
  `spawn`, NSS lookup or filesystem call has no deadline (ADR 0007 limit); the
  10 s drain budget then fails as ADR 0009 describes.

## 7. Sanitation

- No `ExecError`, `RunSpec`, `RunResult`, `OwnerConfig`, cgroup path, account
  name, workspace or errno reaches a `Report`, a row, a response, a log or a
  panic message. `RunSpec` and `RunResult` derive `Debug` (args, cwd, output,
  UID/GID), so the bridge must never format them. Reviewers check this, and
  tests check the bytes (T8).
- No `os_code` is added. ADR 0009 allows one, but `ExecutionUnavailable` has
  none today and this slice does not need it.

## 7a. Panic diagnostics (ADR 0019 C2)

- The default hook prints the payload to stderr before `catch_unwind`
  returns. On Linux that output reaches the journal (*source inference*). On
  macOS it reaches `/Library/Logs/Arxcovo/arxd.log`. Sanitation therefore
  means the payload is fixed where it is produced, not where it is caught.
- P1–P7 apply to the panic-reachable set: production code of
  `exec/{mod,unix,cgroup,output}.rs`, the bridge module and the shim module.
  Bridge tests live in a separate file. Each file may have at most one
  trailing `#[cfg(test)] mod tests`, and the scan stops there.
- Stage B rewrites the six `Result::expect` sites (`unix.rs:22`, `160`, `167`,
  `320`, `321`; `cgroup.rs:72`) as `let … else { unreachable!("<same
  literal>") }`, or a panic-free equivalent. Behaviour on reachable paths does
  not change.
- The fake panics used in tests are test code and may format markers on
  purpose (T10 negative control). The scan never covers test files.
- Not claimed: absence of private data in std, tokio or dependency payloads,
  in implicit panics (indexing, overflow, char boundary), in aborting panics,
  in backtraces beyond T10's fakes, or in real logs. All native paths remain
  **unverified on real hardware**.

## 8. Tests (all unprivileged; fakes and owned temp directories only)

Bridge (any OS, through a `#[cfg(test)]` `Execution` constructor and a fake
`PhasedRunner` port):

- **T1 derivation:** every row of the table in section 1, including refusals
  and a missing workspace path that still constructs.
- **T2 cgroup parser:** accepts one supervisor line. Refuses each listed
  malformed form and inputs over 4,096 bytes.
- **T3 spec:** three request shapes. Argument fidelity for `-x`, `--`, empty
  and Unicode args. 128 args of 512 bytes each plus a 4,096-byte path and cwd
  pass `validate_phased_spec`. A spec still over its limit is row F.
- **T4 phase matrix:** each `RunError` variant through real `dispatch`. Check
  row D, E, F or G, the quarantine flag, the stored reason, no
  `stdout`/`stderr` keys, and `list_activity`. A panicking fake gives G,
  quarantine and a finished (not `started`) row.
- **T5 result mapping:** rows N, M, L (including `TimedOut` with `Some(0)`
  → L), H (cleanup false; output still released on a full match), K (`Exited`
  with no code), I (a fake returns another UID: both IDs absent everywhere,
  output withheld), and macOS boundary → `process_group` → H.
- **T6 discovery:** a valid table → N with tools. `dropped_bytes > 0`, wrong
  order, an extra or missing line, a bad version, or a non-zero exit → K with
  no tools. Row I → no tools.
- **T7 cancellation:** the executor's token stops a blocking fake. Drain
  returns a `shutdown` stop, and the worker is joined. Dropping the bridge's
  per-run state never sets `Stopping`. A fake that sees cancel at the moved
  recheck → E.
- **T8 sanitation:** put marker strings in the account name, workspace,
  environment values and cgroup path. Fail at every class. The markers and
  errno text are absent from responses, rows and database bytes.
- **T9 inertness:** `Hooks::Native` / `Execution::configured` still reports
  `mechanism_unavailable` for Linux, macOS and Windows layouts with a fully
  valid fake resolver. The bridge constructor exists only under `cfg(test)`.
- **T4b catch scope (C2 P7):** a fake port returns a valid `RunResult`. A
  `#[cfg(test)]` seam then panics inside result mapping and, separately,
  inside discovery parsing. Each case gives G, quarantine and a finished row.
- **T10 diagnostics (C2; any OS):** the parent test re-executes the test
  binary with `--exact <fixture> --ignored --nocapture`. Precedents are
  `store/crash_tests.rs:89`, `service/tests.rs:355` and
  `exec/disposable.rs:38`. The child gets a cleared environment plus a fixture
  flag. The parent runs it twice, once with `RUST_BACKTRACE` unset and once
  with `RUST_BACKTRACE=1`, and reads the child's stdout and stderr bytes. The
  fixture drives real `dispatch` through the bridge with T8's markers (account,
  workspace, environment values, cgroup path, request args and cwd). It covers:
  (a) every `RunError` class; (b) results whose UID and GID are the marker
  value `3141592653` and whose output holds a marker, both matched and
  mismatched; (c) a fake that panics with the fixed literal
  `ARX-T10-FIXED-PANIC`; (d) the T4b seams with distinct fixed literals; (e) a
  negative control fake that panics with `panic!("{}", marker)`.
  The parent requires:
  - every marker except (e)'s is absent;
  - each fixed literal appears exactly once per panicking invocation (the
    default hook only; the bridge does not re-emit it);
  - (e)'s marker **is present**. This shows the hook prints payloads and that
    the test can see a leak;
  - the child exits successfully.
- **P-SCAN payload inventory (C2 P1/P6; any OS; pure source test):** use a
  comment-stripping tokenizer, following `code_tokens` at
  `service/owner_windows_tests.rs:757`, over the panic-reachable files via
  `include_str!`. The test must reject:
  - the method calls `.expect(`, `.unwrap(`, `.unwrap_err(` and `.expect_err(`;
  - `assert_eq!`, `assert_ne!`, `debug_assert_eq!` and `debug_assert_ne!`;
  - `dbg!`, `panic_any`, `resume_unwind`, `print!`, `println!`, `eprint!` and
    `eprintln!`;
  - `take_hook`, and `set_hook` anywhere except exactly once in the shim file;
  - any `impl From<…> for RunFailure`.

  Every `panic!`, `unreachable!`, `assert!`, `debug_assert!`, `todo!` and
  `unimplemented!` message must be one string literal without `{`. The
  multiset of (file, macro, literal) must equal a checked-in inventory, so a
  new site needs a reviewed inventory change. A self-test feeds a synthetic
  `panic!("{x:?}")` and a synthetic `.expect(` to the checker and requires
  rejection. The inventory must also be non-empty, so the scan cannot pass
  vacuously.

Runner (exec, unprivileged):

- **R1:** non-root `run_phased` returns `NotStarted::ServiceIdentity`. A marker
  executable proves no child ran. Invalid spec → `Invalid`.
- **R2:** a pure `final_recheck(cancel, shutdown)` gives shutdown precedence.
  The phase helpers (`before`, `spawn`, `after`) tag every site. The existing
  `Runner::run` tests stay unchanged.
- **R2 extension (C3):** `recheck(cancel, shutdown)` and
  `shutdown_requested(runner, caller)` are pure. Test all eight combinations
  of (cancel, runner shutdown, caller shutdown). The result is `None` only
  when all three are false, `Shutdown` whenever either shutdown is set, and
  `Cancelled` otherwise. The legacy value is always the existing literal.
- **L1 (Unix, non-root; skipped when euid is 0):** `run` returns exactly
  `Err(ExecError::Unavailable("root service identity required; no owner
  fallback"))`, and `run_phased` returns `NotStarted::ServiceIdentity`. A
  marker executable proves that no child ran.
- **L1b (any OS):** three invalid specs (bad cwd, 129 args, zero timeout). For
  `run`, each returns its own exact legacy `Invalid` literal. For
  `run_phased`, all three return `NotStarted::Invalid`. No reverse mapping
  from `RunError` can produce three different values, so this kills that
  mutant.
- **L2 (pure, any OS):** legacy `validate_spec` refuses an `exec-child`
  four-item prefix plus 128 agent args, and a prefix plus a 65,536-byte tail.
  `validate_phased_spec` accepts both. `validate_phased_spec` refuses:
  - a 129-item tail;
  - a 65,537-byte tail behind a one-byte target and empty cwd (the case a
    blanket allowance would let through);
  - `arguments[1]` outside `{path, tool}`;
  - a 4,097-byte target or cwd;
  - an `exec-child` spec with fewer than four items;
  - a NUL in a prefix item;
  - a 132-item spec without the prefix.
- **L2b (legacy entry point, any OS; Unix non-root only):** construct a
  Runner with the existing pure valid OwnerConfig fixture. Call the actual
  legacy `Runner::run`, not just either validator, with the valid four-item
  shim prefix from L2 and (a) 128 agent arguments, and separately (b) eight
  8,192-byte arguments (65,536 tail bytes). The second case stays within the
  legacy item count and individual argument bound but exceeds its aggregate
  byte bound once the nonempty prefix is included. Require exact
  `Err(ExecError::Invalid("argv bounds"))` in both cases before backend work.
  No marker executable may run. On Unix skip this entry-point/mutation test
  when euid is 0, as in L1; under the wrong-validator mutation an ordinary
  user reaches only ServiceIdentity refusal before cgroup/spawn, while a
  non-Unix platform reaches only Unsupported. Never run that mutation as
  root. Changing only legacy `run` to call `validate_phased_spec` must compile
  and fail L2b's exact-error assertion. Keep L2's direct boundary matrix.
- **L3 (non-Unix):** `run` returns exactly
  `Unavailable("execution backend unavailable on this OS")`, and `run_phased`
  returns `NotStarted::Unsupported`.
- The root-only sites (cgroup, spawn, post-spawn) are **not** reachable
  unprivileged. They need the reviewed disposable experiment. That covers the
  per-site legacy values, both recheck placements, and caller-shutdown
  polling in the real loop. Until then, the explicit `RunFailure` construction
  at each site (no `From` impl) and review of the §3a table are the only
  checks.

Shim (current user, owned temp directories, Unix CI):

- **S1:** the exit code propagates. Stdout and stderr hold only target bytes.
- **S2:** a missing target gives 127; non-executable gives 126; a missing cwd,
  a symlinked cwd component, `..` or an absolute cwd gives 125. No output in
  any case.
- **S3:** a target killed by `SIGKILL` gives 137.
- **S4:** a relative or empty `PATH` entry is never used, absolute entries are,
  only the first 64 are read, and a non-`TOOLS` name gives 125.
- **S5:** euid 0 refusal, through an injected euid in a unit test. Skip the
  process test when CI runs as root.
- **S6:** `tool-probe` with fake tools: a slash in the version gives `null`, a
  10 s sleeper is killed at 5 s with `found: true`, order is fixed, output is
  under 4,096 bytes, and the probe's cwd is `/`.
- **S7 (C2 P6):** in a re-executed test-binary subprocess, never in the
  parallel runner, because the hook is process-global: a shim entry with an
  injected panicking `#[cfg(test)]` step returns 125, and the subprocess
  stderr contains no panic text.

## 9. Mutations to record (each must fail a named test)

`SpawnFailed → NotInvoked` (T4) · `Cancelled → Stopping` (T4) · swapped recheck
precedence (R2) · exit code kept on `Timeout` (T5) · macOS boundary →
`MacUidSweep` (T5) · cleanup forced `true` (T5) · discovery ignores
`dropped_bytes` (T6) · runner opens the request cwd instead of the shim
(T3/S2) · panic catch removed (T4) · protected dirs empty or missing state
(T1) · shim path mode falls back to a PATH search (S4) · shim follows a cwd
symlink (S2) · signal mapped to 1 (S3) · `Execution::configured` builds the
bridge (T9) · `ExecError` text in a response (T8) · parser accepts a missing
`supervisor` (T2) · probe stays in the workspace (S6). `AfterSpawn → Unknown`
is an equivalent mutant: both are row G.

Added by C1–C3:

- legacy errors rebuilt from `RunError` (L1b);
- legacy `run` uses `validate_phased_spec` (L2b entry-point assertion);
- phased validator uses a blanket +4 items / +8,206 bytes allowance instead
  of the exact prefix (L2);
- caller shutdown ignored by `shutdown_requested` (R2);
- recheck precedence swapped (R2);
- `impl From<std::io::Error> for RunFailure` (P-SCAN);
- `.expect(` restored at `unix.rs:160` (P-SCAN);
- `panic!("{spec:?}")` in the bridge (P-SCAN and T10);
- bridge `eprintln!`s or re-panics the payload (P-SCAN; T10 count of 2);
- catch scope narrowed back to `run_phased` (T4b);
- shim hook removed (S7);
- `set_hook` added in the bridge (P-SCAN).

Two placement mutants are root-only and are **not** catchable unprivileged:
the early check removed, and the final recheck moved before cgroup creation.

## 10. Staging (one concern per PR; none activates anything)

- **A:** this ADR and brief, then a fresh Sol review.
- **B (Sol):** runner `run_phased`, `NotStarted`, moved recheck, external
  shutdown, shared `Cancellation`, argv allowance. R1, R2, existing tests.
  Opus review. *Clarified:* also `RunFailure` and `run_core` (C1), the kept
  early check plus the final recheck (C3), `validate_phased_spec`, the six
  payload rewrites, L1–L3, the R2 extension, and P-SCAN over the four exec
  files.
- **C (Sol):** `exec-child` and `tool-probe`. S1–S6. Opus review.
  *Clarified:* also the shim hook and catch, S7, and the shim file in P-SCAN.
- **D (Sol):** the bridge module and the fake port, with a `cfg(test)`
  constructor only. T1–T9 and the mutations. Production composition is
  unchanged. Opus review. *Clarified:* also the P7 catch scope, T4b, T10, and
  the bridge file in P-SCAN. The constructor passes `Execution::native`'s
  `configured_rights` parameter (present at `1d828ca7`).
- **E (separate design and review; not authorized here):** Linux hierarchy
  setup (unit and `runs/` controllers, creating `runs/`), the janitor, startup
  discovery wiring, and ADR 0009 slice 9 rights checks.
- **F (separate review; not authorized here):** diagnose the `E_PROTECTION`
  failure of 43091339273 first. Then a disposable experiment design covering
  the root-only sites, cleanup and the drop-in effects below.
- **G activation:** only after the full ADR 0017 review, independent
  implementation reviews of B–E, green hosted gates and an accepted F. Linux
  standard only. macOS and Windows stay `mechanism_unavailable`.
  *Clarified:* also ADR 0019 G-P1 (the dispatch-path panic payload audit),
  G-P2 (the release profile stays unwinding) and G-P3 (a separate reviewed
  diagnostic ADR is required if an unfixable data-bearing payload is
  reachable).

## Open facts (unknown until observed)

- Every native Linux and macOS behaviour: the root-only sites, cgroup
  migration under the drop-in's capability set, `cgroup.kill`, leaf removal,
  and output and timing. The only experiment failed before acceptance, at an
  unknown site and with unknown cleanup.
- Whether `ProtectHome=yes` and `PrivateTmp=yes` reach runner children
  (source inference: yes). If so, the workspace, `HOME`, `CARGO_HOME` and
  `RUSTUP_HOME` must sit outside `/home`, `/root` and `/run/user`, and
  `/tmp` is shared across runs until restart. `docs/dev-host-setup.md` and
  `docs/service-install.md` should say so once confirmed.
- Whether std's internal assertions in `spawn` can panic on the pinned
  toolchain (handled as `Unknown` either way).
- Whether startup discovery should run before the janitor confirms an empty
  `runs/`. Stage E decides.

## Coordinator correction after the independent clarification review

Sol high review of b68a73eb requests one B1 correction: T3 now names the
phased validator, and L2b exercises the actual legacy entry point so its
wrong-validator mutation cannot survive a validator-only test. The old direct
L2 matrix remains. This is a test-contract correction, not an implementation
or new API decision. Fresh Opus recheck of the Sol correction is required.
The contract remains proposed, with every inactive/activation gate retained.

The independent reviewer checked the Rust 1.95 primary documentation. String
slice diagnostics may contain a truncated string excerpt as well as a
character; P4 must cover both. The fixed-payload rules and G-P1–G-P3 remain
binding and establish no absence claim for dependencies, aborts or real logs.
