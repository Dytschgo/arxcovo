# ADR0018 second correction disposition

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12. Docs only,
correction attempt 2. Proposal remains proposed pending independent Opus
review of this delta. Original design and first correction remain attributed
to Opus; their independent Sol reviews are retained.

- A-SP1: initialized raw/token/JSON storage belongs to the wipe owner before
  RNG. Partial-fill RNG errors, serialization errors, subsequent ordinary
  failures and unwinding drop it. Planned tests inspect only a boolean from
  the shared wipe routine before storage release; no secret-output seam.
  The digest still hashes the exact token text before clearing.
- A-ST1: replace pre-step injection with private operation-result adapters,
  shared Partial classification, native-result propagation cases and prefix
  write failures. Require production Windows checker fixtures for each
  rejection predicate, including an actual deny ACE in synthetic facts, and
  mutations of native-result handling and checker invocation. Retain Unix's
  real hard-link fixture. Before successful Mode, Unix tests assert privacy
  rather than exact 0600, which can be reduced by umask.
- The earlier PASS dispositions for A-SP2/A-SP3/A-A1/A-A2 stay unchanged.
  Exclusive-create boundary, no rollback/reopen, fixed outputs, old-config
  revalidation and private first-byte creation remain binding.

Validation performed: Git-object extraction, UTF-8/LF and relative-file-link
checks, stale-hook-reference search and git diff --check. No behavior test,
mutation, build, native or privileged action ran. The tests above are
requirements for future implementation, not passing evidence.

E1 merged through all five required gates as PR40, main 884a301. E2 remains
held by unexplained ExecutionFailed run bdbf032b; three instrumented passes
did not fix or waive it. No CLI implementation may begin until all contract
and library gates hold. Native, installed and dedicated M2 remain
**unverified on real hardware**; M2 is incomplete.
