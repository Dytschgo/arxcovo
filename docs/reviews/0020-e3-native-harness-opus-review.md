# E3 native evidence harness: independent pre-push review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
Single-agent review on two separate axes (security/Standards and Spec). No subagents.
This review covers only the harness and fixture below. It does not re-review ADR 0020,
and it claims no Opus independence for the ADR design.

**Verdict: CHANGES REQUIRED before push or hosted execution.** The security posture
passes. Two parser defects (B1, B2) mean that the harness cannot produce any
acceptance evidence against real cargo-nextest 0.9.151 output. Both defects fail
closed, so they award no false credit. Hosted execution in this state would spend a run
to get one opaque `failed_closed` line (A1).

## Identity and scope

- Workspace `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-83c3e49b-32efac3f50091f72`.
  This is the top level of an isolated worktree outside canonical `D:/Code/Arxcovo`,
  which shares the Git metadata. Branch `t3/83c3e49b`. HEAD
  `3540db7b02ae666f5bb92cea2babf732cca0f857`. Status was clean at the start.
- Base: PR61 head `f32deee`. Commits reviewed, `f32deee..3540db7`:
  `3413c0b` (PROGRESS), `ad8b996` (harness), `269c748` (evidence),
  `231e6d7` (L10 fixture correction, latest source commit) and `3540db7` (evidence docs).
- `git range-diff` shows that `486b149` equals `ad8b996` and `411844d` equals `269c748`
  as patches. Their trees differ from the originals only by the one-line PROGRESS
  checkpoint `3413c0b`.
- Product scope: `git diff f32deee HEAD` outside docs, tools and workflows touches only
  `crates/arxd/src/exec/hierarchy/linux.rs`, with 2 lines changed inside `mod tests`
  (L10 `link` becomes `memory.max`). Lines 1–365, the production prefix before
  `#[cfg(test)]`, have the same SHA-256 at HEAD and at `eb93620`
  (`e574c63f…7679b69`). No other product, Cargo, existing CI, policy or protection
  file changed.
- Pins: the HEAD blob of `linux.rs` has SHA-256 `e5b6ac15…54eaff`, which equals
  `SOURCE_SHA`. `mutations.jsonl` has `c8248205…ca3d93`, which equals `MANIFEST_SHA`.
  The evidence-table hashes of the workflow (`c5fcb9a4…`), `driver.py` (`d7727f88…`)
  and `test_driver.py` (`e854b17f…`) match the committed blobs. The action SHAs,
  Rust 1.95.0 and nextest 0.9.151 match `ci.yml`.

Read: AGENTS.md, the dytschgo-review skill, ADR 0020 and the E3 brief context through
the evidence doc, `docs/evidence/linux-hierarchy-e3-native.md`, prior review `de2cc44`
and recheck `1de4a5b` (as cited in PROGRESS/evidence), the adapter source and tests,
`parse::classify_entry`, and the workflow, driver, tests and manifest.

## Checks run by this reviewer

| Check | Result |
| --- | --- |
| `python -I -B -m unittest discover -s tools/ci/e3-native -p test_driver.py -v` (Windows) | 24 discovered, 23 passed, 1 skipped (`test_target_guard_rejects_symlink`: Windows denied symlink creation) |
| Read-only in-memory `apply()` of all 8 pinned rows to the real pinned source, through the driver's own `regular`/`load_mutations`/`apply` | source pin ok, manifest pin ok, each old fragment occurs once and only in the production prefix, 8/8 apply |
| Output-format probe: cargo-nextest 0.9.151 (same version as the workflow) on a **synthetic** crate in the session scratch directory, with the driver's exact reporter flags, outside the project | see B1/B2. Real captured text was fed to the driver's `statuses`/`classify`/`baseline_result` |

Not run: fmt/L6 scanner (the run `91efa0dd` cited by the builder was not rerun), any
Cargo command on the project, the mutation driver, native/Linux tests and the workflow.
**Every native kill remains UNRUN.** No full-suite or Linux claim is made.

## Security / Standards axis

### Blockers

**B1. The status-line regex rejects the nextest 0.9.151 progress counter.**
`driver.py:161` expects `] arxd <name>`. Nextest 0.9.151 prints
`FAIL [   0.015s] (1/1) arxd tests::bad` and `PASS [   0.021s] (2/2) arxd tests::good`.
On that verbatim text, `statuses()` returns `[]`, and
`baseline_result(0, <ten real-format PASS lines>)` raises `baseline_missing_test`.
Every hosted run therefore fails at `baseline_before`, and no L7/L8 value or kill can
ever be emitted.
*Fix:* `r"^\s*(PASS|FAIL|SKIP)\s+\[[^\]\r\n]+\]\s+(?:\(\d+/\d+\)\s+)?arxd\s+" + re.escape(name) + r"\s*$"`.
Keep the exact-name anchor and the single-match requirement.

**B2. Every real kill is classified `invalid`.** `classify()` (`driver.py:166`)
returns `invalid` whenever `"error:"` appears in the output. On every exit-100 run,
nextest 0.9.151 prints the final line `error: test run failed`, which was observed
with `EXIT=100`. Even with B1 fixed, the probe gives `invalid` for a genuine
named FAIL with exit 100. It gives `killed` only after this line is removed by hand.
*Fix:* classify by line, not by substring. For exit 100, require exactly one
stripped line equal to `error: test run failed`. Reject any other line that
matches `^\s*error(\[|:)`, which covers `error[E…]`, `error: could not compile` and
`error: no tests`. For exit 0, reject any `error` line. Keep `SKIP` rejection, exit
100, and exactly one named `FAIL`.

**Test meaningfulness (both blockers).** The unit tests build status lines through the
synthetic `status()` helper (`test_driver.py:19`), which has no counter and no trailing
`error:` line. The tests therefore confirm the parser against a format that nextest does
not emit. *Fix:* add verbatim 0.9.151 fixtures (FAIL run, PASS run with an indented
`SKIP L7: …` stderr marker, and a compile-failure run) as string constants. Assert
`killed`, the L7/L8 values and `invalid` against them. Keep the existing adversarial
cases and rebase them on the real format. Record the probe in the evidence doc. The
earlier statement that `cargo nextest run --help` confirms the flags validated the CLI
surface, not the output format.

### Advisories (non-blocking)

- **A1. The failure category is discarded.** `main()` emits only
  `{"label":"harness","status":"failed_closed"}`. Baseline, restore, tree and
  deadline categories are already fixed strings, so they are safe to emit. Emit
  `str(error)` for `GuardError`, and keep `OSError`/`ValueError` as a fixed
  `os_error`/`value_error` label. Without this, a hosted B1-type failure cannot be
  diagnosed without raw logs.
- **A2. The 40 s/180 s budget is plausible but unmeasured.** The baseline includes a
  cold, uncached fetch and compile of a 208-package lock graph: bundled rusqlite C,
  axum, rmcp, tokio and the reqwest dev graph. The pinned `dtolnay/rust-toolchain`
  is believed to export `CARGO_INCREMENTAL=0` when it is unset. This reviewer did not
  verify that at the pinned SHA, so confirm it. If it holds, each mutant does a
  non-incremental rebuild of the ~35 k-line `arxd` test crate, and 40 s is at risk.
  Overruns fail closed (deadline reserve and 10-minute job), so this is a cost and
  usefulness risk, not a safety risk. *Suggested:* add job env `CARGO_INCREMENTAL: '1'`
  and a pre-step `cargo nextest run --locked -p arxd --lib --no-run`, so the 180 s
  baseline measures test runtime rather than cold compilation. The step stays inside the
  10-minute job and is not required.
- **A3.** `proc.wait(timeout=5)` and `reader.join` paths can raise
  `subprocess.TimeoutExpired`, which is outside the caught `(GuardError, OSError,
  ValueError)`. The result is a traceback instead of the fixed JSON line. Exit is
  still nonzero. Map it to `GuardError("reap_failed")`.
- **A4.** `killpg` runs after the leader has been reaped. If the group is empty,
  `ProcessLookupError` is handled. PGID reuse within milliseconds on a disposable
  runner is a theoretical risk only. No change is required.
- **A5.** `runs-on: ubuntu-latest` floats, whereas the existing experiment pins
  `ubuntu-24.04`. Pin it for reproducible evidence if preferred.
- **A6.** The evidence doc's first section keeps the superseded source SHA `c45d2a41…`
  and the L10 blocker. The correction section explicitly supersedes them. This is
  acceptable. Update both sections after the fixes.

### Security fences verified (source analysis)

- The workflow uses plain `pull_request` only: no `pull_request_target`, no secrets,
  `permissions: contents: read`, and `persist-credentials: false`. It uses only
  existing pinned actions, `timeout-minutes: 10`, a numeric UID print with refusal of 0,
  and is non-required. No existing workflow, policy or protection was changed. Fork
  PRs follow GitHub's default fork model, with a read token and no secrets, and
  nothing auto-merges.
- The driver runs isolated (`-I -B`) and has only stdlib imports. The writable path is
  allow-listed to `linux.rs` and the read path to the manifest. It refuses symlinks and
  reparse points in every ancestor and component, and refuses hardlinks (`nlink != 1`).
  Reads are bounded (64 KiB source, 32 KiB manifest, 256 KiB per tree file, 64-entry
  tree). The source and manifest are SHA-pinned before any write. The fixed count is 8,
  with a schema check, one change and one assertion per row, assertions restricted to
  the ten names (so L6 cannot earn credit), and `test(=…)` exact filters. Every row is
  pre-applied in memory before the first write, and replacement happens exactly once in
  the production prefix only.
- Process handling: no shell, stdin closed, own session, 2 MiB merged-output cap,
  wall deadline, SIGKILL to the process group on every exit path, reap with a
  timeout, and no raw output printed. JSON carries only labels, statuses, seconds,
  exit codes and hashes. No secrets or private data are present in the files.
- Restoration: `mutated()` restores and re-hashes in `finally`. `execute()`
  re-verifies the source hash and the hierarchy content fingerprint, then runs
  `baseline_after` after a survivor, an invalid result, a deadline or a failed
  `baseline_before`. The first non-kill stops the run, with no retry and no loosening.
- Identity: the workflow and the driver both refuse UID/EUID 0. `GITHUB_ACTIONS`,
  `RUNNER_OS=Linux` and the cwd are checked before any source access. A root
  `SKIP E3` marker, or any unknown `SKIP`, makes the baseline fail. L7 and L8 are
  emitted explicitly as `passed` or `skipped`, and a skip earns no native pass.
- Budget: baseline ≤180 s, mutants ≤`min(40, remaining-60)`, `baseline_after`
  ≤`min(60, remaining)`, all within 480 s. Exhaustion fails closed. The overall job
  stays under 10 minutes, below the 15-minute CI budget. Compile failure has exit 101
  plus `error` lines, and is `invalid`, never a kill.

### Smell baseline (judgement calls)

- Possible **Primitive Obsession / Duplicated Code**: the marker strings
  (`"SKIP E3"`, `"error:"`, the allowed L7/L8 lines) are spread across `classify` and
  `baseline_result`. B2's fix is a natural moment to make one line classifier that both
  functions share.
- No other baseline smell is material for an evidence-tool script of this size.

## Spec axis (E3 native-evidence brief, ADR 0020 native gates)

| Requirement | Status |
| --- | --- |
| Separate non-required, Ubuntu, `contents: read`, pinned, no persisted credentials, 10 min, root refused | Met |
| No existing CI/policy/protection change, no new dependency/action | Met |
| Isolated Python; fixed 8 mutants with exact count/replacement validated before the first native test; fixed safe regular tracked path | Met |
| Bounded subprocess/output/deadline/process cleanup; restore in `finally` plus hash and tree fingerprint; baseline before and after | Met |
| Explicit L7/L8 passed-versus-skipped parse; no root skip counts as a native pass | Logic met; **not achievable at runtime until B1 is fixed** |
| Kill requires nextest exit 100 and the named expected FAIL, excluding L6, compiler failure and skip | Logic intended; **every real kill misclassified (B2)** |
| L10 fixture discriminates a symlink reported as regular | Met (below) |
| Production prefix identical to `eb93620`, no other product change | Met (hash-verified) |

Per-mutant discrimination, by source analysis only. All are **UNRUN natively**:

- **L1 take raised / removed:** the pipe test leaves 23 (`32-9`) or 0 bytes unread
  instead of 24 (`32-8`), so the `unread == 24` assertion fails. The removal still compiles
  because `&mut File: Read`.
- **L2 NOFOLLOW removed:** `fs.read` of the `cgroup.events` symlink follows to the
  regular `target`. Owned bypass and umask 077 (0600) pass `trust`, the result is `Ok`,
  and the `Refused` assertion fails. Directory opens use `DIR_FLAGS` and are unaffected.
- **L3 record bound removed:** 512 records still pass, but 513 return `Ok`, so the
  `TooBig` assertion fails. `RECORD_MAX` becomes a `dead_code` warning. Neither the
  repository nor the workflow sets `-D warnings`, so the mutant compiles.
- **L5 system UID:** `fs.uid == 0` is asserted before the fixture and fails for the
  ordinary EUID.
- **L9 dup:** the second `list` call sees a non-zero shared offset in `observe`, and the
  5-entry count also fails. **L9 CLOEXEC:** `observe` sees no `FD_CLOEXEC`.
- **L10 symlink regular:** the `memory.max` symlink has `DT_LNK`, which gives
  `Refused`. The mutant forces `DT_REG`. `classify_entry` accepts a dotted lowercase
  regular name as `Control`, so the `Refused` assertion fails. The old name `link` had no
  dot and stayed `Refused`, which was the masking. The correction is test-only and
  correct. The kill depends on the filesystem reporting `d_type`; ext4 and tmpfs do,
  and a `DT_UNKNOWN` filesystem would produce a survivor, which fails closed.
  `arx-1-2` and the other kinds are unaffected.

## Required before push or hosted execution

1. Fix B1 (counter-tolerant exact regex) and B2 (line classifier requiring exactly one
   `error: test run failed` for exit 100 and rejecting other `error` lines).
2. Add verbatim nextest 0.9.151 fixtures to `test_driver.py` and rerun the portable
   suite. `MANIFEST_SHA` and `SOURCE_SHA` are unaffected unless their files change.
3. A1 is strongly recommended in the same correction. A2 is recommended.
4. A narrow different-provider recheck of the correction. Then the coordinator decides
   whether to push. This review grants no push, merge or activation.

## Provider credits and status

- Harness, tests, manifest and workflow (`486b149` / `ad8b996`) and the evidence doc
  (`411844d` / `269c748`): OpenAI gpt-6.1-sol high.
- L10 fixture correction and pin refresh (`231e6d7`): Sol medium. Docs (`3540db7`):
  coordinator. This review: Opus 5.5 high, review only, with no ADR independence claim.
- M2 remains incomplete. Native, installed and dedicated paths are **unverified on
  real hardware**. The causes of the privacy flake, `E_PROTECTION` and
  `ExecutionFailed` remain unknown, with no retry or diagnosis here. No GitHub push,
  CI, rerun, watch, merge, protection, owner infrastructure, native
  proc/sys/cgroup, account, service, VM, experiment, private-state or
  ignored-fixture operation was performed.
