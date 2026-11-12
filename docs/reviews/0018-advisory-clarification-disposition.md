# Owner CLI brief advisories before S2

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12. Documentation only;
no implementation, behavior test or mutation was run. Core acceptance stays
at 29068df; this clarification needs independent Opus review before delivery.

The final [correction-2 recheck](0018-owner-cli-second-correction-opus-recheck.md)
passed the blockers and left five non-blocking advisories. This delta handles
them without changing the authority or release boundary:

- N1: serialization into the owned Vec of three string fields has no reachable
  ordinary error fixture. Remove that impossible test requirement and retain
  unexpected Err to Io plus wipe-on-return, without adding a fault interface.
- N2: remove the unused production PostCreate enum. Tests may record method
  order through a private test-only enum.
- N3: give PostCreateOps borrowed-handle method signatures, name the concrete
  NativeFileOps separately from owner CLI NativeOps, and include DACL presence
  in Windows checker facts. No path, create, reopen or delete operation is
  available through the native adapter.
- N4: add real Unix wrong-mode and wrong-length negatives beside the existing
  hard-link fixture. All use owned temp and synthetic credentials. Deliberate
  invalid fixtures retain files without repair or rollback; they do not claim
  the intentionally invalid mode is 0600.
- N5: reserve an escaping-aware checked bound before RNG and require stable
  JSON capacity/pointer across serialization on both platform endpoints.
  Round-trip the Windows backslashes. Mutations cover missing capacity and
  omitted Unix mode/length checks.

No valid owner process, NativeOps, production create, privilege or service
path is introduced. Wipe ownership still precedes RNG; the exact token text
is hashed before clearing; exclusive creation still starts the Partial
boundary; fixed outputs, no rollback and old-config revalidation stay binding.
Candidate tests above are requirements, not passing evidence.

Validation: UTF-8/LF, relative file links, stale-tag/error-fixture search,
retained-report byte comparisons and whitespace. No Rust build or native
operation. E2 is draft PR46; all implementation gates remain closed.
Installed/native/dedicated paths remain **unverified on real hardware**;
M2 is incomplete. The original Opus design keeps its independent Sol reviews;
fresh Opus review covers only this Sol clarification.

## Publication follow-up after the PASS review

Opus review 1c80075 passes the clarification and retains five advisories.
The published candidate fixes them directly: current ADR reviewer/status
wording with explicit historical sections; exact end states for deliberately
invalid Unix fixtures; a named cfg(test), per-thread scoped AllocationObserver
inside the real serialization routine, exercised with both endpoint strings
on every OS; checked-bound overflow to Io before owner/RNG without an
unreachable fixture; and exact PR46 head in current status. Stale tag wording
and long prose lines are wrapped. No behavior test was run; the published-head
review must verify these small follow-ups. Actual parent is main 1d828ca.
