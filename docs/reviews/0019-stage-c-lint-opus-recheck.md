# ADR 0019 Stage C Unix lint correction recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-12. Review sub-agent,
no subagents. This recheck covers only the Sol coordinator's correction
`fed4036` of the Sol Stage C implementation, so it counts as the
different-provider review of that correction. Opus wrote the ADR 0019
architecture and the prior Stage C review (`e374e02`, retained at `cb67dc4`).
This recheck claims no independence for that architecture.

**Standards: PASS.** F1 is closed. F2 and F3 stay advisory, and the
correction discloses them accurately. **Spec: PASS** for Stage C, unchanged
from the prior review and still pending hosted Linux/macOS build and
execution. Native, installed and dedicated paths remain **unverified on real
hardware**. M2 remains incomplete.

## Pins

- Worktree: the runtime-prepared T3 worktree, branch `t3/57c09ebd`. Its top
  level matched the working directory and is not the canonical checkout.
  Status was clean at exact HEAD `fed4036c1d6de166e71ae9d0caa360dec24486b2`.
  No checkout or worktree was created, and nothing was written to the
  canonical checkout.
- Target `fed4036`, base `cb67dc4` (retains review `e374e02` of `cc876ab` /
  source `656dde3` / base `c372865`). Actual main is `1604db1` (#52).
- `git diff cb67dc4 fed4036` touches four files: `crates/arxd/src/unix_shim.rs`
  (2+/8−), PROGRESS, the evidence report (one line) and the new follow-up
  evidence.
- `git diff --name-only 656dde3 fed4036` lists only `unix_shim.rs` among the
  source files. `main.rs`, `lib.rs`, the runner, `exec/stage_b_tests.rs`
  (scanner), `unix_shim_tests.rs`, `tests/unix_shim.rs`, Cargo/lockfile and CI
  are byte-for-byte the same as in `656dde3`. The `unix_shim.rs` blob at the
  target is `ca4a7729`.

## Standards

Sources: AGENTS.md, KICKOFF, ADR 0019 C2/P6, prior review F1–F3 and the smell
baseline.

### F1: closed

- `silent_entry` (`unix_shim.rs:26-29`): `set_hook` is still the first
  statement, and `std::panic::catch_unwind(body).unwrap_or(125)` follows it.
  The closure and the catch scope are unchanged: `entry` still passes
  `with_identity(geteuid, body(args))`, so identity, parsing, probe and every
  spawn stay inside the catch. `Ok(code)` → `code` and `Err(payload)` → fixed
  `125`. The payload is dropped in the same frame as before, so behavior is
  identical. No hook restore was added.
- `probe_version` (`:269-272`): the `WouldBlock | Interrupted` guard arm now
  has an empty block `{}` in place of `{ () }`. It still evaluates to unit and
  continues the loop. The other arms are unchanged.
- P-SCAN: the scanner bans tokens by exact match (`stage_b_tests.rs:366`,
  `expect`/`unwrap`/`unwrap_err`/`expect_err`). `unwrap_or` is a different
  token, so it is accepted, and the fixed inventory does not change. The
  focused run below confirms this on the real crate.
- Formatting: `cargo fmt --all -- --check` passes. The empty-arm form is
  rustfmt's own output.

### F2: advisory, accurately disclosed

The follow-up (`docs/evidence/unix-shim-stage-c-review-followup.md`) and
PROGRESS list the four residual lexical forms from the prior review: `use`
aliases of `RunFailure`, `write!`/`stderr().write_all`, renamed `set_hook`
imports, and `unwrap` hidden inside a macro expansion (`json!`, infallible for
the current value types). They also state that the actual shim has none of the
unsafe alias, writer or hook forms. That matches the shim at the target: it
has one `set_hook`, no `use` alias, no stderr write, and its only stdout write
is in `probe`. They also state that the scan is not a semantic privacy proof
and that Stage D must audit its new file independently of a green scan. The
recommended scanner tightening is still open for Stage D. This correction did
not have to include it.

### F3: advisory, closed for this candidate

`docs/evidence/unix-shim-stage-c.md:13` now reads `<prepared-worktree>`. A
search of the four candidate docs (the evidence report, the follow-up, the
prior review and PROGRESS) found no local OS-account path. The older
committed docs are recorded as a separate cleanup that keeps their historical
credits. That cleanup is out of scope here.

### Smell baseline

The two changed hunks are idiomatic simplifications, and no smell applies to
them. The prior advisory smells (`clean_directory`, the `ordinary()` test
name) are in unchanged code and still advisory.

### Docs note (non-blocking)

The PROGRESS heading `Historical checkpoint before Stage C Unix lint
follow-up` is followed by two blank lines. This is cosmetic.

## Spec

Sources: the Stage C brief §5, §7a, §8 S1–S7 and P-SCAN, §9 and §10 C; ADR
0019 D2 and C2/P6. The prior review's Spec table still applies, because no
routing, validation, cwd, resolver, probe, child I/O, exit mapping, test or
scanner byte changed. These requirements were rechecked against the new
bytes:

| Requirement | Observation at `fed4036` |
| --- | --- |
| The hook is the first statement of the private common entry; the catch covers identity, parsing and probe | Holds. See F1. |
| Caught panic → fixed silent 125 | `unwrap_or(125)`. The hook is still a no-op. |
| Probe non-blocking read loop: retry on WouldBlock/Interrupted, `None` on any other error | Unchanged semantics. |
| No behavior beyond the lint fix | Confirmed. The only production edits are the two hunks. |

No new requirement is missing, partial or wrong.

## Credits and limits

Own runs (Windows, at the target, with a clean worktree before and after):

| Run | Scope | Result |
| --- | --- | --- |
| `cargo fmt --all -- --check` | workspace | pass |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Windows host | pass (exit 0) |
| nextest `6809caf9-34c5-4692-9bcd-3560211547e9` | `test(p_scan) \| test(unix_shim::tests)` | 4 passed, 305 skipped |
| scratch stub Clippy, `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin` | `fed4036` shim blob `ca4a7729` plus the unchanged `unix_shim_tests.rs` and `tests/unix_shim.rs`, all targets, `-D warnings` | clean on both |
| scratch stub Clippy negative control | the same harness with the pre-fix `cb67dc4` shim | exactly the 2 F1 errors (`manual_unwrap_or` at `:28`, `unused_unit` at `:277`) on both targets |

- The stub harness is a scratch crate outside the repository. It uses the
  pinned 1.95.0 toolchain (`rust-toolchain.toml`), the repository lockfile
  and `--offline`. Its `lib.rs` contains stub `development::TOOLS` and
  `service::executor::Request` with an always-true `validate`, and its
  `main.rs` contains the exact reserved Unix route. It used only installed
  std targets and cached crates. Nothing was installed or downloaded. This is
  **type and lint analysis only**. It is not a Unix compile of the real arxd
  crate, and it is not acceptance.
- The coordinator's Windows fmt/Clippy and focused run `475526ac-bff7-45ee-
  a9b8-319b32d69a27` (4/4) are credited to the coordinator. The builder's
  full run `be7440d0-0f61-4a24-8201-90a42bf1f5c5` (308 passed / 2 skipped)
  stays credited to `656dde3`. Neither was repeated, and no full suite ran
  here.
- **No Unix compile of the real crate, no Unix execution and no Unix mutation
  ran.** S1–S4, S6 and S7 are still unrun. Unix mutation-kill credit stays
  zero. Hosted Linux and macOS Clippy and tests are the authority.
- CI context (not verified here, read from PROGRESS): main run `43240797525`
  attempt 1 at `1604db1` failed only in macOS job `131260319040`. That job
  started no steps and the runner was never acquired (macOS arm64 capacity).
  The coordinator requested one failed-job rerun. Main is not yet green. This
  does not block the review verdict, but the root merge must wait for main to
  recover and for all five gates at the candidate head.
- Not exercised: production NativeOps, create, Guard, session, native
  preflight, store, the account resolver, the root runner, cgroups, the
  service, fixed system paths or pipes, SCM, LSA, privilege, accounts, VM,
  Lab Manager, experiments, private state, any existing ignored fixture, or a
  new S7 child. No GitHub write, push, PR, merge, watch, rerun, settings or
  babysitter operation was performed, and no secret was printed.
  ExecutionFailed and E_PROTECTION remain unknown, with no fix, waiver or
  retry.

## Verdict

Standards: **PASS**. F1 is closed, and F2/F3 are advisory and disclosed.
Spec: **PASS** for Stage C, pending hosted Linux/macOS. A PR for hosted
verification of exact head `fed4036` plus this docs commit is fine. All five
required checks must be green at that exact head, and main CI must recover,
before the coordinator merges. Stages D–G, the bridge, activation, cgroup
setup and experiments remain **not authorized**. Native, installed and
dedicated paths remain **unverified on real hardware**, and M2 is incomplete.
