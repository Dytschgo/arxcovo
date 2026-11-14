# M2 A4 fork-contract correction recheck — Muse (independent)

2026-11-14; reviewer Muse (Muse Spark, high effort), independent
recheck of the narrow documentation correction against retained prior
Muse review. Document only; no source, build, test, native call,
dispatch, fork, process, network (beyond the primary man pages already
cited in the prior review), publication, push, PR, or subagent action.
Read-only git CLI used for pin/diff verification only.

## Reviewed targets (exact pins)

- Correction under recheck:
  `e5790650f0da7714b4b7d87808f2c2ba22dd21a3` —
  `docs(adr): bind A4 fork closure and descriptor ownership`.
- Retained prior review: `a1a7342ff72bd10a5c2d38863e6f373059c34b2a` —
  `docs(review): independent A4 single-task helper review`
  ([m2-a4-single-task-helper-muse-review.md](/abs/path/docs/reviews/m2-a4-single-task-helper-muse-review.md),
  findings F1–F4).
- Head: `0ac5d41cb1aee294ca7d3416ffea96b6e9134b37` —
  `docs(progress): queue tightened A4 helper recheck` (PROGRESS only,
  2 insertions; out of scope for this recheck, not edited here).
- Requested branch `work/m2-janitor-source`; local checkout resolves the
  same head `0ac5d41` (verified by `git log --oneline` and
  `git show --stat HEAD`).
- Correction delta verified by `git show e5790650`: exactly two files,
  no other tree change:
  - [0020-linux-execution-hierarchy-and-janitor.md](/abs/path/docs/adr/0020-linux-execution-hierarchy-and-janitor.md)
    (D9a binding 1 extended; bindings 2–5 untouched).
  - [m2-a4-single-task-helper-proposal.md](/abs/path/docs/reviews/m2-a4-single-task-helper-proposal.md)
    (new `Binding source-review clarifications` §1–4 plus the required
    pipe-ownership table).

## Standards — PASS

- One-concern conventional commit (`docs(adr)`), two-file scope (ADR
  binding + its correction text), explicit no-acceptance / no-run
  boundary, and explicit **unverified on real hardware** labelling
  satisfy [AGENTS.md](/abs/path/AGENTS.md).
- No product-approval flow, no public-publishing default change, no Lab
  Manager / VM contact, no secret or owner data, no CI-budget or
  review bypass, no PROGRESS edit inside the correction itself.
- No helper framework or speculative approval overhead is added; the
  correction stays at four contract sentences plus one ownership table.
- No Standards findings. This axis grants no implementation or run
  permission.

## Spec — PASS (F1–F4 closed at contract level)

Against prior-review F1–F4 (each required "one minimal contract
sentence before source work"):

- **F1 at-fork audit — closed.** Proposal §1 requires enumerating every
  reachable `pthread_atfork` registration in the linked A4 test binary
  (libtest/std/transitive crates included), proving none exists or each
  is async-signal-safe; the fixture installs and adds none; unknown
  handlers block source acceptance with no empty-set assumption. ADR D9a
  binds this by reference ("including its four source-review
  clarifications") and restates the fail-closed rule ("unknown handlers
  block acceptance").
- **F2 inherited-FD discipline — closed.** Proposal §2 plus the required
  ownership table fixes parent-created `O_CLOEXEC` pipes, child closes
  only fixture-created opposite ends passed as stack FD integers, never
  enumerates or touches another inherited descriptor, holds the rest
  open-but-unused until `_exit`, and forbids dependent EOF claims and
  descriptor/host logging. Table values are correct (ready: parent-read /
  child-write; release: parent-write / child-read; opposite-end closure
  per phase). ADR D9a restates the same discipline.
- **F3 post-fork closure — closed.** Proposal §3 fixes divergent `!`
  child entry always ending in direct libc `_exit`, raw libc calls plus
  stack scalars only, explicit forbidden list (std wrappers,
  unwrap/expect, formatting, implicit allocation/panic/Drop, locks, Rust
  TLS, environment access, callbacks), `EINTR`-only retries with
  recomputed saturating monotonic remainder that never extends the
  absolute deadline, full-graph inspection with sole-caller enforcement.
  ADR D9a restates the list and the saturating-remainder rule.
- **F4 fork placement and single-task proof — closed.** Proposal §4
  fixes fork in the parent's supervisor cgroup outside the pids-limited
  leaf, migration of exactly one child task into the already-limited
  leaf, verification of task count plus live membership/population and
  parent/supervisor preconditions before release, process-existence
  insufficiency, and unchanged D9a numeric bounds with expiry/early-death
  refusal. ADR D9a restates fork-outside-leaf plus
  migration-and-check-before-release.
- No spec behavior requested by F1–F4 is missing; no extra executable,
  Cargo target, production entry, harness, probe, or acceptance grant
  is added. A6 is untouched and stays on its independent contract.

## Security — PASS (fail-closed; source proof still required, not granted)

- **At-fork / linked handlers.** The contract does not assume an empty
  handler set and does not silently accept unknown linked handlers:
  unknown handlers block acceptance at source review. Residual risk
  (a handler reachable only in the linked test binary) is therefore a
  defined source-review gate, not an accepted premise. Proportionate;
  no further doc sentence is required.
- **Inherited FDs.** Open-but-unused plus no-enumeration plus
  no-dependent-EOF is the sound discipline: enumeration/close-all loops
  are excluded, extra held FDs create no tasks, and no handshake depends
  on them. No over-closure risk is introduced.
- **Narrow scalar libc errno exception — explicitly assessed.**
  The exception permits only an immediate scalar libc errno read to
  distinguish `EINTR`; it forbids string conversion and any Rust
  TLS accessor, cites the errno rule in the signal-safety manual as a
  narrow reading (not a general TLS permission), and conditions the
  permission on a verified async-signal-safe accessor/handler contract
  with source-review verification of the exact accessor implementation
  and any signal-handler errno preservation — unknown safety blocks
  acceptance. Assessment: (a) async-safety — the doc does not declare
  any particular accessor safe; safety must be proven per implementation
  (direct TLS read without locks/allocation) at source review, and the
  fail-closed clause makes an unverified accessor a blocker rather than
  an accepted risk; (b) preservation — handler save/restore of errno is
  an explicit review item, so a clobbering handler cannot pass silently;
  (c) scope — "no Rust TLS/to-string" keeps the exception to a scalar
  integer comparison on the `EINTR` path, with `EINTR`-only retries and
  saturating remainder arithmetic unchanged. This is the minimal
  workable contract: forbidding the read entirely would make `EINTR`
  handling unimplementable, while broadening it would reopen allocation/
  formatting/TLS in the child. No change requested; no helper framework
  or extra approval layer is warranted.
- **Deadline/single-task ordering.** Absolute deadline set parent-side,
  child recompute with saturation, fork outside the leaf, and
  migrate-then-verify-membership before release preserve the prior
  review's accepted shape. Scheduling delay remains honestly
  non-guaranteed; expiry/early death still fail.

## Actionable blockers

None. No doc-level blocker remains from F1–F4; the errno exception is
accepted only in its stated fail-closed form.

Source preparation stays blocked until these reviewed docs pass the
ordinary gates and coordinator merge; source review must still prove,
per the bound contract: the at-fork enumeration, the phase-by-phase
FD-ownership table, the full post-fork call graph with the exact errno
accessor and preservation behavior, saturating-remainder construction,
and fork-outside-leaf with live one-task/membership verification. No
implementation, native execution, or acceptance is granted here.

## Verdict

- **Standards: PASS. Spec: PASS. Security: PASS (fail-closed).**
- M2, A4, remaining F/G, and real-Runner PA6 stay open; A6 proceeds
  independently; hardware paths remain **unverified on real hardware**.
