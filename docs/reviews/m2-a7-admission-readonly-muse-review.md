# M2 A7 admission read-only — Muse source/security review

Independent review, Muse Spark 1.3 high, 2026-11-15. Read-only; no builds, no host reads, no dispatch. Builder evidence reused honestly, not rerun.

## Scope pinned

- Candidate: `e3cef57` (`feat(ci): prepare finite A7 read-only admission diagnosis`), parent `fc4ceda`; follow-up HEAD `06a12d6` is PROGRESS-only.
- Baseline main `18b8fef`, tree `215d75d` = reviewed `291a08a` (PR130, per coordinator evidence).
- Five builder paths only: [.github/workflows/a7-admission-readonly.yml](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-39ef13c3-d24796e181d839d3/.github/workflows/a7-admission-readonly.yml), [tools/ci/a7-admission-readonly.py](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-39ef13c3-d24796e181d839d3/tools/ci/a7-admission-readonly.py), [tools/ci/test-a7-admission-readonly.py](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-39ef13c3-d24796e181d839d3/tools/ci/test-a7-admission-readonly.py), `docs/adr/0019-unix-native-execution-bridge.md` (D7j section), [docs/evidence/linux-a7-admission-readonly-preparation.md](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-39ef13c3-d24796e181d839d3/docs/evidence/linux-a7-admission-readonly-preparation.md).
- Shared [tools/unix-exec-validation/driver.py](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-39ef13c3-d24796e181d839d3/tools/unix-exec-validation/driver.py) unchanged: 82576 bytes, SHA256 `43518bed…0f75e` verified locally.
- Spec: [docs/reviews/m2-a7-next-execution-disposition.md](/C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-39ef13c3-d24796e181d839d3/docs/reviews/m2-a7-next-execution-disposition.md) decision (b) + D7j binding text; corrected `fc4ceda` supersedes the old mutable-image/D9b-impossibility inference.

## Standards — PASS, 0 findings

One-concern CI-only delta, conventional commit, UTF8 LF, regular 100644, no deletions/modes (raw diff checked). Every behavior has mocked tests (13 tests, 7 in-memory assertion kills claimed by builder). No secrets, no product approval flows, no unrelated source. Smell baseline: no new abstraction, no duplicated logic (finite 6-entry role table used once), no env-dependent branches beyond the documented gate.

## Spec — PASS, 0 findings

D7j binding met exactly: manual `workflow_dispatch` 3-min `contents:read` job, pinned checkout, no creds, mocked tests before `sudo -n /usr/bin/env -i … python3 -I -B` transport; distinct `A7_ADMISSION_REVIEWED_SHA` required, both old pins forced absent in workflow and wrapper; gate checks repo/actor/trigger/main/sha==workflow==pin/attempt1/host-image before any import or root-down read. Wrapper loads only pinned driver bytes (size+hash before compile) and invokes only `a7_alias_c2` (AST-pinned); package collector, `a7_admission`, cgroup/staging/account/policy paths never called. Finite scope (`/`, `/usr`, `/usr/local`, `/usr/local/bin`, `/usr/bin`, optional C2) with stop-before-open on first unsafe predicate; closed code/role/boolean output, `qualified=false`, `native_credit=false`.

## Security — PASS, 0 blockers

Call graph audited: `_main -> _gate -> _load_driver -> _module_bytes -> _diagnose -> driver.a7_alias_c2`, plus fixed `readlinkat` and bounded `sudo_nnp_digest`. Ancestor FDs held with `O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK|O_DIRECTORY`, exact flags, pre-stat/`fstat`/same-parent reopen checks, parent-first `_verify` on every op (atime excluded only), `finish` verifies then closes all (tests assert closure). Unsafe-component metadata is observed as `E_PATH` predicate without child traversal. Unknown digests/drift refuse; failures collapse to constant `{"schema":1,"status":"refused"}`. Output carries only binding + closed roles/booleans; raw IDs/modes/paths/inodes/unknown hashes/exceptions absent by construction and by test. Prior-run role-less `E_PATH` is not reinterpreted as a historical cause; one fresh observation grants no admission.

Worst issue per axis: none. Findings: Standards 0, Spec 0, Security 0.

Eligible for the publication path only (exact-head review, five greens, coordinator merge, tree-equality, once-only pin/dispatch per ADR). This review grants no read, run, or publication authority. A7/A11, M2, full F/G, real-Runner PA6 remain **unverified on real hardware**.
