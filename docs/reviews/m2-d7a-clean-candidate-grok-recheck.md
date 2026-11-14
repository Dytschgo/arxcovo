# Grok recheck: D7a clean one-run candidate

2026-11-14; Grok (grok-4.7); single-agent recheck of the corrected Sol amendment. This reviewer did not write the amendment. The task forbade further reviewers, tests, builds, native operations, CI, GitHub writes, variable changes, and dispatch. Standards and Spec stay separate.

**Disposition: both prior blockers are closed. The docs candidate is ready for required checks and coordinator merge. Run permission is withheld.**

## Pins

- Reviewed branch `docs/d7a-one-hosted-run` at `70bb9c795b614f5d7f3df5054d79cb7f1e536d77`, tree `d9fbc44fff25dd4a8c08db94a4c47fd14ac2dd78`. This checkout `t3/review-d7a-policy-fix` was that same commit and clean before this file. `origin/main` is `82162956839740753a0b97a790c04b76784e7c9a`, tree `c925747f0acfc6e4bc90d580e7ff7ebc8d47200a`. Merge-base is that main commit.
- Two-dot `git diff 82162956839740753a0b97a790c04b76784e7c9a..70bb9c795b614f5d7f3df5054d79cb7f1e536d77` and three-dot `git diff 82162956839740753a0b97a790c04b76784e7c9a...70bb9c795b614f5d7f3df5054d79cb7f1e536d77` are identical. Numstat is `docs/PROGRESS.md` +2, `docs/adr/0019-unix-native-execution-bridge.md` +59/−7, and added `docs/reviews/m2-d7a-bounded-run-grok-review.md` +48. Blobs are `8813ba87c49927c29517977c8d5da22d2105a4aa`, `e22570b1835f6dde5a47f896e676291ee6925381`, and `629ff9af773f0f5826aa99ca4d5eb965c56f4531`. A docs-excluded diff is empty. The diff has no activity-feature path. That branch was left unread.
- `5135532c18bedbce1f586d370049a770b2632ee0` changes the ADR and PROGRESS only. `70bb9c795b614f5d7f3df5054d79cb7f1e536d77` adds the retained review only.
- Prior review `b1d9653a074535c38aacea32cb6c84506efda0b7`, parent `3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c`, is retained as blob `629ff9af773f0f5826aa99ca4d5eb965c56f4531`. Its verdict covers tree `3b806a1`.
- Evidence `docs/evidence/linux-stagef-a1a2-source-alignment.md` is blob `02e3a2a40b181e7ad5e7c55fd9cb444c248b0e68` at main and at this head.
- These blobs match at both commits. Driver `tools/unix-exec-validation/driver.py` `c0f4f9ff487878293ab31f5c74ed16f1f842cda8` is 41674 bytes, no CR, LF SHA-256 `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`. Workflow `.github/workflows/unix-execution-experiment.yml` `e5f1a96dbfa522bda602db45bf24db000ebddd5c`. Unit `packaging/linux/arxd.service` `0b337773c60e49fe84de76db018d2235ae076254`. Profile `packaging/linux/50-arxcovo-standard-rights.conf` `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`. Fixture `crates/arxd/src/exec/disposable.rs` `52c7368d708f44cf9176c26e681d96d072cef47e`. Setup `crates/arxd/src/exec/hierarchy/setup.rs` `b33bed7cd5d7022f30c039e761c544567304d28c`. Linux port `crates/arxd/src/exec/hierarchy/linux.rs` `ae26fad095242b2d01e48e84e8695261e295cad3`. Parser `crates/arxd/src/exec/hierarchy/parse.rs` `54dd52043e863d12189fda7216da43e095c96ffb`.
- The unchanged workflow still requires `workflow_dispatch`, repository `Dytschgo/arxcovo`, actor and triggering actor `Dytschgo`, `refs/heads/main`, and nonempty source SHA = workflow SHA = `UNIX_EXECUTION_REVIEWED_SHA`. Checkout ref is `github.sha` with `persist-credentials: false`. Runner is fresh `ubuntu-24.04`, job limit 10 minutes, `contents: read`. No test, build, workflow, or variable write was performed.

## Blockers from b1d9653

1. Closed. The status preamble now states one hosted A1/A2 exception, effective only after independent review, required checks, and coordinator merge. Other native execution and historical retries stay unauthorized. The historical `E_PROTECTION` diagnosis stays unresolved, and D7a disposes of that prerequisite for its single run only. Every other implementation, native-acceptance, and production-activation gate remains. D7's activation list is unchanged. Its closing sentences exclude run `43091339273` and its environment, and name the bounded A1/A2 exception as the only newly permitted execution.

2. Closed. The two-dot and three-dot diffs against `82162956839740753a0b97a790c04b76784e7c9a` change only ADR 0019, PROGRESS, and the retained review. The evidence blob stays `02e3a2a40b181e7ad5e7c55fd9cb444c248b0e68`. The driver LF SHA-256 stays `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`. The runtime, package, workflow, fixture, and source blobs above match main.

## Policy

D7a follows the delegated instruction: use the reviewed ADR, then one hosted run after different-provider review, required checks, and coordinator merge. It adds no further owner confirmation. An owner answer that arrives before dispatch takes precedence. The coordinator may set `UNIX_EXECUTION_REVIEWED_SHA` to the verified candidate SHA, dispatch the existing workflow once on a fresh hosted Ubuntu 24.04 runner, and clear that variable in a finally-style step after completion, failure, or rejected dispatch. One accepted dispatch consumes the grant for every job outcome. There is no automatic retry and no reusable permission.

The technical guards in D7a remain: one SHA equality, fresh host, read-only credentials, packaged standard profile, fixed-unit collision refusal, owned-only cleanup, and bounded output and lifetime. The run includes no account provisioning, owner-PC access, owner-host administration, payment, static VM, production constructor, or Stage G activation. Only `exec::disposable::disposable_linux_a1_a2` is selected. An A1/A2 pass accepts only those two cases. M2, Stage F, Stage G, and real-Runner PA6 stay open. Hardware stays **unverified on real hardware**.

## Advisories

- The historical checkpoint still says status proposed, and that nothing there was built, run, or tested. It stays under that heading and does not override D7a.
- The preamble says "independent review". D7a says "different-provider review" plus required checks. Together they match the repository rule. The preamble no longer names owner operation, Stages B–D, or cgroup setup as examples. D7 still requires those for activation, and D7a leaves them unauthorized.
- PROGRESS on this head says the narrow correction review is next. This file is that review. The retained blob `629ff9af` judges `3b806a1`, not `70bb9c7`.

## Standards

Pass. Documentation only. No product approval flow, workflow edit, secret, or baseline smell.

## Spec

The delegated policy and both publication blockers hold on `70bb9c795b614f5d7f3df5054d79cb7f1e536d77`. A first main squash of that tree onto `82162956839740753a0b97a790c04b76784e7c9a` meets the candidate predicate. This supporting review may accompany that squash. A later main commit whose execution bytes differ needs a new disposition and cannot consume this grant.

## Verdict

- **Blockers:** both closed.
- **Publication:** ready for required checks and coordinator merge. This recheck supplies the different-provider review of the corrected docs. Check results are not claimed.
- **Run permission:** withheld until that policy is on main and the full squash SHA is verified against these execution blobs. `70bb9c795b614f5d7f3df5054d79cb7f1e536d77` is not the dispatch SHA.
- **Standards:** pass. 0 findings.
- **Spec:** 0 findings.

Standards 0. Spec 0. Worst issue: none.
