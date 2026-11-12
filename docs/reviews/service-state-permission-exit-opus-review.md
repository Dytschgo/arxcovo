# State permission exit classification: independent Opus review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent, no
subagents, 2026-11-11. Cross-provider review of a Sol (gpt-6.1-sol / medium)
change. Local only: no push, PR, GitHub write, merge or dispatch.

Verdict: **PASS. No blockers; two advisories.**

## Target

- Range `7fefe4d..7781239b699ab026553ab2957286670d0025785d`, one commit
  (`7781239 fix(service): preserve state permission exit classification`). It
  is separate from the ADR 0017 target: merge-base with `07ae3c8` is `fa6f297`.
- Files: `application.rs` (+7/−1), `application/tests.rs` (+33),
  `docs/PROGRESS.md`, `docs/reviews/service-state-permission-exit-correction.md`.
- Reviewed and tested from a `git archive` export of `7781239` in the
  reviewer's own scratch directory. The shared root was not changed and no
  extra checkout was created.

## Propagation check (source)

1. `Native::store` → `Activity::open_service(layout)`, which returns
   `anyhow::Result`.
2. `open_service` calls `crate::service::native::state_guard(layout)?`, which
   returns `Result<Vec<Observed>, layout::Error>`. `layout::Error` implements
   `std::error::Error`, so `?` wraps it into anyhow with the type preserved.
3. `state_guard`'s typed errors: `require_service_identity` gives `Permissions`.
   `checked` → `os::open` gives `Layout` or `Permissions`.
   `check_protection` gives `Permissions`. The header read gives `Permissions`.
   WAL or SHM present gives `Permissions`.
4. `store_category` uses `downcast_ref::<layout::Error>()`. anyhow's
   `downcast_ref` also reaches the wrapped error under `.context(..)`.
   `Permissions` and `OwnerRequired` map to 4. Everything else, including
   untyped `open_inner` failures, maps to 5. This matches ADR 0009: "`4`
   config/state ownership or permissions, `5` store unavailable".

## Commands run (actual, on the export)

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed (37.2 s) |
| `cargo nextest run --workspace --locked --no-fail-fast --status-level fail --final-status-level fail` (single run) | **254 passed, 2 skipped**, run `876aba30-3a41-47b3-850b-c6857d40c186`. Matches the builder's 254/2. |

Source mutations: filter `test(service::application::tests::)`, 11 tests.
Each was an exact replacement with byte-exact restore and a hash check. All
compiled.

| # | Mutation | Run ID | Result |
| --- | --- | --- | --- |
| MB1 | Drop `OwnerRequired` from code 4 | `91868cf3-69d0-4c82-bb56-e0dfc481b6e8` | killed |
| MB2 | Downcast only the outermost chain element (breaks the `.context` case) | `ae10c2d3-4735-4330-95f8-45d50d51d311` | killed |
| MB3 | Also map raw `io::ErrorKind::PermissionDenied` to 4 | `cc3b83df-d5c8-46b3-8fda-2a000f67650d` | killed |
| MB4 | Map `Layout` to 4 | `966592fa-877f-4a90-ba76-424a53288598` | killed |
| MB5 | Revert the `Native::store` call site to a blanket `Store` | `9a1aad45-0d2f-43f8-9c3c-323a48389abf` | **survived** (expected; see A1) |

These add to the builder's two recorded mutations (collapse to Store, and all
typed errors to Permissions).

## Standards

No findings. The commit is focused and conventional. The classifier is private,
with no string matching. The test is pure, with no production guard. The
evidence states that the call site was checked by source inspection only.
PROGRESS records provider, effort and outcome and keeps the **unverified on real
hardware** label. No smell from the baseline applies to a 7-line classifier.

## Spec

The behaviour asked for is implemented. Typed `Permissions` and `OwnerRequired`,
including under an anyhow context, map to exit 4. All eleven other `layout::Error`
variants, a generic anyhow error and a raw I/O `PermissionDenied` map to 5. The
test enumerates every current variant.

Advisories:

- **A1.** The `Native::store` call site has no test (MB5 survives). A test
  through `Native::store` would run the production state guard, and on a root
  CI runner it could go further than the identity refusal. So leaving it out is
  correct under the fences. If wanted, add a pure seam, for example
  `fn store_exit(result: anyhow::Result<Activity>) -> Result<Activity, ExitCode>`
  used by `Native::store` and tested with synthetic errors. That makes a
  call-site regression observable without touching system state.
- **A2.** A typed `Layout` from the state guard (for example, `os::open` on a
  missing or wrong-type state object) still exits 5. ADR 0009 assigns 7 to
  "layout or binary integrity". The requested scope deliberately keeps every
  non-permission error at 5, and the original code also produced 5, so nothing
  regressed. Record an owner or architecture decision if 7 is preferred.
- Integration note: the ADR 0017 target also appends to the end of
  `application/tests.rs` and edits PROGRESS. Expect a trivial textual conflict;
  keep both tests.

## Limits

No production startup, native store or state guard, layout, session, install,
process, pipe, system path, SCM/LSA/privilege/account/service/owner, VM, Lab
Manager or S4U operation ran. No ignored fixture was invoked directly. Hosted
gates are still required. Installed, native and dedicated M2 is **unverified on
real hardware** and incomplete.

Standards: 0 findings. Spec: 0 blockers, 2 advisories; worst is A1, the
untested call site.
