# E2 draft publication on main after PR45

Coordinator: OpenAI / gpt-6.1-sol / medium, 2026-11-12.
Actual base: `2b805c450880fd12b9f0cd9771974e4fa1337a55`, PR45 merged
10:11:09 UTC through all five gates. Main run 43198102900 is pending.

The [independent assessment](../reviews/0016-e2-current-parent-assessment.md)
at target 000835d recommends releasing only the publication hold for a
draft/candidate PR. The coordinator adopts that limited disposition. Merge
is still held for an exact-head independent status, every required hosted
gate and a separate explicit coordinator decision addressing the open issue.
Any hosted failure stays held with its logs/run ID; no blind rerun is allowed.
No check is bypassed, retried to green, weakened or reclassified.

## Source and parent identity

All six E2 crate files are byte-identical at this candidate, builder head
000835d, tested 2986cad, corrected reviewed 1103a66 and prior port 780ca41.
The [builder manifest](e2-current-parent-integration.md) gives their blob IDs.
No source outside those six files changes against actual main. The new
parent's SYSTEM test, state-exit classifier, N3/N2, client and artifact CI
remain inherited unchanged. The development executor source is byte-identical
to the failing port. No runtime caller or CLI/native activation is added.

The E2 files were extracted narrowly from the builder, without its PROGRESS.
Current state was rewritten by hand, preserving actual main's entire status
history below a historical heading. ADR0016, the brief and foundation guide
mark the original fbb1813 parent as historical and state the actual parent
and candidate-only disposition. This addresses assessment R-1/R-2.
Retained independent reports are unchanged; review and mutation credits stay
at their original revisions. The diagnostic patch is a documentation artifact
only and was never applied during these checks.

## Actual refreshed-source checks

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- Exactly one uninstrumented default-concurrency full run for this final
  parent: `cargo nextest run --workspace --locked --no-fail-fast
  --status-level fail --final-status-level fail`: **261 passed, 2 skipped**,
  run `0a145e0e-8342-4c10-8e52-640b0087a72d`, 10.092s test phase.
- Six-file byte comparison, unchanged development source, UTF-8/LF/relative
  file links and `git diff --check`: passed.

This is the first combined run with the newly merged SYSTEM test and state
classifier, not an unchanged retry of a red run. The builder's distinct
259-pass run on its earlier parent stays credited to 2986cad. The ignored
subprocess fixtures were exercised through normal parents, never directly.
Existing owner/installer junction fixtures use owned temp directories only.
No production guard/install/SCM/LSA/account/privilege/service, owner admin,
VM/Lab Manager, private-state or experiment operation ran. No instrumentation,
stress, timeout/concurrency change, skip or source mutation was used.

## The unresolved failure is retained

Original uninstrumented run bdbf032b at 780ca41 remains red and unexplained.
Its overflow subcase returned ExecutionFailed; phase, Win32 code and cause
remain unknown. The assessment also cites an occurrence before E2 on main.
No observation proves E2 does or does not change its rate. Neither prior
instrumented passes nor either new-parent pass proves a fix or waives the
open issue. Debugging remains at feedback-loop phase 1, without a reliable
recurrence; no speculative production fix or regression claim is made.

Hosted verification is still missing for this head. Publication creates no
installed-service, dedicated execution or M2 acceptance. Native, installed
and dedicated paths remain **unverified on real hardware**.
