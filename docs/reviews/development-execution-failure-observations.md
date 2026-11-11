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
