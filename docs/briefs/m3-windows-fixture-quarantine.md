# Windows three-node fixture quarantine revision

Owner scope decision 2026-11-17: cover all tests using the three-node fixture,
including issuance; keep required on Ubuntu/macOS. Supersedes R5-only scope.
Root Sol implementation contract; independent Opus review remains required.

Quarantine the 13 async fixture tests in consensus/three_node.rs and all 11 async
fixture tests in consensus/issue_tests.rs on Windows only. Keep bodies compiled,
unchanged assertions/deadlines/helpers/production. The pure read-attribution unit
in three_node.rs does not create a cluster and remains required everywhere.
The three opt-in diagnostics controls are already ignored on every OS and keep
their existing status; do not add a new platform exclusion to them.

Keep the original eight sampled names and their ignore reasons byte-identical.
The non-gating bounded diagnostic sample remains exactly those eight; it is no
Windows acceptance evidence. Exclude the other five three_node fixture tests by
fixed exact-name FILTER terms (no runtime argv from guard). Issuance tests are
outside the sampler namespace. State this sample exception and all coverage loss.

Guard the full 24-test boundary: eight sampled, five other three_node fixture
tests, eleven issuance tests. Each must have one Windows-only ignore with pinned
reason; no Unix ignore, cfg exclusion, removed/renamed/duplicate gate or newly
unaccounted fixture test may pass. Keep pure PKI tests and module bindings required;
replace the old blanket issuance-ignore ban only with the exact authorized set.
Include meaningful mutation checks for each fence and sample-exclusion mapping.

Allowed source: three_node.rs and issue_tests.rs attributes only; quarantine guard,
windows_diag_sample.py FILTER constant only, sampler tests. Docs: ADR0024 addendum,
sample brief, PROGRESS current health/coverage checkpoint, this brief, evidence.
No workflow/Cargo/policy/settings/retry/timeout/scheduling/production edits.

Local checks: guard and sampler suites, fmt, workspace all-target locked/offline
Clippy; real nextest listing proves24 fixture tests Windows-ignored and exactly8
sample matches. A pure attribution unit remains unignored. Ordinary fixture-only
selection runs0tests (nextest expected exit4); no claim engine behavior passed.
Required hosted Ubuntu/macOS must execute the24 tests, not skip them. No hosted
rerun. Sol sharedfix3/3, Opus0/2 unchanged: exception, not fix.

Keep accurate currentmainc612152 RED line and PR164I4failure evidence. Main stays
red until merged recovery first-attempt exact-main CI succeeds. ci-time-budget
still blocks using1475s wall;738s charged is uncredited because red. Owner must
approve one budget-only protection exception for final exact head after other
four gates pass. Do not use ADR0025 bootstrap or #57 exception without new scope.
No manual status, ci-speed label or bypass. No feature merge before green main.

Opus injected-latency follow-up reproduced exact I4 once; counterfactual passed.
Hooks are removed. Cause on hosted runner and reproduction rate are unproven.
Re-enable as one reviewed fixture suite only after a separate bounded timing
recovery plan with meaningful slow-storage evidence and hosted acceptance;
no further per-test quarantine expansion. M2 open, M3 unchecked; Windows fixture
coverage is unverified on hosted Windows and **unverified on real hardware**.
