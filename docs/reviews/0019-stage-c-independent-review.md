# ADR 0019 Stage C independent review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-12. Review sub-agent,
no subagents. This review covers only the Sol Stage C implementation
(`exec-child`, `tool-probe`, the shim hook and catch, S1–S7 and the P-SCAN
extension), so it can count as the different-provider review of that work.
Opus wrote the ADR 0019 architecture. This review claims no independence for
that contract.

**Standards: CHANGES REQUESTED.** F1 is two Unix-only Clippy `-D warnings`
failures. The pinned 1.95 Clippy reproduces both for Linux and macOS, and
they would fail the required hosted CI. **Spec: PASS** for the Stage C scope,
pending hosted Linux/macOS execution of S1–S7. None of S1–S4, S6 or S7 was
run by anyone. This review grants no bridge, backend activation, cgroup
setup, experiment or M2 acceptance. Native, installed and dedicated paths
remain **unverified on real hardware**. M2 remains incomplete.

## Pins

- Worktree: the runtime-prepared T3 worktree, branch `t3/e1066381`. Its top
  level matched the working directory and is not the canonical checkout.
  Status was clean at exact HEAD `cc876abd4d051c0369cd6b180adee863ca661ce6`.
  No checkout or worktree was created, and nothing was written to the
  canonical checkout.
- Target `cc876ab` (docs only: PROGRESS and the evidence report). Source
  `656dde3c4c221b0048487a738ede5f0782f078c5`. Base
  `c372865f3365ef8d8baf02e32044d607eef9d2b3`, whose parent is actual main
  `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0` (#52, `origin/main`). Base and
  main differ only in `docs/PROGRESS.md` (11 added lines).
- `git diff c372865 656dde3` touches only `src/{main,lib,unix_shim,
  unix_shim_tests}.rs`, `src/exec/stage_b_tests.rs` and `tests/unix_shim.rs`
  under `crates/arxd` (712+/7−). Every committed blob matches the evidence
  manifest. `exec/{mod,unix,cgroup,output,disposable}.rs`, service, config,
  development, the owner CLI, Cargo/lockfile and CI are unchanged.
  `656dde3..cc876ab` touches only PROGRESS and the evidence report.

## Standards

Sources: AGENTS.md, KICKOFF, ADR 0019 C2/P6 and the smell baseline.

### F1 (Medium, blocking publication): Unix-only Clippy `-D warnings` failures

- `crates/arxd/src/unix_shim.rs:28-31`: `clippy::manual_unwrap_or`. The match
  on `catch_unwind(body)` with `Ok(code) => code, Err(_) => 125` reimplements
  `Result::unwrap_or`.
- `crates/arxd/src/unix_shim.rs:276-278`: `clippy::unused_unit`. The
  rustfmt-expanded arm body `{ () }` has an unneeded unit expression.
- Why Windows missed them: the whole body is `#[cfg(unix)]`, so the builder's
  Windows all-target Clippy never type-checked it.
- Reproduction (type analysis only, no execution): a scratch crate with the
  committed `unix_shim.rs`, `unix_shim_tests.rs` and `tests/unix_shim.rs`
  bytes, the repository `Cargo.lock` and `rust-toolchain.toml`, a minimal
  main containing the exact inserted Unix route, and type-level stubs for
  `development::TOOLS` and `service::executor::Request`. I ran
  `cargo clippy --offline --target {x86_64-unknown-linux-gnu,
  aarch64-apple-darwin} --all-targets -- -D warnings` with the pinned 1.95.0
  toolchain. Both targets fail with exactly these two errors. Both std
  targets were already installed. No C build was needed, and nothing was
  installed or downloaded.
- Correction: `std::panic::catch_unwind(body).unwrap_or(125)` and an empty
  arm body `{}`. In the scratch copy, these two edits make the lib, bin,
  lib-test (S5, S7, `version_validation`) and integration-test targets
  Clippy-clean on both targets, with no further warnings. `unwrap_or` is a
  separate token, which P-SCAN already accepts (`unix_shim.rs:81`), so the
  inventory does not change. After the fix, rerun Windows fmt, Clippy and the
  focused P-SCAN tests. Hosted Linux/macOS Clippy is the authority.

### F2 (Low, advisory): P-SCAN residual lexical forms

The six F2 forms from the Stage B review are now rejected. Each is a negative
control, and the focused tests pass (see credits). A temporary probe test
(run `de1d08e4-40fd-4572-934f-58761b1ab855`, file restored to blob
`a2cb8da7`) shows that `scan_with_hook` still **accepts** these forms:

- `use super::RunFailure as F; impl From<std::io::Error> for F {}`. A `use`
  alias still opens the C1 implicit-conversion hole. Only `type` aliases are
  caught (`stage_b_tests.rs:370-377`).
- `write!(std::io::stderr(), "{:?}", x)` and
  `std::io::stderr().write_all(..)`. Only `writeln` is banned.
- `use std::panic::set_hook as quiet;` followed by two calls passes the
  shim's exactly-one count, because it counts tokens, not call sites.
- `serde_json::json!(..)`. The macro expands to `to_value(&x).unwrap()`
  (serde_json 1.0.145 `macros.rs:279`), which no lexical scan can see. The
  shim does use it (`unix_shim.rs:224`).

Audit of the actual shim production code: it has no
`unwrap`/`expect`/`unwrap_err`/`expect_err` call or function value, no raw
identifier, no logging macro or alias, no type or `use` alias, no stderr
write and no second hook. Stdout is written only once, through
`stdout().lock().write_all` in `probe` (`:233`). The hidden `json!` unwrap
cannot fail for `&str`, `bool` or `Option<String>`, and it is silenced by the
shim hook and caught as 125. The evidence (`:101-104`) honestly scopes the
scan as lexical. Recommended before Stage D adds the bridge file: reject a
`RunFailure` token followed by `as`, reject `set_hook` followed by `as`, and
reject the `io :: stderr` path in the scanned files. A green P-SCAN is still
not semantic privacy proof.

### F3 (Low, advisory, privacy hygiene): local path in evidence

`docs/evidence/unix-shim-stage-c.md:13` records the builder's absolute
worktree path, which embeds the owner's OS account name. Eleven earlier committed
docs share this pattern, so this is not new, but AGENTS/KICKOFF forbid private
owner data in commits. Use a placeholder such as `<T3 worktree>`. A sweep of
the older docs belongs in a separate PR.

### Smell baseline (judgement calls)

- Possible **Duplicated Code**: `clean_directory` (`unix_shim.rs:125-133`)
  re-derives the lexical rules of `absolute_executable` (`executor.rs:56-78`),
  with a deliberate difference (it allows a trailing `/`). Acceptable as is.
  A shared helper would stop the two from drifting.
- Possible **Mysterious Name**: the test helper `ordinary()`
  (`tests/unix_shim.rs:42`) means "euid is not 0". Something like
  `non_root()` would be clearer.
- No other smell applies. The `BoundedOutput` extension trait exists only to
  allow builder chaining in tests, which is fine.

## Spec

Sources: brief §5, §7a, §8 S1–S7 and P-SCAN, §9 and §10 C; ADR 0019 D2 and C2
P6; ADR 0007; `Request::validate`.

| Requirement | Observation |
| --- | --- |
| Dispatch before runtime, config, layout and state; Unix only | `main.rs:5-11` is the first statement after argv collection, before the service, owner and Tokio paths. It is `#[cfg(unix)]`, and lib `#[cfg(any(unix, test))]`. On Windows the routes are absent. Owner routes are byte-unchanged after it. |
| Exact shapes; malformed forms → silent 125 | Routing is by prefix. `body` requires exactly `["tool-probe"]`, or `exec-child` with at least 4 items and mode `path` or `tool`. Anything else under the prefix returns 125. Note: `exec-childX` and `tool-probeX` used to fall through to the foreground runtime. They now exit 125 on Unix, which matches the evidence and is benign. |
| Hook is the first statement of the private common entry; the catch covers identity, parsing and probe | `silent_entry` (`:26-32`, private) runs `set_hook` first, then `catch_unwind`. `entry` (`:21-23`) only builds the closure. `geteuid`, `with_identity`, `body`, `probe` and every spawn run inside the catch. A caught panic returns 125. The hook is never restored. The service and foreground paths never install it (P-SCAN: zero hooks in the four exec files; only main calls `entry`). |
| euid 0 → 125 before any operation | `with_identity` (`:15-17`) is the gate production uses. S5 injects 0 with a panicking step (M3 killed). |
| exec-child validation | Reuses the real `service::executor::Request::validate`: TOOLS or an absolute lexical executable, 128 items / 8,192 bytes / 65,536 bytes, NUL, cwd lexical rules (no absolute path, no `.` or `..`). Non-UTF-8 argv → 125 (`:42-45`). |
| cwd open | Linux `openat2(AT_FDCWD, cwd, O_DIRECTORY\|O_CLOEXEC, RESOLVE_NO_SYMLINKS 0x04 \| RESOLVE_BENEATH 0x08)`, the same constants and struct as the reviewed runner (`unix.rs:563-594`). macOS uses one `openat(O_DIRECTORY\|O_NOFOLLOW)` per component from `.`. Both then `fchdir`. An empty cwd stays put. |
| Path mode: absolute only, no PATH fallback | `Command::new(<validated absolute>)`. std does no PATH search for a program that contains `/`. ENOENT/ENOTDIR → 127, anything else → 126. |
| Tool mode | TOOLS only. Manual walk over `take(64)` before filtering, so skipped entries count. Empty, relative, non-UTF-8 and unsafe entries are skipped. ENOENT/ENOTDIR continue, EACCES is remembered (126 at the end), any other error → 126 immediately, and not found → 127. |
| Child I/O and exit | stdin null, stdout/stderr inherited, environment inherited, `wait`, then the code or `128 + signal`. A wait error or a status with neither → 126. The shim itself writes nothing. There is no account lookup, identity drop or service context. |
| Probe | `chdir("/")` first, then TOOLS in table order through the same resolver. `--version` runs with stdin null, stderr null and stdout piped non-blocking. The probe reads at most 4,096 bytes per tick and keeps at most 4,096 bytes. The 5 s deadline still applies after the leader exits if the pipe is held. The `ProbeChild` guard kills and reaps on every return and on unwind; the pipe is dropped before the guard. The version is the trimmed first line, 1–128 bytes, printable ASCII, no `/`, `\` or NUL. The worst case is about 312 bytes per line, or about 2.8 KB for nine lines. An explicit aggregate check (`> 4096` → 125) runs before the single write. `found` is true only if the tool started. There is no sudo probe. |
| Honest limits | The evidence (`:67-69`) states that blocked spawn, filesystem calls and kill/reap have no deadline, and that descendants are not contained. The worst-case probe time is 9 × 5 s ≈ 45 s plus spawn latency, inside the 60 s discovery timeout from brief §2. |
| S1–S4, S6 | They test the current user's own temp directories and skip before spawning when root. S1 checks target-only bytes and exit 23, stdin EOF even while the parent holds a piped stdin, inherited env, cwd, and `-x`/`--`/empty/Unicode args. S2 checks 127/126, the six cwd refusals, the malformed prefix and mode forms, a relative target and argv bounds. S3 checks 137. S4 checks that `:.:relative` is skipped even though `cwd` contains `cargo` (a meaningful negative), unlisted → 125, no fallback in path mode, the 63/64 boundary, `..`, EACCES and continuation. S6 checks slash and NUL versions, the exec-replaced 10 s sleeper killed in 5–9 s with `found: true` and its PID reaped (`kill(pid, 0)` → ESRCH; signal 0 only), `PWD = /`, table order, the 5,000-byte first line → null and ≤ 4,096 bytes. The watchdog kills only its own still-unreaped process group. |
| S5 | Pure injected euid 0, on every OS. Killed M3 (below). |
| S7 | The normal parent re-runs only the purpose-built ignored `s7_panic_fixture` with `env_clear`, with `RUST_BACKTRACE` unset, `1` and `full`. It requires exit 125, empty stderr and no marker on stdout. It skips when root. The parallel parent never sets a hook. A wrong filter would exit 0, so the test cannot pass vacuously. Removing the hook would leave the marker on stderr; removing the catch would make the child exit 101. |
| P-SCAN extension | Shim added. `set_hook` must appear exactly once in the shim and zero times in the other four files (M1 killed). Logging identifiers are rejected anywhere (M2 killed). The fixed inventory is the four non-empty Stage B literals; the shim adds no panic site. Only a trailing test module is excluded, and anything after it is refused. |
| Brief §9 C2 mutation "shim hook removed (S7)" | Killed on Windows by P-SCAN (M1). The behavioural S7 kill is **unrun**. |

Spec advisories (not blocking):

- **S-A1 test gap:** no S2 case passes a non-UTF-8 argv item (e.g.
  `OsStr::from_bytes(b"\xff")`) to cover `:42-45`. There is also no lexical
  check that only `main.rs` references `unix_shim::entry`, which would keep a
  future service caller from installing the process-global hook. P6 relies on
  review for that today.
- **S-A2 Stage D note:** serde_json has no `preserve_order`, so lines come out
  as `{"found":…,"name":…,"version":…}`, not the brief's literal key order.
  The Stage D parser must decode JSON fields and must not byte-match.

## Credits and limits

Own runs (Windows, focused, at the target). Each mutation was restored and
its blob rechecked (`unix_shim.rs` `a071edce`, `stage_b_tests.rs`
`a2cb8da7`). Status was clean after each.

| Run | Scope | Result |
| --- | --- | --- |
| `b9928059-4ac9-4e70-8578-ead1204c6be7` | `test(p_scan) \| test(unix_shim::tests)`, unmodified | 4 passed, 305 skipped |
| `de1d08e4-40fd-4572-934f-58761b1ab855` | temporary F2 probe test | 5 residual forms accepted (F2) |
| `6a653fbc-c523-469e-afa8-0a7482771a84` | M1: shim `set_hook` removed | killed by `p_scan_checked_nonempty_inventory` |
| `b572ef3b-42ff-4d0b-b1bb-38428b7f01d6` | M2: `eprintln!` added in `silent_entry` | killed by `p_scan_checked_nonempty_inventory` |
| `4d7baa28-e74d-43e9-a0b4-f21bddd2fa78` | M3: `with_identity` ignores the uid | killed by `s5_injected_identity_refuses_root` |
| scratch Clippy, Linux and macOS | committed shim and tests, pinned 1.95 | 2 errors each (F1); clean with the F1 fix |

- The builder's runs (`5a120636…` focused, and the one full workspace run
  `be7440d0-0f61-4a24-8201-90a42bf1f5c5`, 308 passed / 2 skipped, 10.608 s,
  at `656dde3`) are credited to the builder as recorded. I did not repeat
  them, and their logs are not in this worktree. I ran no full suite.
- **No Unix compile of the arxd crate, no Unix execution and no Unix mutation
  ran.** The scratch Clippy is type analysis with stubs, not acceptance. S1–S4,
  S6 and S7 are authored and unrun. Unix mutation-kill credit is 0, including
  PATH fallback, cwd symlink following, signal → 1, catch removal and the
  probe staying in the workspace. Hosted Linux and macOS are mandatory.
- Not exercised: production NativeOps, create, Guard, session, native
  preflight, store, the account resolver, cgroups, the root runner, fixed
  system paths, service, SCM, LSA, privilege, accounts, VM, Lab Manager,
  experiments, private state or any existing ignored fixture. Nothing was
  pushed, and no PR, watch, merge, rerun, settings or babysitter operation was
  performed. ExecutionFailed and E_PROTECTION remain unknown, with no fix or
  waiver.

## Verdict

Standards: **CHANGES REQUESTED**. F1 must be fixed before publication. F2,
F3 and the smells are advisory. Spec: **PASS** for Stage C, pending hosted
Linux and macOS. After F1, a short recheck of that two-line correction, by a
reviewer from a different provider than the one who makes it, is enough. A
PR for hosted verification is then fine. All five required checks must be
green at the exact head before the coordinator merges. Stages D–G, the
bridge, activation, cgroup setup and experiments remain **not authorized**.
Native, installed and dedicated paths remain **unverified on real
hardware**, and M2 is incomplete.
