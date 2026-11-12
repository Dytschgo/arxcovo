# Intermittent development execution failure: observation checkpoint

Status: unresolved; no cause established and no fix attempted.

The observed symptom is `ExecutionFailed` in the real Windows development
executor during full-workspace nextest runs. Earlier failures affected cancel,
overflow, the junction fixture and the SDK positive control. Passing unchanged
reruns do not establish a repair.

## Reduced reproduction work

Anthropic / claude-opus-5-5 / requested high reported 124 isolated real-path
runs and three full arxd runs with at most two test processes at once. None
reproduced `ExecutionFailed`. Five CPU-starvation failures were instead
`Timeout` versus `OutputLimit`; these are a separate symptom. No hypothesis
or production fix was established from those results.

## Full-concurrency checkpoint

Codex / gpt-6.1-sol / medium ran ten sequential invocations of
`cargo nextest run --workspace --locked --no-fail-fast` at source
`f15a271e2b1807267edddb3a5115727fa1354401`, using the ordinary default
concurrency. Temporary diagnostic instrumentation captured fixed stage labels
and numeric OS errors at failure sites, without paths, arguments, environment
or identity values. No stress processes were added.

- Nine invocations passed all 150 tests; two subprocess-only fixtures were
  skipped directly and exercised through their parents.
- Invocation three passed 149 and failed the existing handle-slot reuse test.
  This source predates merged PR #37; that failure is separate from the target
  symptom.
- No invocation produced `ExecutionFailed` or a diagnostic error-stage line.
- Original source bytes were restored after the bounded run. Instrumentation
  is not part of this change. No retry was presented as a fix.

The useful next feedback is a stage/error capture from a natural recurrence;
these observations do not justify speculative changes or weakening assertions.
The Unix integration still needs its current-parent refresh, independent review
and hosted checks before publication. No production constructor, owner/admin,
SCM, LSA, rights, account, service or VM operation ran. Installed-service and
dedicated-account M2 paths remain **unverified on real hardware**.

## Current-main E2 port checkpoint — 2026-11-12

Coordinator retains Opus5.5 high bounded diagnostic results on exact780ca4164494f6c3da00a340f4c4c67e3ac9d305. The uninstrumented E2 port runbdbf032b-658a-4123-a279-303cb40566ed failed at windows.rs956:67 in results_count_dropped_output_and_report_nonzero_exit; the first overflow sub-case returnedExecutionFailed (output_bytes100).116passed,1failed,132notrun,2skipped. No failing phase, Win32 code or cause is established.

Three instrumented default-concurrency full workspace runs then passed249/249,2skipped each: dbf64cd7-721a-4315-aa89-25db2e70d816 (9.898s),2e0cabc5-0541-414c-a677-1cc69b7e5121 (10.509s),760d71a7-3500-49f0-bbf1-ffeccacb2bd9 (10.718s). NoExecutionFailed or diagnostic failure-stage line appeared. A focused child-token-mismatch parent test demonstrated fixed-tag instrumentation output; no ignored fixture was run directly. These passes are evidence acquisition, not a fix, not a final uninstrumented pass and not a waiver of the original red. No hypothesis is confirmed and no regression fix was made.

The retained [instrumentation artifact](development-execution-failure-instrumentation.patch), SHA25660e9ab39075e07cd4acc08ac530b672353efb1d1250a3445372fac38cbd5d71e, captures fixed phase/site labels and numeric OS errors before cleanup, plus bounded numeric child/job facts after failure. It prints no paths/argv/environment/identities/child output. It is TEMPORARY diagnostic source, not a product patch: apply only for a justified natural local verification, capture a recurrence, then restore before committing source. No debug tag remains in crates. E2 publication stays held while its unexplained red is unresolved. No more blind stress/rerun loop is authorized by this note.

No GitHub, privileged constructor, SCM/LSA/account/service/owner admin/VM/Lab Manager/native experiment or private-state operation occurred. Native/installed/dedicated M2 remains **unverified on real hardware**.

## New-parent uninstrumented integration checkpoint — 2026-11-12

OpenAI / gpt-6.1-sol / high integrated the six reviewed E2 files unchanged on
basefbb1813 (inheriting merged E1/N3/N2/artifact CI). At source2986cad, exactly
one `cargo nextest run --workspace --locked --no-fail-fast` at default concurrency
passed259/259,2 skipped,9.908s, rund78206ad-1919-4183-8614-0098d379edd2.
No instrumentation or direct ignored fixtures; no full rerun or stress loop.
This supplies new-parent integration evidence only: the original red remains
unexplained, no cause/failing phase/Win32 code is established and no fix or waiver
is claimed. E2 stays **HELD** pending independent Opus integrated-head assessment
and coordinator publication disposition. [Exact evidence](../evidence/e2-current-parent-integration.md).
