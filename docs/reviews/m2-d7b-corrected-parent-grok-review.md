# Grok review: D7b one corrected protected-parent A1/A2 run

2026-11-14; Grok (grok-4.7); single-agent Standards, Spec, and security review. This reviewer did not write the amendment. The task forbade further reviewers, tests, builds, native operations, network, CI, and dispatch. Standards, Spec, and security stay separate.

**Disposition: no blockers. The docs candidate is ready for ordinary required checks and coordinator merge. This review grants no run permission.**

## Pins

- Reviewed source `60584f5036c8773169de57e681604ce458f5738c`, parent `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`, tree `c7ef1c73e1fc6b2e351302a366bc61c1c3837fd5`. Two-dot and three-dot diffs against main are the same: `docs/adr/0019-unix-native-execution-bridge.md` +74/−7. ADR blob `b1cbe87f413672753896d44ed7071736fdf43975` (34858 bytes, no CR). Main ADR blob remains `e22570b1835f6dde5a47f896e676291ee6925381`. A docs-excluded diff is empty. `git diff --check` on that range is clean.
- Retained `06bb698691bc62eb5a2d43c680bc188dfb5ae288`, parent `228390614e7f29de9f4d49ff126af0332f56e96d`, tree `d29249043a3220bb1161e4d509797b03172749ad`. Its ADR blob equals the source blob. Against the source, only `docs/PROGRESS.md` differs. Against main `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4` (tree `35fd3d11538d7e7581235e23a7e6ac5b4685d8f5`), the diff is that PROGRESS file plus the ADR. A docs-excluded diff is empty. `git diff --check` is clean.
- Prepared parent `228390614e7f29de9f4d49ff126af0332f56e96d`, tree `ffc7502a7dcfbbacb80e51c1a24ceca7a53a2412`, differs from main only by `docs/PROGRESS.md` (+2). Blob `1fbb9aa77a7e4fcee76e00a44f7130f101efc3ad`. Main PROGRESS blob `998874de759da37359402af5f6ee819107e5e1a6` is unchanged at the source commit. The added status line records the PR #85 merge, leaves D7a consumed, and states that no run is authorized. It gives the named main CI run no credit.
- This file is absent from both reviewed commits. It is a supporting review document and may accompany the first main squash. The dispatch SHA is the full squash that lands on main, after the coordinator rechecks that squash against `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`.
- These blobs match at main, the source, the prepared parent, and the retained commit. Driver `tools/unix-exec-validation/driver.py` `d02508eb74b931313e86e164988cd63b18ca60f1`, 41685 bytes, no CR, LF SHA-256 `8b0e1efeada395f8dd6b03e18c23b0648e99f9c29e8349ad2c3926735d6a9d59`. Workflow `.github/workflows/unix-execution-experiment.yml` `e5f1a96dbfa522bda602db45bf24db000ebddd5c`. Unit `packaging/linux/arxd.service` `0b337773c60e49fe84de76db018d2235ae076254`. Profile `packaging/linux/50-arxcovo-standard-rights.conf` `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`. Fixture `crates/arxd/src/exec/disposable.rs` `52c7368d708f44cf9176c26e681d96d072cef47e`. Setup `crates/arxd/src/exec/hierarchy/setup.rs` `b33bed7cd5d7022f30c039e761c544567304d28c`. Linux port `crates/arxd/src/exec/hierarchy/linux.rs` `ae26fad095242b2d01e48e84e8695261e295cad3`. Parser `crates/arxd/src/exec/hierarchy/parse.rs` `54dd52043e863d12189fda7216da43e095c96ffb`.
- Evidence is unchanged, including `docs/evidence/linux-stagef-a1a2-source-alignment.md` `02e3a2a40b181e7ad5e7c55fd9cb444c248b0e68`, `docs/evidence/linux-stagef-protected-parent-fix.md` `2bc2349f2d7c7338ff8ee01592c5894ec7af4099`, `docs/evidence/unix-execution-first-dispatch.json` `1964a553146e43c4d1de6e53e5227207cadc119d`, and `docs/reviews/m2-stagef-protected-parent-grok-review.md` `ee3123ebd3e48db4421e21a47398429343cf3bf2`. The alignment note still records historical driver blob `c0f4f9ff487878293ab31f5c74ed16f1f842cda8` and LF SHA-256 `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`, and it still rejects `bec6460b`. That historical citation was not copied back over the corrected driver. The refusal record `docs/evidence/linux-stagef-a1a2-run-43622157531.json` is blob `c78587c87545fcf710bd9246ac3ee698d65b5689`, 599 bytes, no CR, LF SHA-256 `eda437f17e697d5afaa2cec7a424f88b56472e91b9f18f53e57cd9d16c27d8c9`.
- No test, build, workflow run, variable write, or dispatch was performed. Hosted check runs named in PROGRESS were not re-queried.

## Consistency

The preamble, D7, the D7a header, and D7b agree. D7a is consumed. D7b proposes one fresh hosted A1/A2 run and withholds permission until independent Grok review, the normal required checks, and coordinator merge. The historical `E_PROTECTION` diagnosis stays unresolved. D7b disposes of that prerequisite for its single future run only. D7's activation list is unchanged. Every other implementation, native-acceptance, and production-activation gate remains.

D7a is marked historical. Its body records the original grant and is not current permission. It names merged `f01da6f425cc8d9d3ee8c304ab8112017618da44`, accepted run `43622157531` attempt 1, `E_PROTECTION/run_parent/initial`, mutation `none`, cleanup `not_reached`, and a cleared reviewed-SHA variable. The refusal JSON matches that record: root-owned directory, write bits 18, predecessor root-owned with write bits 0. D7b does not renew D7a.

Run `43091339273` stays an unknown failure site and cleanup, and its environment stays excluded. The D7a environment stays excluded. Source-review and publication success give no native credit. The historical checkpoint under "Historical proposal and clarification checkpoints" still says status proposed and that nothing there was built, run, or tested. It stays under that heading.

The prepared PROGRESS line does not authorize a run. Main's existing PROGRESS already says D7a is consumed and no native retry is authorized.

## Policy and workflow

D7b follows the delegated ADR route: one reviewed candidate, ordinary gates, coordinator merge, then one set, one dispatch, and a clear. An owner answer that arrives before dispatch takes precedence. No further owner confirmation is added for those three operations.

The unchanged workflow `.github/workflows/unix-execution-experiment.yml` is manual `workflow_dispatch` with `contents: read`. Its job runs only when the event is `workflow_dispatch`, the repository is `Dytschgo/arxcovo`, actor and triggering actor are `Dytschgo`, the ref is `refs/heads/main`, and nonempty `UNIX_EXECUTION_REVIEWED_SHA` equals both `github.sha` and `github.workflow_sha`. Checkout uses `github.sha` with `persist-credentials: false`. The runner is fresh `ubuntu-24.04`, the job limit is 10 minutes, and the native step is `driver.py run` after `driver.py guard`. `driver.py` selects only `exec::disposable::disposable_linux_a1_a2`, passes `--ignored --exact --test-threads=1`, and its `gate` repeats the same actor, ref, hosted-runner, and SHA checks. The workflow does not clear the variable. D7b leaves that to the coordinator's finally-style clear after completion, failure, or rejected dispatch, including a check that the variable is absent. An accepted dispatch consumes the grant for every job outcome. Rejection creates no automatic retry.

`plan` returns `Path('/var/lib/arx-ci-' + run)` and `arxd.service` for an id of `[1-9][0-9]{0,19}-[1-9][0-9]{0,19}`. It reads no parent override. `protected` lstats every ancestor and refuses a link, a non-root owner, or any group or other write before `mkdir`. `decide` maps write bits 18 to the recorded `/opt` refusal. `open_stage` still walks `/`, `var`, `lib`, and the per-run name with `O_NOFOLLOW`. `reserve_unit` uses `O_EXCL`. Cleanup of the stage runs only after `protected(stage, 'run_cleanup')`. The fixture drop-in is the `ExecStart=` reset, the `python3 -I` launcher, `Restart=no`, and `RuntimeMaxSec=140`. `disposable_linux_a1_a2` calls `prepare_a1a2` before it continues. The selected test creates no account. `fixture_cleanup` is not called from this path.

Required publication contexts in `.github/main-protection.json` are `verify (ubuntu-latest)`, `verify (windows-latest)`, `verify (macos-latest)`, `ci-time-budget`, and `agent-review`. D7b names the latter two inside "all normal required gates" and does not drop the platform checks.

## Standards

Pass. Documentation only. The amendment is one concern. It adds no product approval flow, secret, workflow edit, or permission change. No baseline smell applies to this prose diff. `git diff --check` is clean.

## Spec

Pass. The delegated policy is present and consistent across the preamble, D7, consumed D7a, and D7b. The source tree is an ADR-only delta against `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`. The retained tree adds only the prepared PROGRESS document. Execution-related source, fixture, driver, workflow, packaged unit, standard profile, and evidence blobs match main. The corrected driver hash is the protected-parent hash above. The stale alignment-era hash and the rejected `bec6460b` citation were not reintroduced.

A first main squash of this docs set onto `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4` meets the candidate predicate when the only differences are this ADR and supporting review or progress documents. Main advancing, or any source or workflow difference, needs a new reviewed disposition. `60584f5036c8773169de57e681604ce458f5738c` and `06bb698691bc62eb5a2d43c680bc188dfb5ae288` are not dispatch SHAs. `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4` is the comparison baseline, and the workflow would treat it as a runnable main SHA if the variable were set to it. D7b forbids that substitution.

## Security

Pass. The diff authorizes no owner-PC access, owner administration, account provisioning, payment, static VM, backup deletion, production constructor, Stage G activation, A13, shim, rights probe, or other native case. It does not relax ancestor, collision, or cleanup guards, and it does not change workflow permissions. The run stays on a fresh GitHub-hosted Ubuntu 24.04 runner through the existing manual workflow. One pass would accept only A1 and A2. M2, full Stage F, Stage G, real-Runner PA6, and production activation stay open. `/var/lib` and installed or dedicated paths stay **unverified on real hardware**. Optional diagnostics stay parked.

Reversibility of this docs change is a two-way door. Reverting the ADR commit restores the consumed-D7a wording. No native directory was created here.

## Verdict

- **Blockers:** none.
- **Publication:** ready for the ordinary gates only: the three platform `verify` checks, `ci-time-budget`, `agent-review`, and coordinator merge. This file is the different-provider review of the docs. Check results are not claimed. This review does not merge.
- **Run permission:** withheld. Set `UNIX_EXECUTION_REVIEWED_SHA` only after those gates, and only to the verified full main squash SHA. Dispatch the existing workflow once, then clear the variable and verify it is absent. This review does not authorize that dispatch.
- **Standards:** 0 findings.
- **Spec:** 0 findings.
- **Security:** 0 findings.

Standards 0. Spec 0. Security 0. Worst issue: none.
