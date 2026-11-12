# Windows service-identity guard regression test: independent review

Reviewer: Anthropic / claude-opus-5-5 / high, as a cross-provider review sub-agent of a Sol (OpenAI / gpt-6.1-sol / medium) change. One agent did the review with no subagents. This was local only: no push, PR, status, watch, approval, merge or dispatch.

## Target

| item | value |
| --- | --- |
| base | `b5e65561b001a3616fa8a6623512e7ff8aa0e346` (#10 main) |
| head | `6d946606bf8724ea4af4538973dcbf17366c3712` (one commit, `6d94660 test(service): protect Windows service identity refusal`) |
| diff | `crates/arxd/src/service/native.rs` (+13, test only), `docs/PROGRESS.md` (+27/−4) |
| read via | pinned Git objects and an owned-temp `git archive` export of the head |
| spec | the brief: a read-only test that compares current process identity with fixed SYSTEM and checks that `require_service_identity()` returns the matching `Ok`/`Err(Permissions)`, with no production change and no startup path |

## Verdict

- **Standards: pass**, with one documentation finding (S2-1). It does not block the source.
- **Spec: pass.** It is honest about scope, with one wording advisory (S2-A1).

## What was verified (source)

- The production code is byte-identical. The only Rust change is a `#[cfg(windows)]` test in `service::native::tests`. It calls `crate::local::current_identity()` (an `OpenProcessToken(TOKEN_QUERY)` read) and `require_service_identity()`, and nothing else. There is no OwnerGuard, session, `Native`/Application constructor, config or layout read, pipe, SCM, LSA, privilege or account call.
- Privacy holds. The identity is reduced to a `bool` before any assert. `assert_eq!` on `Result<(), Error>` prints only `Ok(())`/`Err(Permissions)`. A failed `unwrap()` on `current_identity` prints only the fixed `identity unavailable`. No SID is formatted.
- `Error` derives `PartialEq`, so the comparison is exact and not just `is_err()`.

## Checks run by this reviewer (Windows, ordinary user, owned temp export of `6d94660`)

Only `--exact service::native::tests::service_identity_guard_matches_process_identity_without_startup` was invoked. Each mutation was applied to the helper body and then restored, with `cmp` confirming it.

| mutant of `require_service_identity` | result |
| --- | --- |
| `valid = true` (the builder's mutant) | **killed** (`Ok(())` vs `Err(Permissions)`) |
| `!=` instead of `==` | **killed** |
| wrong constant `S-1-5-19` | survives (expected in an ordinary-user context) |
| `valid = false` | survives (expected in an ordinary-user context) |
| restored head | pass |

Note on the procedure: my first "baseline" invocation reused a stale binary built from another export, because archive mtimes are older than the build artifacts. It reported 0 matching tests. I disregarded it. The restored-head run, rebuilt after the file was touched, is the valid baseline.

`cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings` passed with a fresh target dir. I did not run the full suite.

## Spec — value and coverage, judged honestly

- **Real value.** The existing `production_refuses_normal_user_before_fixed_config_access` is conditional (`if require_service_identity().is_err()`). Under an always-`Ok` helper it silently asserts nothing. The new test is unconditional for an ordinary-user runner, so it closes that hole. It also catches an inverted predicate.
- **What it does not prove.** On every ordinary-user runner (dev and CI) it reduces to "non-SYSTEM is refused". The SYSTEM-accept branch and the exact `S-1-5-18` constant are checked only if the test runs as SYSTEM, which no current runner does (see the surviving mutants). The oracle also shares `current_identity()` with the helper, so a fault in that function that affects both sides would go undetected. As the builder correctly states, it does not prove that every startup caller (`Native::load`, `bind_service`, `state_guard`, `ensure_endpoint_directory`) keeps its call, and it does not exercise a real SYSTEM service.
- **S2-A1 (advisory, wording).** PROGRESS says "This covers the helper policy itself". It would be more precise to say "covers the helper's non-SYSTEM refusal direction". A test that locks the constant would need a pure `is_service_identity(&str)` split, which is a production refactor and out of scope here.

## Standards

- **S2-1 (documentation, minor).** This one-concern test commit's `PROGRESS.md` hunk also brings in unrelated content. It has a full "Local N3 stopping-only dispatch correction" section with N3 run IDs and 223-test claims, and a rewritten Current-state that says "Local E1 and N3 follow-up reviews pass". That is a Divergent Change in the record. I did not verify those statements in this review, and they may duplicate or conflict with the N3 branch's own PROGRESS edits. Before publication, reconcile them so this PR carries only its own section and the current-state line it needs.
- The test name is accurate and the comment states the privacy constraint. No baseline smells in the source.

## Limits

Actual SYSTEM startup, installed, native and dedicated M2 paths are **unverified on real hardware**, and there is no M2 acceptance. I made no production constructor, SCM, LSA, privilege, account, owner, admin, service-manager, VM or Lab Manager call.

**Totals:** Standards has 1 finding (S2-1: unrelated N3/current-state PROGRESS content in this commit). Spec has 0 blocking findings and 1 advisory (S2-A1: "helper policy" wording overstates ordinary-runner coverage).
