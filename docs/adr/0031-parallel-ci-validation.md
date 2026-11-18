# ADR 0031: parallel Rust and UI validation

Status: proposed; independent review and first hosted measurement pending.

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
12-minute timeout. There are no source, nextest, quarantine or timeout changes.

Preserve the three required `verify (OS)` names as unconditional, fail-closed
completion gates. Each gate runs even when a dependency fails and requires both
complete worker matrices to have succeeded. Failure, cancellation or skipping
any worker prevents every required verify check from passing. This deliberately
uses all-platform dependency results rather than attempting to infer per-matrix
success from a single aggregated output. Branch protection is unchanged.

The existing charged-v1 validator in ADR0025 accepts exactly the old three-job
layout. Leave its code and arithmetic unchanged. Nine-job split runs therefore
receive `unavailable:jobs-invalid` and the original **900-second wall-time** rule,
with no queue credit. Historical three-job runs retain their existing measurement.
Do not broaden metadata trust to claim a queue-only improvement. If a split main
exceeds 900 seconds, its wall baseline blocks ordinary merges; this change does
not supply an exception or a creditable candidate above that baseline.

The unchanged optional Windows sample remains last in the Rust worker, with its
anchor first in that worker. Removing sequential UI work before it can change
whether its existing budget admits a sample. Keep the wrapper, bounds, ownership,
selection and admission logic byte-identical. Automatic observations grant no
stability, re-enable, repair or exhausted-attempt credit.

## Verification and delivery

Pin every validation step to its owning phase and test gate dependencies and
failure behavior. Assert conservative budget fallback for the new layout. Before
merge, require independent exact-head review, all existing required checks, first
attempt wall time at most 900 seconds and the existing Windows feature limit for
its workers. Retain bounded per-job logs, counts and durations on D:. Report total
run wall time and longest execution separately; neither proves a root cause.

After the CI-only PR merges, verify its exact-main first attempt before merging
that main into PR182 and pushing a normal source-preserving refresh. No rerun or
dispatch, trigger-only commit, code-fix attempt, counter reset or gate bypass is
authorized by this decision. Engine Sol3/3 and historical Opus0/2 remain unchanged.
The pending Claude Code OAuth request is independent and stays untouched.

## Consequences

Six workers and three short gates use more runner assignments and may increase
queue delay. First-attempt hosted evidence decides whether this split meets the
existing limits. Gates need no runtime artifact transfer. Reverting this CI-only
change restores the previous topology; no application data or schema is changed.
M2 remains OPEN and M3 UNCHECKED; relevant hardware paths remain **unverified on
real hardware**.
