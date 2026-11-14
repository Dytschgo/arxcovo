# Grok review: D7a one disposable A1/A2 run

2026-11-14; Grok (grok-4.7); single-agent review of the Sol amendment. This reviewer did not write it. The task forbade further reviewers, tests, builds, native operations, variable changes, and dispatch. Standards and Spec stay separate.

**Disposition: not publication-ready. No run permission.**

## Pins

- Isolated worktree, branch `t3/review-d7a-bounded-run-policy`, Git top-level. Clean before this file. Reviewed amendment `3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c`, parent `527d110f2903c8b08a9668eca1ee8d410eb2c8a6`. That commit changes only `docs/adr/0019-unix-native-execution-bridge.md` (blob `7482af3925462cf5892ee4167c16bce5f96462d9`) and `docs/PROGRESS.md` (blob `a3b31e16d83eaa2a2e8d727c0d7e934a3127301a`).
- Source identity is `82162956839740753a0b97a790c04b76784e7c9a` (`origin/main`), tree `c925747f0acfc6e4bc90d580e7ff7ebc8d47200a`, the same tree as reviewed head `c0b149ce35aa0ea964b606e10ef16747c89058e0`. It is not an ancestor of this branch. Merge-base is `6b0aadbd55396ee738b5b0509f574427af192718`. The local branch named `main` is `176af7a180387c0423fe70a013567d99def9f14c` and was not the comparison.
- `git diff 82162956839740753a0b97a790c04b76784e7c9a 3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c` names five documents. Execution paths match: driver `c0f4f9ff487878293ab31f5c74ed16f1f842cda8` (LF SHA-256 `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`, 41674 bytes, no CR); workflow `e5f1a96dbfa522bda602db45bf24db000ebddd5c`; unit `0b337773c60e49fe84de76db018d2235ae076254`; profile `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`; fixture `52c7368d708f44cf9176c26e681d96d072cef47e`.
- The unchanged workflow still requires `workflow_dispatch`, repository `Dytschgo/arxcovo`, actor and triggering actor `Dytschgo`, `refs/heads/main`, and nonempty source SHA = workflow SHA = `UNIX_EXECUTION_REVIEWED_SHA`. Checkout ref is `github.sha` with `persist-credentials: false`. Runner is fresh `ubuntu-24.04`, job limit 10 minutes, `contents: read`.
- Driver `TEST` is `exec::disposable::disposable_linux_a1_a2`. `prepare_a1a2` calls production `setup` before `READY=1`. `run_body` is the only native body. No test, build, or workflow run was performed in this review.

## Blockers

1. The status preamble still forbids the run D7a grants. Lines 11–15 say this document grants no native execution, that the `E_PROTECTION` diagnosis remains a separate gate, and that the no-retry rule remains in force. Amended D7 says the bounded A1/A2 exception is the only newly permitted execution. D7a dispositions the historical-diagnosis prerequisite for one hosted run. The first main squash that contains the amendment is the only candidate, so the preamble has to agree inside that squash. The amended D7 sentences about run `43091339273` agree with D7a: that run and its environment stay excluded, and the new run is not a reproduction.

2. Amendment tree `3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c` fails D7a's candidate test. The candidate is the first main squash containing the amendment. Its execution-related source, fixture, driver, workflow, packaged unit, and standard profile must match `82162956839740753a0b97a790c04b76784e7c9a`, and only this ADR plus supporting review/progress documents may differ. A later main commit cannot consume the grant by substitution. This tree also changes `docs/evidence/linux-stagef-a1a2-source-alignment.md` from blob `02e3a2a40b181e7ad5e7c55fd9cb444c248b0e68` to `05995b1fe30e4a0d65e643328e6ee985237fd4c9`. That hunk restores stale driver content hash `bec6460b9609339fe54ff01df9e13591c3eeed00706e303af07a176c4a5259c2` and removes main's correction. The recomputed LF SHA-256 of blob `c0f4f9ff487878293ab31f5c74ed16f1f842cda8` is `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`. Keep the evidence blob equal to main. Publishing this tree would make an ineligible first squash.

## Advisories

- The checkpoint that D1–D7 stand, except the labelled D1, D4, and D5 sentences, remains under the historical heading and keeps status proposed. It does not override D7a.
- One accepted dispatch consumes the grant on success, failure, or a skipped job. "Dispatch once" and "do not dispatch again" also cover a rejected dispatch. The clear stays in the finally path, including rejection, and the variable remains through job evaluation. No further confirmation belongs in the policy.
- Supporting files under `docs/reviews/`, including `m2-stagef-a1a2-native-run-proposal.md`, `m2-stagef-a1a2-native-plan-grok-review.md`, and this review, fit the allow-list. `docs/PROGRESS.md` fits. The evidence file does not.
- A source merge on main before this squash, including the parallel activity-ID work, makes execution bytes differ from `82162956839740753a0b97a790c04b76784e7c9a`. That squash cannot use this grant. The reviewed-SHA variable has to be the full candidate SHA. The old main SHA would fail the workflow equality check once the docs change the commit.
- M2, Stage F, Stage G, and real-Runner PA6 stay open. Hardware stays **unverified on real hardware**. An A1/A2 pass would accept those two cases only.

## What already matches

D7a allows one fresh hosted Ubuntu 24.04 run of the existing `unix-execution-experiment.yml` after different-provider review, the required checks, and coordinator merge. It keeps the manual actor, main ref, SHA equality, read-only credentials, packaged standard profile, fixed-unit collision refusal, owned cleanup, and bounded output and lifetime. It selects only `disposable_linux_a1_a2`. It excludes A13, every other native case, account provisioning, owner-PC access, payment, host administration, static VMs, production activation, and automatic retry. An owner answer that arrives before dispatch takes precedence. The clear-after-completion timing from the corrected plan remains.

## Standards

Pass. The amendment is documentation. It adds no product approval flow, workflow change, or secret. No baseline smell applies. The preamble clash is a Spec defect.

## Spec

The delegated policy is in D7a: one reviewed candidate, one fresh hosted run, normal gates, then one set, dispatch, and clear, with the existing technical guards kept. Two requirements fail. The preamble contradicts the new grant. This tree is not a qualifying later squash because the evidence document differs and cites the wrong driver hash.

## Verdict

- **Publication:** not ready. Fix the preamble in the same squash that introduces D7a, and leave the evidence blob at `02e3a2a40b181e7ad5e7c55fd9cb444c248b0e68`.
- **Run permission:** not granted. A corrected tree still needs exact-head review and all required checks before merge. After merge, verify the full candidate SHA, set `UNIX_EXECUTION_REVIEWED_SHA` to that SHA, dispatch once, and clear it in the finally path.
- **Standards:** pass. 0 findings.
- **Spec:** 2 blockers.

Standards 0. Spec 2. Worst Spec issue: the status preamble contradicts D7a, and this tree would be an ineligible first squash.
