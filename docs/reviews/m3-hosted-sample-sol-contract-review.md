# Sol review: bounded hosted Windows sample

Target: Opus2e3a388 retained181604c, on main58d06c1. Main CI43815774741 is independently confirmed green on that exact live SHA. Standards/Spec/Security conditional: sample implementation remains blocked on the corrections below. C1 alone is approved as a separate measured workflow probe, subject to independent review and smoke/gates.

C1: preserve non-Windows Playwright install with --with-deps, use Windows install without that flag, and pin both branches in the existing workflow test. Keep every smoke/test and the12min job cap unchanged. Do not assume the151s install step is all feature-install overhead; record actual PR Windows install/job timings against the598s reference and inspect next main. Under60s savings abandons this candidate;60–100s requires an explicit evidence decision; at least100s supports proceeding to the next sample-contract review. No caching or extra optimization scope.

Sample blockers (settle before implementation):

- B1: the proposed last step has two native Python commands in a PowerShell multiline block, repeating the already reproduced failure-mask bug. Require && or explicit exit checking so wrapper tests cannot fail silently before the wrapper runs.
- B2: the diagnostic/gating split is inconsistent. A hung or crashed test can prevent JUnit being written or leave a partial document after global cancellation, but junit_missing/junit_parse/deadline_run currently gate. Verify actual nextest global-timeout exit/report behavior, then make every expected test-caused timeout/abort/partial report a bounded diagnostic outcome, while genuine selection/job-ownership/wrapper integrity errors gate. No test behavior may quietly become a required-job failure through the parser.
- B3: E from an anchor plus15s allowance is not a hard job-start timestamp, and30s post-step allowance is a measured estimate. Avoid the claim that the step can never push the job past its cap. Use a verified job-start timestamp or explicitly conservative measured admission with the existing720s external cap and kill-on-close ownership; state residual CI overhead risk. No timeout increase.
- B4: child waiting on a go byte must itself have a bounded wait and parent-death behavior. Job assignment can fail before the child belongs to the kill-on-close job. Explicitly reap that owned child on assign failure before returning; never leave it blocked outside the job. Likewise handle cancellation between spawn and assign.

Output must use actual closed numeric/enum grammar, not merely an ASCII character allowlist and a recognized prefix. Every released value must be validated against its expected field type; rejected data stays private and is counted only. Preserve the8KiB D1 limit and aggregate bounds. No hosted sample is authorized by this conditional review.

C1 adds no source fix or re-enable. M2 stays open; M3 unchecked, all8 Windows fixtures remain quarantined and **unverified on real hardware**. No build/test ran in this design review; later workflow checks must be meaningful and independent Opus review must cover Sol changes.

## Recheck of Opus26ac2d6 (retained9ab5dc0)

B1–B4 and typed-output grammar are closed at the contract level. Standards PASS; Spec/Security PASS for implementation only after measured C1 headroom. Assignment failure owns and reaps the blocked child before go, with parent-death/5s watchdog; test-caused global cancellation/partial report is diagnostic, while inventory and cleanup integrity failures still gate. The adaptive admission now honestly admits timestamp/post-step residual risk under the unchanged external720s cap. Accept deadline_list and cleanup as integrity failures rather than test acceptance outcomes.

Implementation clarifications: enforce a single90s absolute whole-wrapper budget in addition to phase budgets, including child launch/assignment and error cleanup. Derive and freeze the actual ordered step list on the post-C1 base, not the historical literal25; C1 splits one install step into two. Under python -I import EXPECTED from the fixed guard path with importlib (not an ambient-module import), validate that import reads the intended ordinary repository file, and reject reparse points. No input selects argv, paths, IDs or module code. These clarify the existing contract without broadening source/workflow scope.

No sample code is started yet. PR154 timing and next-main confirmation must meet the declared C1 criterion before dispatch. Independent review of implementation and actual local process-tree/assign-failure controls remain required; this document is no native verification or authority for a product fix/re-enable.
