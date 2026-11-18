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

## Terminal follow-up before corrected-source publication

The old b0 attempt completed FAILURE in504 seconds wall before7141c1dc was
pushed. Rust Windows427s/709PASS, Ubuntu146s/695PASS and macOS189s/663PASS, with
existing36/22/8skips. All three UI jobs failed lint, and all three verify gates
failed closed. All nine logs and final metadata/counts/durations/hashes are now
retained under the same D root in all-log-manifest.json and
final-execution-proof.json. No supersession cancellation occurred.

Corrected7141c1dc received fresh independent Sol6.1/medium Standards/Spec GO,
[attestation](https://github.com/Dytschgo/arxcovo/pull/185#issuecomment-6991559983)
and agent-review success59301105218. Its normal first CI44396900780 is a separate
incomplete observation; this source review does not establish hosted acceptance.
