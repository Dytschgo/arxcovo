# Windows owner-library linear port evidence

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12.
Prepared base: `0937cacfcdb5be036edcd1a126ba0f65521599df` (actual merged PR36).
No delegation or GitHub writes. Local stack requires coordinator Opus high
independent review before publication; hosted gates pending. M2 incomplete.

## E1 boundary

The four owner_windows/owner_scm implementation and test files are byte-for-byte
equal to reviewed source `4a5919e61542ee38507a9ba461549a61e4b3a67d`.
Original E1 implementation c3332b7/ff5a198, full independent report42c923d,
follow-up fa6f297 are retained under docs/reviews/0016-*. Prior mutation evidence
is reused only for these identical files, not claimed rerun. The retained E1
evidence had one isolated CP1252 dash; it was converted to UTF-8 without changing
its historical claims. ADR/brief status reflects settled review dispositions.

Executed on this port:

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `cargo nextest run --locked -p arxd -E 'test(owner_windows) | test(owner_scm)'`:
  41 passed, 189 filtered out; run5ad924b0-9736-4f89-a33c-0be97a3ca220.
  Includes S1/U1/U2/U3, overlap/structural error, real owned-temp publication
  and synthetic SCM/LSA tests. No production constructor or ignored child invoked.
- Source byte-equivalence, local Markdown links and `git diff --check`: PASS.

Only owned temporary/fake/synthetic new paths were exercised. Production
OwnerGuard/session/install constructors, fixed namespace, real SCM/LSA/restore,
service/account/rights and installed paths are **unverified on real hardware**.
No owner admin, VM, old Lab Manager, native experiment or private-state read.
No runtime MCP/CLI/Plan/backend wiring. CI/check permissions/timeouts unchanged.
All historical PROGRESS/provider rows are retained under dated checkpoints.
