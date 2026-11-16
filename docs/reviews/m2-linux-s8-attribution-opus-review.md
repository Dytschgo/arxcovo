# Opus review: Linux S8 attribution source

Status: **PASS, no blocking finding, 2026-11-16.** Reviewer: Anthropic /
claude-opus-5-5 / high, independent Standards/Spec/Security review sub-agent
of the Sol (gpt-6.1-sol high) builder change. Single reviewer, no children.
Docs only. No source, PROGRESS, evidence, brief, workflow or Cargo edit. No
push, PR, status, comment, merge, watch, rerun, dispatch, admin, VM, native M2,
install or compiler setup.

## Pins

| Item | Value |
| --- | --- |
| Reviewed head | `3540a28c745923acc079c7eac049bc53cbc48693` (evidence only) |
| Source commit | `70d9d06094bfcc7b255eb9a8d21932f78f6c98ff` |
| Prepared base | `f0128d6376de8d5de6a2208a91f82145cee1676e` on main `e601caa` |
| Spec | `docs/reviews/m2-linux-s8-feedback-disposition.md` (2a8f92b) plus the binding Sol contract `m2-linux-s8-feedback-sol-contract.md` |
| Standards | AGENTS.md, ADR 0019/0020 boundaries, the S13 source-boundary test, and the review smell baseline |

Scope: `git diff f0128d6 70d9d06` touches exactly two files. The shim has
+163/-0 lines and the tests have +317/-15. `70d9d06..3540a28` adds only
`docs/evidence/linux-s8-attribution.md` (+99). `git diff --check
f0128d6 3540a28` exits 0.

## Checks actually run by this reviewer

1. **Insertion-only shim.** The shim diff has 0 deleted lines. A brace-depth
   script over all 18 inserted hunks found every top-level inserted statement
   or item preceded by `#[cfg(test)]`, with 0 ungated and 0 unbalanced hunks.
   So the non-test source is byte-identical to the base: control flow, the 10
   `"unknown"` returns, the other return strings, `DEADLINE`, `Child`/`Drop`
   and the fork-child block.
2. **Fork child.** The `pid == 0` block is unchanged. The only inserted code
   that a child reaches is `#[cfg(test)] if pid != 0`, an integer compare with
   no call, allocation or attribution write. `Timing` is never dropped in the
   child because the child either execs or calls `_exit`.
3. **S13 boundary.** No forbidden token (`Command::`, `execv(`, `var(` and so
   on) appears in the production split of the new source.
4. **Pure formatter on the host.** I extracted the `attribution` module (with
   its `closed_enum!` macro, ranges 147–223) and the test formatter types and
   `s8_attribution_formatter` into a scratch crate. I built it with
   `rustc 1.95.0 --edition=2024 --test -D warnings` (allowing only the
   harness-level `dead_code` and `unused_imports`), then ran it with
   `--exact s8_attribution_formatter`: 1 passed, 0 failed, 0.01 s. A separate
   host snippet confirmed that `#[cfg(test)]` on `let`, `if` and block
   statements compiles under 2024 with and without `--test`.
   This is a host syntax and formatter check only. It is **not** a Windows
   proxy for the shim, not Linux compile evidence, and not runtime credit.
5. **Worst-case line length.** Using the longest closed labels and u32 maxima,
   the line is 245 ASCII bytes. The cap is 512.
6. **S9c hook.** After removing whitespace, the rename-hook body is
   byte-identical between base and source.

## Reused builder checks (not repeated)

- rustfmt check and `git diff --check`: exit 0.
- The builder's static audit and its formatter pass.
- The single permitted `cargo check --target x86_64-unknown-linux-gnu`
  failed after 8.52 s with ring's `x86_64-linux-gnu-gcc` ToolNotFound. It was
  not repeated. **No Linux compile, clippy or test execution exists.**

## Standards

0 findings. Judgement notes only:

- `attribute` repeats the shape of `observe`. The contract requires a separate
  thread-local (no Observation identity), so this duplication is endorsed.
- The formatter's `.min(60_000)` repeats the saturation already done by
  `ms()`. It is a deliberate bound for arbitrary record values and is tested.
- The repo's CI runs `clippy --all-targets -D warnings` on Linux, and that
  outcome is unverified here. I see no default-lint trigger:
  `new_without_default` does not apply to an unexported `pub(super)` type,
  and `Default::default()` is not a const initializer.

## Spec

0 findings. Contract items 1–5 and disposition items 1–4 are met:

- **Separate identity.** `Attribution` is a separate `Copy` thread-local
  holding closed enums and u32 counters only. Neither `format_attribution` nor
  `emit_attribution` can reach `Observation`, dev, ino, uid, gid or pid.
- **Reset before the anchor.** The reset is the first statement of `run()`
  (`deadline_ms` comes from the pure `settings.deadline()`). After that,
  `site=anchor` is set before `directory()`, and `none` after it succeeds.
- **Sites.** All 10 collapsed `unknown` returns set their own site
  immediately before returning. Exit outcomes leave `site=none`.
  `wait_err` is skipped for EINTR, which continues the loop; ECHILD maps to
  `wait=echild` with `site=wait_err`.
  The enums match the contract:
  - `read`: na, byte, eof, eagain, other
  - `status`: exited, signaled_kill, signaled_other, other
  - `wait`: na, pending, reaped, interrupted, echild, other
- **Counters.** `polls` uses a saturating increment before each `waitpid`.
  `pre_ms` runs from `execute` entry to the parent's fork return. `wait_ms`
  runs from fork to the drop of `Timing`, so it includes the unchanged
  `Child` cleanup, as the code comment notes. Both clamp at 60000.
- **errno.** Errno is only read, via `last_os_error`, after `waitpid` and
  after `read`. Between those reads and production's own errno check at
  `result < 0`, the inserted code makes no syscall: it only touches a
  thread-local `Cell` that was already initialised at `run()` entry. Production
  decisions are therefore unchanged. Residual: if `__tls_get_addr` ever
  allocated, errno could in theory change. That cannot happen for an
  executable's static TLS.
- **Exactly once.** S8 (4 modes) and S9c (file and directory) each evaluate
  `call` or `run` once, emit one line, then assert that same value against the
  unchanged expected value or message. The S8 elapsed, marker, waitpid/ECHILD
  and kill checks remain, as do the S9c A/B checks. No deadline, retry, skip
  or nextest flag changed.
- **Controls.** The mode `S` control asserts `unknown`, `site=deadline` and
  `wait_ms >= deadline_ms`, and makes no poll assertion. The missing-anchor
  control first confirms that all fields are non-default, then asserts that
  the whole record equals the default with `site=anchor` and
  `deadline_ms=200`, and that `polls=0`. Mode 3, A and K mappings are not
  asserted.
- **Formatter test.** It checks ASCII, the 512-byte bound, no `/ \ \n \r`,
  the exact 15-key order, the prefix, saturation, every
  case×mode×outcome×site label, and membership of the read, status and wait
  labels.
- **Output grammar.** The outcome comes from the five closed strings, and any
  other string maps to `unknown`. Marker and pidfile come from `exists()`,
  `elf_mib` from metadata length, and `fixture_ms` is bounded. The line
  carries no errno, ID, path, environment, argv, absolute time or free text.

Informational, not a defect: S8's existing `< 1 s` elapsed bound now also
covers the emission (one `metadata`, two `exists`, a `format!` and an
`eprintln!`). That is sub-millisecond, and it follows from the contract's
"emit, then assert" order.

## Security

0 findings. The change is test-only; the production binary is unchanged.
The emitted line is closed-grammar ASCII. `root` is used only for `exists`
and `metadata` and is never formatted. No secret or owner data appears in
source or evidence.

## Acceptance for the normal main-based PR (coordinator-owned)

1. Rebase or recreate on current main with exactly these two source files
   plus the evidence. Ubuntu, Windows and macOS required checks must be green,
   including Linux clippy `-D warnings` and fmt, and `ci-time-budget` must
   stay under 15 minutes.
2. On the PR's own Ubuntu job, `s8_attribution_controls` and
   `s8_attribution_formatter` must pass, and `s8_outcomes_and_reap` and S9c
   must have run. Record the measured duration of the new tests; the claimed
   <1 s is unmeasured.
3. If S8 or S9c goes red, record its `ARX_S8_ATTR` line as the wanted
   evidence and stop. Do not push again for green.
4. A control failing twice for the same reason is a contract defect: return
   it to Opus.
5. A Sol-provider (non-Anthropic) review of any further source change is
   required.

Budgets: Linux fix 0/3; observability 0/3 before publication (3-push budget).
No dummy green push. The first normal Ubuntu PR source observation is pending.
S8 and S9c remain separate, with unknown causes. Main CI `43862021319` is
green. PR159 stays held on Ubuntu S8 and the Windows engine failures. The
Windows Opus diagnosis is done, and its next contract is pending at root.

M2 open; M3 unchecked; A7 FINAL NO-GO; A12 consumed, no retry. Hardware paths
are **unverified on real hardware**.
