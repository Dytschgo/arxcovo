# PR185 prospective validator: first hosted lint failure

Head `b0a986f9ebaeda508467450224a8cd39eaa60c3e`, normal first CI
[44395332475](https://github.com/Dytschgo/arxcovo/actions/runs/44395332475), attempt1.
All three UI jobs failed `npm run lint`: added test line198 used bare
`structuredClone`, which is not a declared ESLint global. Existing tests use
`globalThis.structuredClone`. No linter rule, policy threshold or production
implementation defect was established by this error.

Failed jobs: Ubuntu135123584437, macOS135123584581, Windows135123584610. Bounded
logs are retained in `D:/Temp/arx-ci-split-b0`, with byte sizes/SHA256 in
log-manifest.json; every fetch exited zero without truncation (8MiB/45s bounds).
first-failure-run.json and first-failure-jobs.json retain the incomplete-run
snapshot: Rust Ubuntu succeeded, Rust Windows/macOS were still running. No
complete Rust or remaining smoke result is claimed for this snapshot.

Fresh independent Sol6.1/high source review returned NO-GO with this single
Standards finding; Spec had zero findings after inspecting the exact topology,
gate-time constraint, unchanged arithmetic, fixture and legacy compatibility.
No attestation/status was published for this head. Report and pins:
`D:/Temp/arx-ci-split-credit-review`.

Coordinator reproduced the exact lint error locally before correction, then
changed only that test reference to the existing `globalThis` convention.
`npm run lint` passed in2.780s; all60 Node script tests passed in6.153s, with
zero failures/skips/cancellations. `git diff --check` passed. Evidence:
`D:/Temp/arx-ci-split-lint/{before-lint.log,after-lint.log,after-node.log,results.json}`.
Existing dependencies were reused; no install, rule suppression or runtime test.

This is one bounded CI-only test correction, not a transport/engine fix. The
original949/null-policy HOLD and prior history remain unchanged. Corrected source
requires fresh exact-head review and its normal first hosted attempt; no rerun or
dispatch is authorized. Any supersession follows unchanged PR concurrency, and
unfinished old-head jobs must not be reported as passed. EngineSol3/3 and
historicalOpus0/2 unchanged; M2 OPEN/M3 UNCHECKED, relevant paths **unverified on
real hardware**. OAuth, host setup and babysitter untouched.
