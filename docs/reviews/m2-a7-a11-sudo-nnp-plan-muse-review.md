# M2 A7/A11 sudo NNP plan - independent Muse review

Scope: plan commit `f500c12` (`docs(m2): plan combined A7 A11 sudo NNP acceptance`,
100-line `docs/reviews/m2-a7-a11-sudo-nnp-plan.md` + `docs/PROGRESS.md` touch)
on parent `f8b4131`, base `7113a42` (PR117 merge). Reviewed at
`07d163c` clean; plan file byte-identical between `f500c12` and `HEAD`
(the `f500c12..HEAD` delta is `docs/PROGRESS.md` only).
Read-only: AGENTS.md, `docs/KICKOFF.md` M2/routing, brief 0020 E4/R5/S1/S6/S8-S13
and privileged-acceptance A7/A11, ADR 0019 D2/D7(a-g), ADR 0020
D8/R5/D10/Bounds/Consequences, `unix_shim_rights*.rs`,
`exec/mod.rs` (`run_phased`), `exec/unix.rs`, `exec/disposable.rs`,
`exec/disposable/a4.rs`, `tools/unix-exec-validation/driver.py`
(admission/staging/cleanup), E4 evidence
(`docs/evidence/linux-rights-probe-e4-native.md`). No implementation, build,
test, dispatch, account, sudo, probe, push, or native operation performed.
A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**.

## Standards - PASS, no findings

Docs-only review commit scope: exactly one new file,
`docs/reviews/m2-a7-a11-sudo-nnp-plan-muse-review.md`. Small conventional
commit; no `docs/PROGRESS.md`, source, ADR, config, workflow, packaging, or
test bytes touched. UTF-8 LF, Markdown only, no secrets, owner data, paths,
identities, or policy text. No product approval flows, publishing changes,
checklist overhead, or policy-parser/host-audit framework added. Consistent
with settled style (concern-per-file reviews, hardware-unverified labelling,
`agent-review` before merge).

## Spec - CONDITIONAL PASS (plan is sound; three blockers below gate execution)

The plan correctly derives the contract from the repo, not from paraphrase:

- Existing coverage is stated honestly: E4 S8/S9/S9b/S9c/S10/S11/S12 plus S6
  integration (`TOOLS` order, bounded rights line, NNP=true, sudo!=exit_zero)
  explicitly allows absent/untrusted/unknown and is therefore insufficient
  for A7. Hosted E4 run evidence is owned-ELF only, not A7/A11. Correct.
- `Runner::run_phased` -> `unix::run` already sets NNP, drops UID/GID,
  clears capabilities/groups, and verifies identity before exec; account
  policy checks groups, not named sudoers grants. No production change is
  needed there. Correct, and the plan's no-activation / no-bridge / no-config /
  no-CLI-selector / no-package / no-workflow-change ceiling matches ADR 0019
  D7 and ADR 0020 D10/Stage G separation.
- Actual callgraph for the oracle exists in-tree: staged production `arxd`
  with literal `[tool-probe]` through the real `Runner::run_phased` is
  directly callable in tests (same shape as existing `disposable_linux_*`
  ignored controllers); `unix_shim_rights_linux.rs` already implements the
  D8 bound probe (anchor `/`, C1/C2/C3, descriptor-relative ancestor checks,
  exact `bin` alias rule, `O_PATH|O_NOFOLLOW|O_CLOEXEC` final open,
  classify-all-before-execute, `fork` + `execveat(AT_EMPTY_PATH)` with
  `["sudo","-n","true"]`, empty env, stdio `/dev/null`, cwd `/`, close-on-exec
  error pipe, 5 s poll / SIGKILL-own-child / reap). The plan reuses this
  instead of reinventing it. Feasible.
- A7 needs exactly what the plan asks: two fresh generated build accounts
  (one root-owned `0440` exclusive sudoers.d file granting ONLY
  `<generated> ALL=(root) NOPASSWD: /usr/bin/true`; one with no applicable
  rule), each through the same Runner/account/limits/deadline configuration,
  oracle `no_new_privs:true` + `sudo:exit_nonzero`, absent/untrusted/unknown/
  exit_zero/skip as failures. This is must-have acceptance per brief A7, not
  overengineering: one account cannot prove the fence is policy-independent
  (ADR 0020 D8: under R3 the refusal is identical with and without the rule).
- A11 needs only the part pure tests cannot prove: E4 S9b already covers the
  alias rule with fakes, and S9c covers check-to-exec binding with the test
  hook. What stays unproven is (a) C1-selected + C3-skipped against the real
  merged-`/usr` shape and (b) `execveat` actually running the installed ELF
  `sudo` as the build account. The plan's fixed E4 reentry with `cfg(test)`
  scalar observation (C1 selected, alias accepted/C3 skipped never opened,
  `O_PATH` + `FD_CLOEXEC` on the held regular inode, calling UID/GID match
  pins, observe-before-fork, async-signal-safe child, normal nonzero + EOF on
  the exec-error pipe, reentry rights also true/exit_nonzero, runner
  identity/cleanup as A7, output <=4096 reduced to booleans, no usernames /
  paths / numeric identities / raw policy text) is the minimal sufficient
  oracle. Keep it scalar-only; anything richer is overengineering.
- Out-of-scope exclusions are correct and must be retained: no general policy
  parser (fixed `visudo` syntax checks only, never policy proof), no host
  audit framework, no per-run `sudo` (D8 observes at discovery), no polkit /
  non-exec channel claims, no admin-row or macOS credit (pure-logic /
  `unknown` only), no owner steps, no later milestones, no diagnostic
  framework, no weakened acceptance.

## Security - PASS WITH HARD CONSTRAINTS (owned-only provisioning/cleanup)

Intent is preserved; the following are load-bearing, not optional:

- All `/etc` preparation happens outside the Standard service:
  `ProtectSystem=full` forbids in-service sudoers edits. Root-owned install
  path only; no installation fallback; refuse ambiguous/external policy.
- Fixtures consume only newly owned identities: two exclusive generated
  accounts, private roots/workspaces, no supplementary groups; pin captured
  UID/GID/inodes. Production values pinned per S12 (anchor `/`, owner 0,
  C1/C2/C3 order, both alias targets, argv, empty env, 5 s deadline, stdio
  `/dev/null`, cwd `/`); native reentry uses `Settings::default()`, real `/`
  anchor, owner 0, actual installed sudo, unchanged probe/rights observation.
  No fake ELF, owner/argv/env override, or copied sudo.
- Cleanup is owned-only: stop/confirm unit/cgroup disappearance, no live
  fixture UID, then remove only matching owned policy file / accounts /
  workspaces / artifacts and recheck absence. On uncertainty retain
  reservation/quarantine, fail closed, never adopt or delete foreign state.
- Bounded output discipline holds end to end: rights line <=96 bytes, total
  output <=4096, reentry output privately captured and reduced to booleans,
  never released/persisted/logged beyond the latch decision.

## Blockers (exact; execution waits on all three)

1. Qualifying host/image + policy gap unattested. No evidence attests a fresh
   GitHub-hosted Linux image (systemd>=254, kernel>=5.14, cgroup2,
   memory+pids delegation, `user.delegate=1`) with root-owned installed ELF
   C1 sudo, protected ancestors, C2 absent-or-trusted, root-owned `/bin` as
   exactly `usr/bin` or `/usr/bin`, and a bounded local sudoers configuration
   that includes the one generated file and excludes other applicable
   user/group/default grants. Until attested, deliver only ignored source +
   pure rejection tests and credit neither A7 nor A11.
2. No execution authority from this plan. D7a-g are consumed (D7g proposes
   only one fixed A4 run); A7/A11 needs a NEW narrowly reviewed native grant
   naming both accounts/cases, artifact/source/workflow pins, one dispatch,
   and cleanup/variable lifecycle before selection or execution. Owner already
   authorizes coordinator disposable actions through those gates; no extra
   human approval step is required or wanted.
3. Review/merge gates unchanged: Sol builds, Opus 5.5 high design/security
   review (grok-4.7 high only on provider limits), different-provider
   source/exact-head review, green required checks including `ci-time-budget`
   (<15 min), coordinator merges. Routing note: Opus-limited / Grok-402
   history resolves to the already-authorized existing Muse fallback; not a
   new blocker and waives no gate.

## Advisories (non-blocking)

- Keep the `visudo` syntax check as a bounded fail-closed refusal only; it is
  not policy evidence (D8 observes an exit status, never a sudoers fact).
- Expect authentication log entries per hosted discovery (ADR 0020
  Consequences); cap native scope to two accounts x (A7 probe + A11 reentry).
- Do not widen to per-run sudo, admin enablement, macOS, polkit, or M2 MCP
  run/activity, Stage G, full F, or PA6 claims on this slice.

## Minimal next slice (settled, no spec waiver, no 100-line ADR loop)

- Slice 1 (no grant needed): one ignored controller
  `exec::disposable::sudo_nnp::disposable_linux_a7_a11` + one literal ignored
  build-account E4 reentry; driver owned-fixture preparation/cleanup paths
  (unselected in ordinary CI); pure/model rejection tests with mutation proof
  for every discriminator; S12/S13 pins. Credits neither A7 nor A11.
- Slice 2 (needs Blocker 1 + 2): single disposable dispatch of Slice 1 on
  both accounts through the real Runner; A7 oracle both cases + A11 scalar
  observation; owned cleanup + evidence record. If image/grant unavailable,
  stop after Slice 1. No ADR change is required unless a requirement changes.

Eligibility: plan eligible as the settled M2 A7/A11 sub-slice subject to
Blockers 1-3; not eligible to claim A7/A11/M2/full-F/G/PA6 until Slice 2
passes on the attested host through all gates.
