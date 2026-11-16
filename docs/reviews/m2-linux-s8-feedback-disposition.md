# Linux S8 failure: feedback-path disposition

Status: **PROPOSED, docs only, 2026-11-16.** Author: Anthropic /
claude-opus-5-5 / high, architecture/debug sub-agent. Needs a different-provider
review (Sol; Grok 4.7 high fallback) before any builder starts. No source,
test, workflow, PROGRESS or ADR change. No Linux execution, rerun, dispatch,
push, PR, comment or install. Debug phase 1 is **not done**. No cause, hypothesis
ranking or fix is claimed here.

## Pins and recorded facts

| Item | Value |
| --- | --- |
| Base / design HEAD | main `e601caa` (CI `43862021319` green) / `2c23af0` |
| Red PR | PR159 head `3d702d1`, CI `43865758301`, Ubuntu job `133293758711` |
| Toolchain / runner | rustc 1.95.0, cargo-nextest 0.9.151, no `.config/nextest.toml` |
| Failure | `s8_outcomes_and_reap` at `unix_shim_rights_linux_tests.rs:112`, mode `3`: expected `exit_nonzero`, got `unknown`; 0.645 s; position 526/579; 528 pass, 1 fail, 22 skip, 50 not run |
| Same job | all 15 TLS tests passed |
| Shim + test bytes | identical to main. That gives no causal attribution either way |

The local job log was read only for the lines above. No raw log, path or ID from
it is committed. The failed assertion runs before the elapsed-time, marker and
reap checks, so the hosted record cannot tell how long mode `3` took. It also
cannot tell whether the helper ran.

## Feedback path decision

- **Local:** none. WSL is not installed (root-confirmed). Docker and podman are
  absent. The earlier Multipass inventory timed out and was not repeated. No
  remote host or credential was supplied. Getting a local Linux loop would need
  owner provisioning (a WSL install or a VM), which agents may not do. It stays
  an owner option, not a prerequisite. No local reproduction is invented.
- **Authorized ordinary Linux:** the required Ubuntu `ci.yml` job on normal
  pushes to a main-based PR. It already runs the unchanged, red-capable s8
  assertion, but at an unknown, low rate, and it collapses every failure branch
  into `unknown`. So it can sample the failure but cannot yet diagnose it.
- **Decision:** make each future red self-attributing through a test-only
  contract. Debug phase 1 completes only when either (a) an attributed red
  exists and a site-specific loop is then designed from it, or (b) owner-provided
  Linux capacity enables the bounded S9c-style repeat loop. Until then: no
  hypothesis ranking, no fix.

## Collapsed `unknown` sites in `unix_shim_rights_linux.rs` (structure, not cause)

`anchor` run():251 · `classify` candidate or alias error in classify_and_execute():305
· `null` :348 · `cwd` :351 · `pipe` :355 · `fdwrap` :360 · `fork` :375 ·
`deadline` :404 · `reaped_bad` :413 (pipe byte present, read error, or not
WIFEXITED) · `wait_err` :433. The static poll loop (deadline checked before
`waitpid`, 5 ms sleep) is a site list only. It proves nothing about which site
fired.

## Attribution contract v1 (for a later builder; not implemented here)

1. **Shim, `#[cfg(test)]` only:** add a separate `Copy` thread-local
   `Attribution`, not new fields on `Observation`, so the emitter cannot reach
   uid, gid, dev or ino. Reset it at the entry of `run()`, before the anchor
   branch. Set it in the parent only and never touch it in the fork child (same
   rule as `Observation`).
   Fields: `site` (enum: the 10 sites above plus `none`); `pre_ms` (from
   `execute` entry until fork returns) and `wait_ms` (from fork until return),
   both u32 saturating at 60000; `polls` (u32 count of `waitpid` calls);
   `deadline_ms`; `read` {na, byte, eagain, other}; `status` {na, exited,
   signaled_kill, signaled_other, other}; `wait` {na, echild, other}.
   Production control flow, return strings and the `DEADLINE` constant do not
   change. Every inserted line is cfg(test)-gated.
2. **Tests:** `s8` (and `s9c`, labelled separately) emit exactly one stderr line
   per iteration, immediately **before** the unchanged `assert_eq!`:
   `ARX_S8_ATTR v1 mode=<A|3|S|K> outcome=<str> site=<enum> pre_ms= wait_ms=
   polls= deadline_ms= read= status= wait= marker=<0|1> pidfile=<0|1>
   fixture_ms= elf_mib=`. Marker and pid-file values come from non-mutating
   `exists()` checks.
   **Forbidden in the line:** raw errno, OS error text, paths, environment,
   argv, pid, uid, gid, inode, device, absolute timestamps and free text.
   Nextest prints this line only for failing tests, so passes stay silent.
3. **Unchanged:** every existing assertion line, the 200 ms fixture deadline,
   the 5 s production deadline, `s12`, nextest flags and workflows. No retry,
   skip, ignore, quarantine, timeout increase, test reordering or gate bypass.
4. **New test `s8_attribution_controls`.** It is ordinary, Linux-only and
   skips as root. It covers only outcomes that are deterministic by
   construction:
   (a) missing anchor → `unknown`, `site=anchor`, `polls=0`;
   (b) mode `S` → `unknown`, `site=deadline`, `wait_ms>=deadline_ms`,
   `polls>=1`;
   (c) a pure formatter test: the exact key set, and no forbidden key or `/`.
   Mode `3`, `A` and `K` mappings are not asserted, so the test cannot add a
   second copy of the flaky symptom.

## Credit, budgets and stop rules

- **Without Linux, no Linux credit:** diff inspection that every addition is
  cfg(test); Windows and macOS CI unaffected; and, only if the target is
  already installed, `cargo check --tests --target x86_64-unknown-linux-gnu
  -p arxd` as compile-only evidence.
- **Linux credit:** only from the attribution PR's own Ubuntu job. That means
  control (a), (b) and (c) passing, and `s8` and `s9c` running. The CI time
  increase is under 1 s.
- **Budget:** one Sol builder, one PR from current main, at most 3 pushes on a
  separate counter: S8 observability 0/3. Linux fix stays 0/3. The shared
  Windows Sol 3-attempt stop is unrelated. No dispatch, manual rerun or
  push made only to get green.
- **Stop at once if** the diff touches non-cfg(test) code or any existing
  assertion or deadline, the line could carry a forbidden field, or a control
  fails twice for the same reason (contract defect: return to Opus).
- **If s8 or s9c goes red inside the attribution PR,** that red is the wanted
  evidence. Record its line and do not push again to turn it green. The PR then
  waits under the normal gates. Diagnosis proceeds from the line.
- **PR159 stays blocked.** It gets no waiver, and the TLS stop rules are
  unchanged. After attribution merges, a normal update of PR159 from main
  counts as one observation: green is a pass sample, not a fix, and red
  carries attribution. Its merge disposition belongs to the coordinator plus a
  different-provider reviewer.
- **If no attributed red appears** across normal Ubuntu jobs, record the
  observed sample count. Keep S8 and S9c open and distinct. Never quarantine.
- M2 stays open. A7/A11 FINAL NO-GO and A12 (consumed, no retry) are unchanged.
  This gives no native M2 grant. Hardware paths remain **unverified on real
  hardware**.

## Handoff

Builder: OpenAI / Sol / high. Files:
`crates/arxd/src/unix_shim_rights_linux.rs` (cfg(test) additions only) and
`crates/arxd/src/unix_shim_rights_linux_tests.rs` (emit lines plus the new
control test). Conventional commit `test(linux-shim): attribute collapsed
unknown outcomes`. Reviewer: Opus 5.5 high, with this checklist:
the cfg(test) gating; no change to any existing assertion; reset before the
anchor branch; no field written in the fork child; the forbidden-field audit;
the CI budget.

**Next action:** the coordinator requests a Sol (or Grok fallback) review of this
document. If it is accepted, the coordinator assigns the Sol builder above.
