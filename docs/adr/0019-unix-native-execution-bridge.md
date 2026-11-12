# 0019: Unix native execution bridge (runner phases and build-account shim)

Status: **accepted inactive contract; implementation waits for this contract
PR to merge through independent exact-head review and all required hosted
gates**. Coordinator OpenAI / gpt-6.1-sol / medium, 2026-11-12, main after
S3 #50 (`4f01fca`). This disposition accepts the original Opus architecture
with the retained independent Sol high reviews, and the Sol validator-test
correction with its retained independent Opus high recheck. See the
[correction recheck](../reviews/0019-validator-correction-opus-recheck.md).

This accepts the staged design only. It grants no production constructor,
native execution, owner operation, experiment retry or M2 acceptance.
Stages B-D still require separate implementation reviews and green CI;
cgroup setup/cleanup and the E_PROTECTION diagnosis remain separate gates.
The existing no-retry rule and every activation gate remain in force.

## Historical proposal and clarification checkpoints

Status: **proposed** (Anthropic / claude-opus-5-5 / high, architecture,
2026-11-11, base `07ae3c89`). A fresh independent Sol review is required
before implementation. ADR number 0018 is reserved for the concurrent owner-CLI
design; this ADR does not use it.

Clarified 2026-11-12 (same provider and model, so not an independent review)
on main `1d828ca7`, docs parent `d9b6fa30`. The
[Sol review](../reviews/0019-unix-bridge-sol-review.md) passed the inactive
design and raised advisories B-A1 (legacy error compatibility) and B-A2 (panic
diagnostics). [Contract clarifications](#contract-clarifications-2026-11-12)
C1–C3 settle them. D1–D7 stand, except for the labelled sentences in D1, D4
and D5. The status stays **proposed**. A fresh independent Sol review of the
clarified ADR and brief must pass before implementation. See the
[disposition](../reviews/0019-contract-clarification-disposition.md).

This ADR adds no native backend to production. `Execution::configured` stays
`MechanismUnavailable` on every OS. Native, installed and dedicated M2 paths
remain **unverified on real hardware** and incomplete. Nothing here was built,
run or tested. Statements about Rust std, the kernel or systemd that were not
checked against a primary source are labelled *source inference*.

The implementation brief is
[0019-unix-native-bridge-implementation.md](../briefs/0019-unix-native-bridge-implementation.md).

## Context

ADR 0017 (accepted for local implementation; its full-validator review is still
running) says the dispatcher maps a typed `Report`. A native adapter must prove
a creation outcome at every failure site. ADR 0009 says Linux and macOS use ADR
0007's `Runner` "unchanged in principle". It also says `run_command {tool}` is
re-resolved "inside the shim as the build account", and discovery runs
`arxd tool-probe` as the build account. Reading the source at `07ae3c89` shows
five mismatches that block a correct bridge.

1. **The runner's errors carry no phase.** `Runner::run` returns
   `ExecError::{Invalid, Unavailable(&str), OperatingSystem(i32)}`.
   `OperatingSystem` comes from sites before creation (`fstat`, cgroup setup,
   `socketpair`), from `Command::spawn` (`unix.rs:305`), and from sites after
   the child exists (`read_exact`, `drain`, `waitid`, `try_wait`;
   `unix.rs:319-369`). A bridge reading `ExecError` alone can only claim
   `Unknown` for all of them, or match on message strings. String matching is
   not a contract. Claiming `Unknown` everywhere turns ordinary per-run refusals
   (account changed, not root, no cgroup) into host quarantine.
2. **There is no Unix shim, and direct exec makes typos quarantine.** The
   runner opens the cwd and execs the target inside `pre_exec`
   (`unix.rs:289-295`, `305`). A missing cwd or executable fails in the child,
   so `spawn` returns `Err`. ADR 0017 classifies that as `Unknown` (row G,
   quarantine until restart). One mistyped `cwd` from any agent would disable
   execution on the host. `tool` cannot work at all: the service may not
   resolve `PATH`, and the runner requires an absolute executable.
3. **Cancellation and shutdown do not connect.** The executor passes
   `development::Cancellation`. The runner takes a different
   `exec::Cancellation` type and polls only its own internal shutdown token
   (`mod.rs:136-140`). The executor's shutdown token cannot reach a running
   child.
4. **The cancel recheck is not the last step before creation.** The runner
   checks cancellation (`unix.rs:148`) before cgroup creation and `socketpair`.
   ADR 0017 requires the final recheck immediately before the primitive. The
   check also merges cancel and shutdown into one string.
5. **macOS cannot meet the required boundary.** The runner reports
   `MacProcessGroupBestEffort` with `tree_cleanup_confirmed = false` always.
   ADR 0017 requires `mac_uid_sweep`. Every macOS run would be row H and
   quarantine. The ADR 0009 sweep, pre-run check and janitor do not exist.

## Decision

### D1. Typed runner phases (small ADR 0007 API addition)

`Runner` gains `run_phased`. It returns `Result<RunResult, RunError>`. Its
behaviour, fences, order and cleanup are those of `run`, except for D3 and D4.
`run` stays as a wrapper that maps `RunError` back to `ExecError`, so existing
callers and tests keep their meaning.

```rust
pub enum RunError {
    /// The creation primitive was not called.
    NotStarted(NotStarted),
    /// `Command::spawn` returned Err: a failed fork, a created child whose
    /// pre_exec failed, or a failed exec. Not distinguishable.
    SpawnFailed,
    /// `Command::spawn` returned a child; a later step failed. Drop cleanup is
    /// best effort and its result is not reported.
    AfterSpawn,
}
pub enum NotStarted {
    Invalid, Unsupported, Cancelled, Shutdown, ServiceIdentity,
    AccountChanged, AccountPolicy, PrivateRoots, Containment, Resource,
}
```

- Each site gets a fixed class at the point of failure. No variant carries a
  string, path, errno, UID or GID. `ExecError` text never crosses the bridge.
- `SpawnFailed` exists because std reports pre-fork pipe errors, fork errors,
  child-side `pre_exec` errors and exec errors through one `Err`. It is
  `Creation::Unknown`, as ADR 0017 already says. This ADR claims nothing about
  std reaping the child.
- `AfterSpawn` is `Creation::Created`. A richer evidence report (ADR 0017 row
  J) is allowed by 0017 but not adopted. It would need proof that the child
  joined the cgroup, and the gain (J instead of G, both quarantine) is small.

*Deviation, C1:* the sentence above that has `run` map `RunError` back to
`ExecError` cannot hold. `RunError` has no text or errno, so that reverse
mapping loses data. Both entry points instead wrap one private core whose
error carries both the phase and the exact legacy `ExecError`. The guarantee
that existing callers and tests keep their meaning is kept, with the three
labelled exceptions in C1.

### D2. A fixed build-account shim for every Unix run

Every dedicated Unix run executes the fixed `Layout::binary()` through the
runner. The agent's target is never the runner's executable.

- `run_command {executable}`: argv `["exec-child", "path", <exe>, <cwd>, args…]`.
- `run_command {tool}`: argv `["exec-child", "tool", <name>, <cwd>, args…]`.
- `discover_tools`: argv `["tool-probe"]`.
- `RunSpec.cwd` is always empty (the workspace root). The runner still opens
  the workspace root after the ID drop, with its existing no-symlink rules.
- The shim runs after exec as the build account. It inherits the observed
  identity, the cgroup (Linux), the process group, no-new-privileges (Linux),
  the cleared capabilities, the core limit, umask `077`, `/dev/null` stdin, the
  runner's stdout/stderr pipes and the owner environment.
- `exec-child` opens `<cwd>` beneath its current directory with the runner's
  rules (Linux `openat2` `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS`; macOS
  `openat(O_NOFOLLOW)` per component), then `fchdir`. `path` mode spawns the
  absolute path. `tool` mode resolves the fixed-table name against the
  absolute, clean entries of the owner `PATH` (at most 64). It skips empty and
  relative entries, so there is no current-directory lookup. It spawns with
  stdin null and stdout/stderr inherited, waits, and exits with the target's
  code, or `128 + signal` when the target died from a signal.
- `exec-child` writes no bytes of its own. It reports its own failures only as
  fixed exit codes: `125` (bad shim argv, cwd refused or missing, euid 0), `126`
  (target found but could not be started), `127` (target or tool not found).
  These are real exit statuses of the observed child. They can collide with a
  target's own `125`–`127` or `≥ 128`. A row M (`failed`, `exited`, code) stays
  truthful about the process the runner observed.
- `tool-probe` changes directory to `/` before probing, so no workspace file
  (`rust-toolchain.toml`, `.cargo/config.toml`) can influence discovery. It
  prints exactly one JSON line per `TOOLS` entry, in table order.
- Both subcommands refuse euid 0, dispatch in `main` before any runtime is
  built, and read no configuration, layout or state.

Why: agent mistakes (cwd, executable, tool) move after the observation and
become row M, not quarantine. `SpawnFailed` and `AfterSpawn` are left for host
or install faults, where quarantine is correct. This also matches the Windows
contract (ADR 0009 step 8: the shim opens the cwd and runs the target with
`std::process::Command`). It does not weaken ADR 0007. The cwd is still opened
after the ID drop, by the same identity, with the same kernel rules. The
spawn-and-wait shape keeps signal deaths out of ADR 0017 row K
(`exited` without a code), which would otherwise quarantine on every crashing
build.

### D3. One cancellation type; the caller's shutdown reaches the runner

`exec::Cancellation` and `development::Cancellation` become one type (a
re-export; no behaviour change). `run_phased` takes the caller's request
cancellation and the caller's shutdown token. It also keeps polling the
runner's own shutdown token. Either shutdown wins over cancel, as in
`stop_reason`. The bridge never shares a token with `Runner::drop`, so dropping
a runner can never put `Execution` into `Stopping`.

### D4. The final recheck moves to immediately before `spawn`

The cancel/shutdown recheck moves after cgroup creation, `socketpair` and
`Command` construction, immediately before `command.spawn()`. It reports
`NotStarted::Shutdown` before `NotStarted::Cancelled`. Between `spawn` and the
end of the identity read, the runner already polls nothing (`unix.rs:305-326`),
as ADR 0017 requires.

*Deviation, C3:* the existing check at `unix.rs:148` is **kept**, not moved,
and the final recheck is **added** before `spawn`. The guarantees of D4 do not
change.

### D5. Argv allowance for the shim prefix

ADR 0007's argv bound stays the agent bound (128 items, 65,536 bytes, enforced
by `Request::validate`). `validate_spec` allows exactly the fixed prefix on top:
at most 4 more items and 8,206 more bytes (10 + 4 + 4,096 + 4,096). Each item
stays at most 8,192 bytes, and the executable path is at most 4,096.

*Deviation, C1:* only `run_phased` applies the allowance, and only to the
exact four-item `exec-child` prefix. The agent tail keeps the ADR 0007 bound
on its own. Legacy `run` keeps the unchanged ADR 0007 `validate_spec`. As
written, a total of 132 items and 73,742 bytes would have let a short target
carry more than 65,536 agent bytes.

### D6. Platforms

- **Linux standard** is the only target of this bridge.
- **Linux admin** stays unavailable (`mechanism_unavailable`). `Runner::new`
  refuses `admin`, and ADR 0009's admin group and sudoers handling is not
  implemented.
- **macOS** stays unavailable (`mechanism_unavailable`). Lifting this needs
  the ADR 0009 `mac_uid_sweep` extension (a new `TreeBoundary`, the
  real-or-effective UID pre-run check, the post-run sweep and the janitor),
  separately reviewed. The bridge must never map `MacProcessGroupBestEffort` to
  `mac_uid_sweep`. macOS admin stays refused at config, start and every
  preflight (ADR 0009, ADR 0017 `Pin::new`).
- **Windows** is untouched. This ADR designs no Windows backend. S4U failed
  twice; no third probe is planned, and there is never a password fallback.

### D7. Staging; nothing is activated

The bridge is test-constructible only (`#[cfg(test)]` constructor and seams).
No production selector, environment variable, config key or CLI flag can enable
it. Activation is a separate, reviewed change. It requires: the full ADR 0017
review closed; independent implementation reviews of D1–D5 and the bridge;
green hosted gates; the separately reviewed Linux cgroup setup and janitor
slice; and an accepted, separately reviewed disposable native experiment. The
first experiment (43091339273) failed with `E_PROTECTION` before harness
acceptance. Its failure site and cleanup state are unknown, and no retry is
authorized.

## Contract clarifications (2026-11-12)

These settle Sol advisories B-A1 and B-A2 (credited to the
[Sol review](../reviews/0019-unix-bridge-sol-review.md)). They are
requirements for stages B–D and G. Nothing here was built or run. The
[brief](../briefs/0019-unix-native-bridge-implementation.md) §3a, §7a, §8 and
§9 give the exact tests and mutations.

### C1. Exact legacy errors through one private core (B-A1)

The exec module adds one private type: `RunFailure { phase: RunError, legacy:
ExecError }`. It is never `pub`, derives nothing, and has no `From` impl to or
from `std::io::Error`, `ExecError` or `RunError`. So every fallible site must
name its class explicitly. It has two constructors: `new(class, ExecError)`,
and `os(class, io::Error)`, which builds `legacy` through the existing
`ExecError::from` (`mod.rs:101-104`). The legacy errno value is therefore
byte-identical, including `-1` when there is no raw OS error.

- `Runner::run_core(spec, cancel, caller_shutdown: Option<&Cancellation>) ->
  Result<RunResult, RunFailure>` holds the one implementation. `unix::run` and
  `account` return `RunFailure`. The `Group::create` and `trusted_root` call
  sites map with `RunFailure::new(NotStarted::Containment, e)` and
  `RunFailure::os(NotStarted::PrivateRoots, e)`. `cgroup.rs` is unchanged.
- `run(spec, cancel)` runs the unchanged `validate_spec`, then `run_core(spec,
  cancel, None)`, mapping errors with `|f| f.legacy`. Its signature, error
  values (variant, `&'static str`, errno), site order, argv bounds, cleanup,
  `Runner::new`, `shutdown_handle` and `Drop` stay as they are at `1d828ca7`.
- `run_phased(spec, cancel, shutdown)` runs `validate_phased_spec` (C1/D5),
  then `run_core(spec, cancel, Some(shutdown))`, mapping errors with
  `|f| f.phase`. A validation failure is `NotStarted::Invalid`. The legacy
  value is dropped inside `run_phased`. No string, errno or ID crosses it.
- `exec::Cancellation` becomes `pub use crate::development::Cancellation`
  (D3). It has the same path, the same derives (`Clone`, `Default`) and the
  same methods.

Legacy `run` changes observably in exactly three ways. Each is labelled:

1. **Race-window refusal (D4).** Suppose cancel or shutdown becomes visible
   after the existing early check and before `spawn`. `run` now returns
   `Err(Unavailable("run already cancelled or shutting down"))`, an existing
   value. Before, it started a child and returned `Ok` with outcome
   `Cancelled` or `Shutdown`. No existing test asserts the old behaviour.
2. **Merged `Cancellation` type identity (D3).** This only adds; no legacy
   code stops compiling.
3. **Panic payload text** at the six unreachable `Result::expect` sites (C2).
   The panic itself is kept. Only its message loses the error's `Debug` text.

No other legacy deviation is allowed. A lossy reverse mapping, a shared
validator, or giving legacy `run` the shim allowance would each fail the brief's
L1–L3.

`validate_phased_spec` keeps the common executable, cwd, timeout and output
checks unchanged. Suppose `arguments[0] == "exec-child"`. Then it needs at
least four items, `arguments[1]` in `{path, tool}`, `arguments[2]` of 1–4,096
bytes, `arguments[3]` of at most 4,096 bytes, and no NUL. The unchanged
ADR 0007 argv rule then applies to `arguments[4..]`. Any other spec gets the
unchanged rule on all its arguments; `["tool-probe"]` is one such spec.

### C2. Panic diagnostics boundary (B-A2)

**Facts.** These come from the primary std docs as served at
doc.rust-lang.org, which rendered Rust 1.99.1. They were not checked against
the pinned 1.95.0 source, which is not installed locally.

- If a hook is set, it is invoked "before the panic is caught, before
  unwinding". `catch_unwind` "only catches unwinding panics, not those that
  abort the process".
- The default hook "prints a message to standard error and generates a
  backtrace if requested". The hook "is a global resource". `set_hook` and
  `take_hook` panic when called from a panicking thread.
- `Result::expect` panics "with a panic message including the passed message,
  and the content of the `Err`". `NulError` derives `Debug` over
  `(usize, Vec<u8>)`.

In this repository, no `set_hook` or `take_hook` call exists in
`crates/arxd/src`. The Linux unit sets no `StandardError=` (journal by systemd
default; *source inference*). The macOS plist sends stderr to
`/Library/Logs/Arxcovo/arxd.log`. So any service panic message becomes a
persistent log line. ADR 0017 retention rule 6 and ADR 0007 ("does not … log
argument/environment values") already forbid private data there. A caught
panic that maps to `Failed { Unknown }` is classification only. **It is not
sanitation.**

**Boundary (normative).** The *panic-reachable set* is the production code of
`exec/{mod,unix,cgroup,output}.rs`, the bridge module and the shim module.
Code after a trailing `#[cfg(test)] mod tests` is excluded.

- **P1. Fixed payloads.** `panic!`, `unreachable!`, `assert!`,
  `debug_assert!`, `todo!` and `unimplemented!` take one plain string literal
  with no `{` as their message. These are forbidden: the method calls
  `.expect(`, `.unwrap(`, `.unwrap_err(` and `.expect_err(`; `assert_eq!`,
  `assert_ne!` and their `debug_` forms; `dbg!`; `panic_any`;
  `resume_unwind`; `print!`, `println!`, `eprint!` and `eprintln!`. Their
  messages carry `Debug` output of operands or errors.
- **P2. No formatting** of `RunSpec`, `RunResult`, `ObservedIdentity`, either
  `CapturedOutput`, `OwnerConfig` or `ServiceConfig` fields, `Pin`, paths,
  account names, environment values, errno or a caught payload into any panic,
  log or stream.
- **P3. Payload dropped unread.** No downcast, no formatting and no logging.
- **P4. No range indexing on `str` or `String`** for agent, config or output
  text. The std char-boundary message includes the offending character (1.99.1
  docs; not verified for 1.95.0). Use byte slices or `str::get`.
- **P5. The `pre_exec` closure stays panic-free.** It is panic-free today at
  `unix.rs:200-302`.
- **P6. No hook in the service process.** No `set_hook` or `take_hook`, and
  never one swapped per run. The shim is a separate, short-lived process. It
  installs one no-op hook as its first statement, runs its body under
  `catch_unwind`, and exits `125` on a caught panic. This keeps D2's "writes no
  bytes of its own" true.
- **P7. Catch scope.** The bridge's `Executor::run` wraps its whole per-run
  body in one `catch_unwind(AssertUnwindSafe(..))`. That body is spec
  derivation, `run_phased`, result mapping and discovery parsing. `Err(_)`
  maps to `Failed { Unknown }`, so the row finishes as G and quarantines.
  Bridge preflight stays outside the catch; no child can exist there. This is
  a deviation from brief §3 "wraps `run_phased` only". A mapping panic would
  otherwise leave the row `started` (`executor.rs:296-316`).

Stage B must fix the six existing `Result::expect` sites (`unix.rs:22`, `160`,
`167`, `320`, `321`; `cgroup.rs:72`). Use `let … else { unreachable!("…") }`
with the same literal, or a panic-free equivalent. `unix.rs:22`, `160` and
`167` would print workspace, protected-directory, cgroup or request-cwd bytes.
They are unreachable only while `clean_path` refuses NUL (`mod.rs:164`,
`cgroup.rs:38`).

**What this proves, and what it does not.** The brief's P-SCAN and T10 can
show three things. Owned panic-reachable code carries only fixed payloads. The
bridge and dispatch path add no marker bytes to stdout or stderr for the faked
classes. The default hook does print payloads; a negative control shows it.
They do not show the absence of private data in std, tokio or dependency
payloads, in implicit panics, in aborting panics, under a non-default hook,
or in the root-only native paths, or in real journald or log contents. All of
those stay open, and native paths remain **unverified on real hardware**.

**Activation gate additions (stage G).**

- **G-P1.** Audit every explicit panic site on the Linux standard dispatch
  path outside the scanned set: executor dispatch, `complete`, `assess` and
  `output_value`, the store writes used by `complete`, `Execution::preflight`,
  and `ServiceConfig::execution_preflight` with `NativeResolver`. Record each
  site's payload class. Any data-bearing payload is fixed under P1, with the
  file added to P-SCAN, or activation stops.
- **G-P2.** The release profile must stay unwinding (no `panic = "abort"`). The
  test suite cannot show this (*source inference*: Cargo ignores the profile
  panic setting for tests).
- **G-P3.** Suppose a reachable data-bearing payload sits in code this
  repository cannot change. Activation then stops until a separate,
  independently reviewed service diagnostic ADR exists. It is limited to one
  hook installed once in `service::run` before the runtime, never swapped or
  restored, with fixed text only, unchanged exit and restart semantics, and
  its own subprocess test. This ADR neither designs nor authorizes it.

### C3. Recheck placement (D4)

One pure helper, `recheck(cancel, shutdown) -> Option<RunFailure>`, runs at
two sites. Here `shutdown` is the runner's token or the caller's token. The
first site is the existing check after `account` (`unix.rs:148`). The second
is a new final recheck immediately after `Command` construction and
immediately before `command.spawn()`. Shutdown wins: the phase is
`NotStarted::Shutdown`, otherwise `NotStarted::Cancelled`, and the legacy
value at both sites is the existing literal. Keeping the early check preserves
legacy order exactly. It also avoids a root `mkdirat` and removal of a cgroup
leaf for a request that is already dead. The poll loop uses the same
`shutdown_requested(runner, caller)` helper.

## Consequences

- One more ADR 0007 public type and method. `Runner::run` keeps its semantics
  and exact error values, except for the three labelled C1 deviations.
- The shim is new trusted code that runs as the build account. A bug in it can
  only act with build-account rights. Its integrity relies on the root-owned
  binary directory (ADR 0009 protection rules), as on Windows.
- Exit codes `125`–`127` and `128 + n` are ambiguous between the shim and the
  target. That is documented, not hidden.
- Pre-creation refusals map to rows D, E or F and do not quarantine. Spawn and
  post-spawn errors are row G and quarantine, as ADR 0009 and ADR 0017 require.
- A missing workspace root, an unreadable shim binary or any failed `pre_exec`
  step still quarantines. These are owner or install drift.

## Relation to other ADRs

- **ADR 0007:** additive API (D1, D3), reordered recheck (D4) and argv
  allowance (D5). Identity, capability, descriptor, cwd, cgroup and cleanup
  fences are unchanged.
- **ADR 0009:** makes the Unix "shim" and `tool-probe` concrete (D2). It
  deviates from "Runner unchanged in principle" only in that the runner execs
  the fixed shim, never the agent target. Tool discovery semantics, timeouts
  and output caps are unchanged. The ADR 0009 cgroup setup sentence is
  incomplete: the runner (`cgroup.rs:57-66`) also requires `+memory +pids` in
  `runs/cgroup.subtree_control`, not only in the unit cgroup. The setup slice
  must enable both.
- **ADR 0017:** no change. This ADR supplies the creation classification for
  each Unix site. If the running 0017 review changes the `Report` or `Creation`
  contract, this ADR must be rechecked first.

## Sources and inference

Repository sources read at `07ae3c89`: `crates/arxd/src/exec/{mod,unix,cgroup,output,tests,disposable}.rs`,
`crates/arxd/src/service/{executor,config,preflight,application,layout}.rs`,
`crates/arxd/src/development/{mod,types}.rs`, `crates/arxd/src/main.rs`,
`packaging/linux/arxd.service` and its standard-rights drop-in, and ADRs 0007,
0009, 0014, 0015 and 0017. Nothing was run, and no external source was fetched
for this ADR.

Clarification re-pin, `1d828ca7`. Between `07ae3c89` and `1d828ca7` the
following are byte-identical: `exec/`, `development/`, `main.rs`,
`service/{config,preflight,layout}.rs`, `packaging/`, `Cargo.toml`,
`rust-toolchain.toml` and ADRs 0007 and 0009. So every line cited above still
holds.

- `service/executor.rs` adds only `configured_rights`, and
  `Execution::native` gains that parameter. The `#[cfg(test)]` bridge
  constructor must pass it. `Report`, `Creation`, `Executor` and dispatch are
  unchanged.
- `service/application.rs` adds only store exit classification.
- ADR 0017 adds only status checkpoints. Its contract text is unchanged, and
  its validator merged as #42 (`4c2264d`).

C2 quotes the std docs at doc.rust-lang.org (1.99.1). The fetched URLs held no
repository or owner data.

Source inference, not verified:

- That std `Command::spawn` with a `pre_exec` closure forks (not
  `posix_spawn`), sends child-side errors over an internal CLOEXEC pipe, and
  returns them as `Err`. This only supports classifying `Err` as `Unknown`.
- That `ProtectHome=yes` and `PrivateTmp=yes` in the standard-rights drop-in
  apply to runner children through the inherited mount namespace. If true, the
  workspace and toolchain must live outside `/home`, `/root` and `/run/user`,
  and `/tmp` is one private directory that all runs share until restart.
- That cgroup v2 migration of the child into `runs/<leaf>` needs only the
  root-owned file permissions inside the service's unit cgroup under the
  drop-in's `CapabilityBoundingSet`.

## Clarification-review correction checkpoint — 2026-11-12

Independent Sol high review of b68a73eb passes Standards but requests B1 in
the test contract. The coordinator corrects T3's validator name and requires
L2b to exercise legacy Runner::run at its item/aggregate-byte boundary,
including the wrong-entry-point mutation. Review and correction are retained
in the accompanying reports. This proposal remains unaccepted pending a
fresh independent recheck; no implementation or activation is authorized.

The reviewer verified the panic facts against pinned Rust 1.95 documentation,
including that slicing diagnostics can include a truncated string excerpt.
Earlier author's 1.99 evidence remains historical. These source facts do not
prove absence of private data in dependency/implicit/aborting panics or logs.
