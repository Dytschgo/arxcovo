# Opus disposition: C1 Windows Playwright timing evidence

Reviewer: Anthropic / Claude Opus 5.5 / high, read-only design sub-agent, 2026-11-16. Builder: Sol.
Target: `work/ci-windows-playwright-install` at `2c4b28a`; PR154 head `5ba2cf6` (all five gates green; independent Opus comment 6909366108).
Spec: C1 in brief `2e3a388` (acceptance and stop rule), the C1 evidence file as committed in `43b1dc3`/`5ba2cf6`, Opus review `01c0761` P1, Sol sampler recheck `8a7caaa`.

**Verdict: PROPOSE_REVISION.** This needs Sol's written acceptance before merge. If Sol rejects it, the fallback is **ABANDON** (see the end). The PR run has **not** passed the declared criterion, and this document does not say it has.

## Facts (one public-API read of each job's steps; no rerun, no new run)

| Windows job | Job | clippy | nextest | npm ci | Install | Everything except install |
| --- | --- | --- | --- | --- | --- | --- |
| PR153 ref 133123407634 (`53a7ff2`) | 598 s | 87 | 281 | 4 | 151 | 447 |
| main 133127359300 (run 43815774741, `58d06c1`) | 542 s | 82 | 248 | 4 | 139 | 403 |
| PR154 133134613345 (run 43818081980, `5ba2cf6`) | 493 s | 93 | 304 | 13 | **7** | 486 |

- **Component first (P1 order).** The install step saved 144 s against 151 s and 132 s against 139 s. The mechanism comes from source: `--with-deps` on Windows runs `install_media_pack.ps1`, which runs `Install-WindowsFeature Server-Media-Foundation` (`01c0761`). The Chromium shell it installs is the same. The browser smoke (4 s) and Electron smoke (7 s) passed.
- **Whole job second.** The job is 105 s shorter than the 598 s reference and 49 s shorter than the 542 s main run. Against main, all steps except the install grew by 83 s: nextest +56, clippy +11, npm ci +9, other steps +7 net. Against the reference they grew by 39 s: nextest +23, clippy +6, npm ci +9, other +1.
- **Why the extra time is not C1's.** Every step that grew runs *before* the changed step (24). PR154 changes no Rust source compared with `58d06c1`: the diff touches only `ci.yml`, `ci-workflow.test.mjs` and docs. The new Linux-branch step 23 is skipped and takes 0 s. So the gap between the 49 s whole-job figure and the 132 s component saving comes from steps C1 cannot affect. This follows from step order. It is not a statistical claim, and it does not predict what the next run will measure.
- **Noise.** On three runs, nextest ranged from 248 to 304 s (56 s) and all non-install steps from 403 to 486 s (83 s). Each configuration has one run, so no variance, confidence interval or causal whole-job reduction is claimed.
- **Headroom.** PR154 leaves 720 − 493 = **227 s**. The brief's sample envelope is 15 s allowance + 90 s step + 30 s post steps = 135 s, which leaves 92 s of slack. That slack is only 9 s more than the 83 s spread above, so the margin is thin.

## Why neither a silent pass nor a mechanical abandon is honest

The declared rule is "its PR run and the next main run each show a Windows job reduction of at least 100 s; under 60 s abandons". It names no baseline. The committed C1 text measures against "the 598s/151s reference", and against that the PR run passes with 105 s. Before the run, `5ba2cf6` adopted dual baselines (P1). P1 sent any result that depends on the chosen reference to a coordinator decision. This result straddles both thresholds: 105 s passes and 49 s abandons. The coordinator's conservative reading (49 s, below 60 s, abandon) is legitimate and is the fallback.

The underlying problem: a 100 s **whole-job** delta from one run cannot be measured when adjacent runs already differ by 56–83 s in steps C1 cannot touch. The 100 s threshold was there to give the sampler headroom, not to certify a speedup. That case supports moving the threshold explicitly. It does not support pretending the threshold was met.

## Proposed revision (narrow; replaces only the C1 acceptance and stop rule)

- **R1, merge gate (PR run).** The PR run must meet all of these:
  - the Windows install step saves at least 100 s against **both** 151 s and 139 s, i.e. it takes **≤ 39 s**;
  - the browser and Electron smokes pass;
  - all five PR gates are green, and the latest exact main is green when freshly checked.

  The run measured on `5ba2cf6` meets R1 with 7 s. That timing evidence stays pinned to run 43818081980. A later docs-only head needs its own normal gate run. Report that run's timing; it cannot replace or improve on the pinned evidence. If its install takes more than 39 s or a smoke fails, hold.
- **R2, sampler dispatch gate (next main).** This applies to the first push CI run on the exact merge SHA, with no rerun and no other CI change in between. All of these must hold:
  - the run is green overall;
  - the Windows install takes ≤ 39 s and both smokes pass;
  - the Windows job takes **≤ 500 s**, which leaves at least 220 s: the 135 s envelope plus 85 s, covering the largest observed 83 s spread.

  If the job takes 501–585 s, the sampler's own E ≤ 600 s gate would still admit a sample, but the cushion is smaller than the observed spread. That outcome goes to a coordinator decision, which Sol reviews. Above 585 s, or on any failure, the sampler stays held. No rerun is allowed in any of these cases.
- **R3, stop rule.** C1 is abandoned (reverted in its own PR) if the install saves less than 60 s against either baseline (install over 79 s) on the PR or next-main run. The whole-job delta is reported but is no longer a criterion.
- **R4, claims.** The only claims allowed are an identifiable install-step saving of 132–144 s on the observed pair and the absolute headroom actually measured. No claim may say C1 reduces Windows job time by ≥ 100 s or by any fixed amount, or that hosted media works: the smokes do not exercise media playback.
- **Unchanged:** the 12-minute cap, the required smokes, the five gates, no reruns, no caching or other optimization, the `8a7caaa` conditions for implementing the sampler and its independent review, the eight quarantined fixtures, M2 open and M3 unchecked. All of this is **unverified on real hardware**.

## Conditions and fallback

- Until Sol accepts R1–R4 in writing, the hold stays: no merge, no sampler work, no rerun. The revision takes effect only through that cross-provider acceptance. This review cannot apply it.
- **If Sol rejects it: ABANDON as declared** (conservative 49 s reading). Close PR154 unmerged, leave main unchanged, and keep the sampler waiting. Next work resumes the bounded A0/PR149 track rather than caching or other CI optimization.
- **Limits.** One run per configuration. The 708 s main figure in the brief is still unread. A slow next-main runner can fail R2 purely from noise; under R2 that means a hold, never a rerun.
