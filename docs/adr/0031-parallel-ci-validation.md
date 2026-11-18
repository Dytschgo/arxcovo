# ADR 0031: parallel Rust and UI validation

Status: merged as e11b0767; reviewed7141 first CI qualified627wall/622charged, own first-main qualified920wall/827charged/93idle. Original first attempt remains held.

## Context

On 2026-11-18 the owner's delegated oversight explicitly authorized a separate
`ci-speed` change, then a normal PR182 refresh after the resulting main is green.
PR182's first attempt reached the 12-minute job limit after its Rust cases passed.
The retained disposition establishes the cancellation mechanism; the underlying
slowdown cause remains unknown. Splitting sequential validation is a structural
mitigation to measure, not a demonstrated runtime fix.

## Decision

Run Rust/platform validation and Node/UI/browser/Electron validation in parallel
on each of Ubuntu, Windows and macOS. Each phase retains its commands, pinned
tools, platform conditions and internal order. Each worker keeps the existing
12-minute timeout. Application source, nextest, quarantine and timeouts are unchanged.

Preserve the three required `verify (OS)` names as unconditional, fail-closed
completion gates. Each gate runs even when a dependency fails and requires both
complete worker matrices to have succeeded. Failure, cancellation or skipping
any worker prevents every required verify check from passing. This deliberately
uses all-platform dependency results rather than attempting to infer per-matrix
success from a single aggregated output. Branch protection is unchanged.

The initial implementation left ADR0025's exact-three-job validator unchanged.
Its first attempt passed every job but took 949 seconds wall, received no credit
and was held against this ADR's initial 900-second wall requirement. That outcome
remains recorded in [the original observation](../evidence/parallel-ci-first-attempt-held.md).

A fresh independent disposition supports adapting the existing owner-accepted
charged-v1 principle prospectively. Accept only two closed layouts: the unchanged
legacy three-job layout, or exactly six Rust/UI workers and three verify gates
matching the pinned workflow. Worker labels must match their OS; every gate label
must be ubuntu-latest. For the nine-job layout, each gate's creation timestamp
must be at or after completion of all six workers. Reject early-created gates
even if they start later. Keep all existing run/head/repository/attempt/identity,
strict timestamp, bounded-read and complete-job checks.

Use the unchanged interval sweep over every job, including gates. Credit only
created pending jobs while none executes; charge execution, overlapping waits,
no-pending gaps, prefix/tail and idle time before gates exist. Keep the threshold
at **900 charged seconds**, green-run/all-jobs eligibility, and original wall
fallback for invalid layouts or metadata. Preserve strict charged-plus-longest
reduction for ci-speed above a slow baseline. No arbitrary topology, limit rise,
status exception or history rewrite is allowed.

This explicitly replaces the initial split-only wall restriction for a NEW
reviewed source head and its normal first attempt. It cannot qualify the held
949-second run retrospectively. Its counterfactual geometry is 165 idle seconds,
784 charged seconds and 441 longest execution; 784 exceeds the base's 603 charged
seconds, so it is not a charged-plus-execution speed reduction. This adaptation
does not promise a better future measurement or establish a queue-capacity cause.

The unchanged optional Windows sample remains last in the Rust worker, with its
anchor first in that worker. Removing sequential UI work before it can change
whether its existing budget admits a sample. Keep the wrapper, bounds, ownership,
selection and admission logic byte-identical. Automatic observations grant no
stability, re-enable, repair or exhausted-attempt credit.

## Verification and delivery

The standing owner rule, reiterated2026-11-19, recovers a proved zero-step,
unassigned-runner acquisition cancellation on the unchanged head without owner
approval. It is not a test rerun/fix attempt. Recover only the affected worker
and dependent gates; retain successful workers and original failed-attempt
metrics. Hourly recovery is bounded to four requests if acquisition fails again.
This first-actual-execution exception does not change the budget evaluator,
history, required checks, source review, timeouts or quarantine. Real test/step
failures remain outside it. See PROGRESS Main CI health for the durable rule.

Pin every validation step to its owning phase and test gate dependencies and
failure behavior. Assert exact-layout/time validation and conservative fallback
for any defect. Before merge, require independent exact-head review, all existing
required checks and the NEW head's normal first attempt at most 900 validated
charged seconds (or wall seconds if credit is unavailable), plus the existing
Windows feature limit for its workers. Full wall time must always be reported.
Retain bounded per-job logs, counts and durations on D:. Report longest execution
separately; these measurements do not prove a root cause.

After the CI-only PR merges, verify its exact-main first attempt before merging
that main into PR182 and pushing a normal source-preserving refresh. No rerun or
dispatch, trigger-only commit, code-fix attempt, counter reset or gate bypass is
authorized by this decision. Engine Sol3/3 and historical Opus0/2 remain unchanged.
The pending Claude Code OAuth request is independent and stays untouched.

## Consequences

Six workers and three short gates use more runner assignments and may increase
queue delay. First-attempt hosted evidence decides whether this split meets the
existing budget semantics. The validator extension must receive fresh independent
review before adoption; original source GO does not cover it. Gates need no
runtime artifact transfer. Reverting this CI-only change restores the previous
topology; no application data or schema is changed.
M2 remains OPEN and M3 UNCHECKED; relevant hardware paths remain **unverified on
real hardware**.
