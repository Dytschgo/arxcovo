# Read-only Windows SYSTEM-refusal regression

Coordinator: OpenAI / gpt-6.1-sol / medium, 2026-11-12.
Base: `4c2264d6c3caee9507e11870ad361daea6c1dfdf` (PR42).
Original source: `6d946606bf8724ea4af4538973dcbf17366c3712`.

Applied only the original 13-line cfg(windows) test in native.rs. No production
statement changes. The retained [Opus review](../reviews/service-system-guard-test-review.md)
is byte-identical to the report at 51a84e1. Its mutations and checks retain
their original pins; no fresh mutation run is claimed.

The test reads the current process identity, reduces it to a boolean and
compares only Result<(), Error>. It does not format a SID or enter startup,
config, Native/Application construction, OwnerGuard/session, SCM, LSA,
account, privilege or service paths. No pipe opens or owner administration.

The original S2-1 documentation finding is resolved by excluding its entire
old PROGRESS hunk, including the unrelated N3 section. Current status is
recorded separately with all inherited history kept. The S2-A1 wording is
honest: ordinary-user runners cover non-SYSTEM refusal, not every policy
direction, the exact SID constant, current_identity correctness or guard
calls at every startup site. Real SYSTEM acceptance remains untested.

Actual coordinator checks on the port, all exit 0:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- `cargo nextest run --locked -p arxd -E
  'test(=service::native::tests::service_identity_guard_matches_process_identity_without_startup)'`:
  1 passed, 242 not run (filtered/ignored), run
  `f706593d-1035-4090-9481-345537fe69f1`, 0.018s test phase.
- Patch applicability, scoped diff, UTF-8/LF/relative links and whitespace.

No full suite repeat for this unchanged-production, test-only port. Fresh
Opus integration review precedes publication; all hosted gates remain required
for merge. No new mutation, native dedicated or installed-service test,
ignored fixture invoked directly, VM/Lab Manager, private-state or experiment
operation. This is reversible regression coverage, not a production change.
Installed/native/dedicated paths remain **unverified on real hardware**;
M2 is incomplete and E2's unexplained red run remains held.

## Publication refresh after artifact CI merged

Actual base: b3e1d13888ae5967b095d4151144aa240169ed77 (PR43).
The base product tree is unchanged from original port base4c2264d; #43 adds
only the reviewed workflow step and docs. native.rs is byte-identical to
reviewed a1ce4cf. Both reports are retained unchanged, including the
[current integration review](../reviews/service-system-refusal-current-main-review.md)
from 1fc027e. Its focused run f4f8f108 passed 1/1; this remains credited to
a1ce4cf, not the refreshed candidate. The missing run-ID advisory is resolved
in the new status: main after #41 was run43190243712, 481s.

No unchanged Rust checks were repeated for this ancestry/docs-only refresh.
Git byte, UTF-8/LF/relative file-link and whitespace checks passed. Main after
#42 passed in596s (43192357855); after #43 run43194236390 is pending. Fresh
published-head independent recheck and all hosted gates are still required.
