# N3 late stopping-only dispatch: independent review

Reviewer: Anthropic / claude-opus-5-5 / high, as a cross-provider review sub-agent of a Sol (OpenAI / gpt-6.1-sol) implementation. One agent did the review with no subagents. It reviews only the N3 delta. It is not a fresh full review of unchanged PR36, and it claims no independent review of the original Opus architecture: that architecture credit is still the earlier Sol review of the Opus ADR. This was local only, during the queue pause. There was no push, PR, status, watch, approval, merge or dispatch.

## Target

| item | value |
| --- | --- |
| base | `4abf276a15883e8e55ff53df26a0641c62b43efd` (its `crates/` equal reviewed public PR36 `5c3c01e`; `5c3c01e..4abf276` only touches `docs/reviews/service-report-validation-probe.*`) |
| implementation | `aea31e755ebdb90ea79c4311a7163bc58ab79e45` `fix(executor): finish stopping refusals without late workers` |
| builder docs | `cb3e062` |
| reviewed head | `4b04cb8aff8a41c0f2c1bd77f1f76c1200685cbf` (coordinator docs only) |
| diff | `crates/arxd/src/service/executor.rs` (+180/−98), `crates/arxd/src/service/executor/tests.rs` (+136), `docs/PROGRESS.md` (+12) |
| spec | AGENTS.md, docs/KICKOFF.md, ADR 0009 §shutdown ("A clean stop leaves no row `started`", line 163) and refusal table (`stopping` → `refused`/`shutdown`, `quarantined` → `refused`/`quarantined`), PROGRESS N3 disposition |

## Verdict

- **Standards: pass, with one advisory smell.**
- **Spec: changes requested. One low-severity semantic regression (N3-R1).** It bypasses no safety or authority control and no backend runs, but it breaks the "no unrelated semantics altered" requirement. A one-line fix plus one test should clear it before publication.

## What was verified

**Linearization (the core claim). It holds.** `drain()` runs `shutdown.cancel()` (Release). It then takes `workers.lock()`, runs `mem::take` and drops the guard in the same statement. Only after that does it await `join_next`. `dispatch()` holds the same `workers` guard across `try_join_next`, the new `shutdown.is_cancelled()` (Acquire) check, and either `spawn_blocking` or the synchronous refusal. The two critical sections are mutually exclusive:

- If dispatch locks first and sees not-cancelled, its worker goes into the set that drain later takes, and drain joins it.
- If dispatch locks first and sees cancelled, it refuses synchronously and registers no worker.
- If dispatch locks after drain's take, the mutex release/acquire orders drain's earlier `cancel()` before the check. Dispatch therefore always sees cancelled, refuses synchronously, and never spawns into the replacement set.

No interleaving registers a worker after drain owns the JoinSet. Plain `shutdown()` without drain takes the same synchronous path and gives the same outcome and reason (`Stopping`) as before, without the worker.

**Synchronous persistence before return.** `complete()` runs `finish_typed` before `dispatch` returns its future. A dropped or unpolled caller future therefore cannot leave the row `started`. The `late_dispatch_after_shutdown_persists_even_when_caller_disconnects` test checks this.

**Deadlock and ordering of the DB write while `workers` is held.** The lock order is `workers` → `Activity.db`. `workers` is locked only in `dispatch` and `drain` (grep). Drain never holds it across an await or DB access. Accepted workers take `db` without `workers`. The MCP callers (`mcp.rs:128`) call `start_dedicated`, which releases `db`, before they call `dispatch`. I found no reverse order, so there is no cycle. The blocking cost is one SQLite transaction on the caller's async thread. That matches the existing synchronous `finish_typed` rejection and refusal paths in `mcp.rs`. On a multi-thread runtime, a concurrent `drain` can block one runtime thread on the std mutex for that transaction. This is acceptable and bounded.

**`complete()` is a faithful extraction.** I compared the old closure body (`let invalid_discovery` … audit flag) with the new `complete()` after removing whitespace. The only differences are rustfmt arm braces and trailing commas, `this.` → `self.`, and `request.as_ref()` → `request`, and they are token-for-token equal otherwise. With `dispatch`/`complete` removed, the rest of `executor.rs` is byte-equal after whitespace normalization. Response shaping, storage-failure withholding (`StorageFailed`, no `execution_reason`), `audit_failed`, `DedicatedAccount` mode, once-only program/argc, `normalize`, invalid-discovery handling and post-spawn quarantine are unchanged. So are the accepted-worker body, `active` accounting, callback cancellation (`CancelOnDrop`) and `try_join_next` quarantine.

## Checks run by this reviewer (Windows, ordinary user, owned temp, fakes only)

- `cargo fmt --all --check`: pass. `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `cargo nextest run --workspace --locked --no-fail-fast`: **223/223 passed, 2 skipped** (subprocess-only fixtures, not invoked directly).
- The three new late-dispatch tests pass at head.
- Mutations on `aea31e7` source, each restored with `git checkout` (final `git status` clean). The filter was `late_dispatch_ | service::executor::tests::` (26 tests).
  - M1: the in-lock guard set to `if false`. **3/3 new tests fail** (registered worker), 23 pass.
  - M2: the synchronous refusal passes `None` instead of the request. **1 fails** (program/argc assertion).
  - M3: the synchronous refusal returns `Err(Shutdown/Stopping)` without `complete()`. **3/3 fail** (row stays `started`).
- `docs/PROGRESS.md` decodes as strict UTF-8 at head.

## Spec

### N3-R1 (low, requested): stopping now outranks quarantine for late requests

The new guard checks only `shutdown`. The refusal path it replaced ran `preflight()`, which checks `quarantined` before `shutdown`. `capability()` uses the same order. PROGRESS also says "quarantine decisions now share one gate". The ADR refusal table defines `quarantined` and `stopping` as separate stored reasons.

**Failure scenario, reproduced with a temporary pure test that was then removed.** A fake executor is quarantined, `shutdown()` is called, and then a dedicated `run_command` is dispatched:

- base `4abf276` (isolated `git archive` export): row reason `quarantined`, `execution_reason` `Quarantined`.
- head: row reason `shutdown`, `execution_reason` `Stopping`, while `capability()` still reports `quarantined`. No backend call occurs in either case.

The audit row loses the quarantine fact, and the response disagrees with the cached capability.

**Fix:** under the guard, use the shared gate, for example `Report::Unavailable(self.preflight().err().unwrap_or(Unavailable::Stopping))`. With `shutdown` already cancelled, `preflight()` returns `Quarantined` or `Stopping` before it reaches `backend.preflight()`. Then add a regression test for quarantined plus stopping.

No other requirement is missing or wrong. The tests cover late dispatch after drain, during a gated accepted-worker drain, and after shutdown with a caller disconnect, plus run and discovery, a SQL trigger completion failure, no backend call and no pending worker, as the brief requires. No concurrent-drain contract, sleep, retry or framework was added.

## Standards

- Advisory smell, **Duplicated Code / Repeated Switches**: the stopping decision now exists in three places. They are the in-lock guard, `preflight()`, and the worker's post-`active` `shutdown.is_cancelled()`. The guard's own copy is what produced N3-R1. Routing the guard through `preflight()` removes the divergence.
- `complete(&self, report, request, activity, id)` is a well-named shared path. No documented standard is broken. The rustfmt-driven reformatting of the closure is mechanical.

## Residual (unchanged, not findings)

- A storage failure in a late refusal after a completed `drain()` sets `audit_failed` after that drain has already returned. The caller still gets `StorageFailed`. Before this change the late worker escaped drain entirely, so this is no regression. ADR shutdown ordering (connections drain before `shutdown_pending`) stays the governing contract. No concurrent-drain contract is implied.
- N1 quarantine policy, the N2 report-validation gaps and N5 stay open and unchanged. Native execution stays `MechanismUnavailable`.
- Every installed, native and dedicated path is **unverified on real hardware**. M2 stays open, with no acceptance claim.
- I made no production constructor, SCM, LSA, privilege, account, owner, admin, service-manager, VM or Lab Manager call, and added no probe. I invoked no ignored fixture directly.

**Totals:** Standards has 0 hard findings and 1 advisory smell (duplicated stopping gate). Spec has 1 finding; the worst is N3-R1, where stopping now outranks quarantine in the late refusal.
