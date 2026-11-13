# ADR 0020 E4 compile and NNP-fence narrow recheck

Status: **B1 CLOSED and B2 CLOSED, by source analysis and scratch-stub
typecheck/lint only. The format advisory S-A1 is CLOSED. No new blocking
finding.** One new non-blocking standards finding, N1, is a stray UTF-8
double-encoding in an unrelated S1 test literal. It should be reverted before
publication.

This is a narrow code recheck. It is not acceptance. It does not approve a
merge and grants no activation. A pass here allows publication for the hosted
gates only. Real Linux compilation, Linux runtime and native mutation kills are
**still unrun**. Native paths stay **unverified on real hardware**.

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
No delegation and no subagents. The correction and its evidence are
Sol-authored (OpenAI / medium), so this is a different-provider check. Both
axes are reported separately below.

## Pinned target and scope

- Checkout: isolated worktree `t3/ea68b84c`, outside the canonical checkout.
  `git rev-parse --show-toplevel` matched the worktree. HEAD was
  `35e55bdc572c062f9ed17ca1b9f0cdc7c188ee54`, and status was clean at start.
- Base: `41bb2bb`, which retains the prior review
  `docs/reviews/0020-e4-independent-security-review.md` (from `cf5f282`).
- Reviewed `git diff 41bb2bb..35e55bd`:
  - source `cf8b9e7` changes 3 files (+255/−69):
    - `unix_shim_rights_linux.rs`
    - `unix_shim_rights_linux_tests.rs`
    - `tests/unix_shim.rs`
  - docs `35e55bd` adds one PROGRESS line and the evidence section
    "First independent-review correction".
- Unchanged since the prior review: `unix_shim_rights.rs` `b269e00`,
  `unix_shim.rs` `e70d088`, and `exec/stage_b_tests.rs`
  (`git diff --stat 41bb2bb HEAD` is empty for these).
- Correction manifest: blob IDs and SHA-256 values (from `git show HEAD:…`,
  LF bytes) match the evidence table exactly:
  - `unix_shim_rights_linux.rs` `0807785…` / `2eb9036e…3efd`
  - `unix_shim_rights_linux_tests.rs` `32d55ad…` / `ba73cff9…7024`
  - `tests/unix_shim.rs` `a26e24c…` / `83dbfc32…0297`

  No file has CRLF line endings or a BOM.

## Checks actually run (this recheck)

Host: Windows `x86_64-pc-windows-msvc`, toolchain 1.95.0, which is the
`rust-toolchain.toml` pin. The workspace has no `[lints]` table and no
crate-level lint attributes, so the stubs use the same lint set as the real
crate.

| Check | Result | What it proves |
| --- | --- | --- |
| **Stub A, NOT the real arxd crate.** Copied from the prior review's scratch stub. The three rights files at HEAD are copied verbatim (SHA-256 matches the manifest). `libc =0.2.191`, `serde_json =1.0.145` and `tempfile =3.23.0` match the real `Cargo.lock`. `cargo +1.95.0 clippy --offline --locked --all-targets --target x86_64-unknown-linux-gnu -- -D warnings` | pass (fresh target dir) | The Linux lib and the lib-test build typecheck and lint clean. This covers the `cfg(test)` and non-test `Settings` and the out-of-line `#[path]` test module. Nothing was linked or executed. |
| Stub A, same command, `--target aarch64-apple-darwin` | pass | The non-Linux path typechecks. It is not a macOS run. |
| Stub A, stub-only reverts of each B1 fix, one at a time, each restored to the verbatim bytes afterwards | each detected: `_ => return,` → `needless_return` at `_tests.rs:337`; `argv.as_ptr()` → E0308 against `gnu/mod.rs:1205`; removing `cfg_attr(not(test), derive(Default))` → E0599 in the non-test lib | The stub reaches every corrected site, in both cfg configurations. |
| **Stub B, NOT the real arxd crate.** A package named `arxd` with a `TOOLS` constant, an empty `arxd` bin, and `tests/unix_shim.rs` copied verbatim (SHA-256 `83dbfc32…`). Clippy all-targets `-D warnings`, Linux and Darwin | pass / pass | The B2 `pre_exec` fence and the `cfg(linux)` / `cfg(not(linux))` assertions typecheck and lint. This includes `bool_assert_comparison`, which does not fire on `Value`. Nothing was executed. |
| Stub B, injected `let _: u8 = libc::PR_SET_NO_NEW_PRIVS;` inside the `cfg(target_os = "linux")` block, then restored | E0308 at line 292 | The Linux-only block really is compiled by the stub. |
| `rustfmt +1.95.0 --check --edition 2024` on both rights files | pass | The 26 hunks reported in S-A1 are gone. |
| Real crate: `cargo fmt --all --check` | pass | |
| Real crate: misformatted `fn ordinary()` in `_tests.rs`, then `cargo fmt --all --check`, then `git checkout` | exit 1, then restored; status clean | cargo fmt now follows the `#[path]` module. |
| Real crate: `cargo test -p arxd --lib --locked -- s13_ s12_e4 s6_rights p_scan_ s11_` | 7 passed, 323 filtered | The portable pins, the scanner, both S13 boundaries and the P-SCAN inventory still hold after the split. |

**Not run:** the real crate's Linux or macOS check, Clippy or tests. The real
Linux blocker remains `ring`'s cross C compiler, and nothing was installed. No
WSL, no network and no code upload. No native S6, S8–S11 or S12 run happened,
and no native mutation kill. No system `sudo` and no production tool probe ran.
There was no `/proc`, `/sys`, root, account, owner, service, cgroup, VM,
private-state, experiment or ignored-fixture action. No fake executable ran,
because no non-root Linux host was available. I did not repeat the full
workspace run or the Windows workspace Clippy.

**Builder credits, accepted as stated and limited to Windows:** fmt, locked
workspace all-target Clippy, and focused 8/8 (`2d1d0b7b`). The prior review
ran 7 focused tests and I ran 7 again. The builder's eighth test is not
identified in the evidence. These credits prove no Linux path.

## Standards

### B1: CLOSED (stub level)

1. The `execveat` argv/envp pointers now use `.as_ptr().cast()`
   (`unix_shim_rights_linux.rs:313-314`). This converts `*const *const c_char`
   to the pinned glibc `*const *mut c_char`, and `execveat` does not write
   through them. `cast()` infers its target, so it would also accept a
   const-pointer signature.
2. `#[cfg_attr(not(test), derive(Default))]` sits on the field-less production
   `Settings`, and the manual `Default` is `#[cfg(test)]` (line 73). Production
   gets an empty derived default. The test default still carries the production
   constants, and `s12_production_constants` asserts them. Nit, not a finding:
   the per-field `#[cfg(test)]` inside the already `cfg(test)` impl is
   redundant.
3. `e4_nnp_helper` now ends in `_ => {}`. That is the last statement, so
   behaviour is unchanged.

The `#[path = "unix_shim_rights_linux_tests.rs"] mod tests;` declaration
resolves relative to `src/`, because it is not inside an inline module.
`use super::*` still binds `linux`. No new Clippy warning comes from the
cfg_attr, the cfg(test) impl or the module path, and stub A covers both cfg
configurations.

### S-A1: CLOSED

The diff of the test file, with whitespace and trailing commas ignored, is the
`needless_return` change plus one dropped redundant `{ }` around an
`unsafe { kill }` arm (`b'K' =>`, equivalent). Everything else is rustfmt
layout. No test logic changed.

### N1 (new, non-blocking): UTF-8 double-encoding in the S1 literals

`tests/unix_shim.rs:94` and `:100` changed from `'ü'` (bytes `C3 BC`) to
`'Ã¼'` (bytes `C3 83 C2 BC`). This is the CP-1252 reading of UTF-8, encoded
again. It is outside both review findings and the evidence does not mention
it. Both occurrences changed together, so S1 still checks that a non-ASCII
UTF-8 argument passes through byte-exact, and the test is not weakened.

Still, it is an undisclosed out-of-scope edit in a fix commit, and it points
to an edit path that re-encodes files. I found no other non-ASCII changes in
the three source files, the evidence or PROGRESS.

Fix before publication: restore the two literals to `ü` (`C3 BC`) and update
the manifest row for `tests/unix_shim.rs`. That change touches only test
bytes, so it does not need another source review. Stub B confirms that the
current bytes compile either way.

### Observation (non-blocking): stale truncation markers

`unix_shim_rights.rs:72` splits on `"#[cfg(test)]\nmod tests"`.
`stage_b_tests.rs` `e4_source_allowed` truncates at the token run
`#[cfg(test)] mod tests {`. Neither marker exists any more. Both checks
therefore scan the whole production file, which is stricter, and they pass
(the `cfg(test)` `Default` has no forbidden tokens). The behaviour fails
closed, and the out-of-line test file was never scanned, just as with
`include!` before. No action is needed. The markers can be dropped later.

## Spec

### B2: CLOSED (source analysis plus stub B typecheck)

`tests/unix_shim.rs:263-274` builds the S6 command. On Linux it registers a
`pre_exec` that calls `prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)` and returns
`Err(last_os_error())` on failure. `spawn` then fails, and `pipe_output`'s
`unwrap` panics before anything executes, so the fence fails closed. This has
three effects:

- The shim, and the system `sudo` it may run, inherit NNP. The setuid bit
  stops elevating, so `sudo -n true` cannot run as root and cannot touch the
  root-owned timestamp directory.
- An unelevated child stays user-killable, so the deadline guard is bounded
  again.
- The prior `< 9 s` budget concern is reduced, because `sudo` refuses early.

`prctl` and reading errno are async-signal-safe and do not allocate. The
pattern matches the existing R3 fence (`exec/unix.rs:258`) and S11.

The new assertions `no_new_privs == true` and `sudo != "exit_zero"` on Linux,
plus `null` / `"unknown"` elsewhere, match ADR D8 and the prior fix text. On
non-Linux Unix, `observe()` returns `(None, "unknown")` without executing
anything (`unix_shim_rights.rs:26-31`).

**No other unfenced privileged probe call in tests:**

- `tests/unix_shim.rs:124-125` (`["tool-probe","extra"]` and
  `["tool-probe-extra"]`) fail the shim's `args.len()==1 && args[0]=="tool-probe"`
  check and return 125 without probing.
- `unix_bridge/tests.rs` builds `RunSpec` values against a fake port only.
- `unix_shim_tests.rs` never calls `probe()`.
- Every unit-test `run`/`call` uses fixture `Settings` with a temporary anchor
  and the current owner:
  - `e4_path_helper` overrides `anchor`.
  - `s12_production_constants` only reads `Settings::default()`.
  - `e4_nnp_helper` only reads NNP.

**Residual, as accepted in the prior B2 text:** with NNP set, the real system
`sudo` binary is still *executed* unprivileged when CI runs S6. That is the
accepted fix option and runs no privileged probe. It has not been exercised.

### Unchanged spec state

The descriptor-bound design findings from the prior review stand, and the
other prior advisories (S-A2, S-A3, Spec A-items) are not reopened. E4 stays
unpublished and inactive, with no production caller. M2 remains incomplete.
E_PROTECTION and ExecutionFailed diagnosis stay unknown, with no experiment
retry. The known privacy flake remains unresolved and unrelated.

## Disposition

- **Standards B1:** closed at stub level.
- **Spec B2:** closed at source and stub level.
- **Format advisory S-A1:** closed.
- **N1:** revert the two S1 literals and update the manifest before
  publication. This is non-blocking and needs no new source review.

Publication may proceed for the hosted gates: fmt, Linux Clippy, nextest, and
native S6/S8–S13 with mutation kills. Merge and activation still need green
required checks, real Linux compilation and native evidence. This recheck
grants neither.
