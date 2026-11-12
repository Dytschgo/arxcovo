# N3-R1 shutdown/quarantine priority: independent recheck

Reviewer: Anthropic / claude-opus-5-5, acting as a cross-provider review sub-agent for a Sol (OpenAI) correction. The delegation target requested high effort. This report makes no claim about the runtime effort, which is not observable. One reviewer, no subagents, single-agent review. Scope: only the N3-R1 correction. The prior report `docs/reviews/executor-late-drain-independent-review.md` (commit `cdb6b81`, which reviewed `4b04cb8`) already holds the linearization and `complete()` verdicts, and I did not redo them. No claim is made about Opus's original architecture.

## Target

| item | value |
| --- | --- |
| base | `cdb6b81` (its `crates/` equal reviewed `4b04cb8`) |
| head | `a26570dd22998b41172b6ad2fd0227e910599710` (the prepared checkout's HEAD) |
| commits | `b3a457d` fix(executor): preserve quarantine on stopping refusals; `a26570d` docs: record shutdown priority review correction |
| diff | `executor.rs` (+2/−1), `executor/tests.rs` (+48), `docs/PROGRESS.md` (+4). `git diff 4b04cb8 a26570d -- crates/` touches only those two source files. |
| spec | prior N3-R1 finding and its suggested fix, ADR 0009 refusal table (`stopping` → `shutdown`, `quarantined` → `quarantined`), AGENTS.md |

## Verdict

- **Spec: pass. N3-R1 is resolved.** No new blocking gap.
- **Standards: pass.** One low finding about the docs record (EX-S1) and one informational test-coverage note (EX-I1).

## Spec

- The synchronous branch now uses `self.preflight().err().unwrap_or(Unavailable::Stopping)`. `preflight()` keeps its unchanged order: quarantined, then stopping, then `backend.preflight()`. That matches `capability()` and the accepted-worker path. The old priority is preserved, and only the late synchronous refusal changes.
- No backend callback when stopping. The branch is entered only when `shutdown.is_cancelled()`. `Cancellation` is a one-way `AtomicBool`: `cancel` only stores `true`, with Release/Acquire ordering. So `preflight()` always returns `Quarantined` or `Stopping` before it reaches `self.backend.preflight()`. The `unwrap_or` fallback cannot be reached and is only defensive. This rests on inspection; see EX-I1.
- A quarantine detected by the same call's `try_join_next` (a panicked worker) is set before the check, so it also wins. That matches the pre-N3 behavior.
- The late-worker fix is unchanged. The `workers` guard, the in-lock check, the synchronous `complete()` before return, and the accepted-worker body (including its own post-`active` shutdown check) do not differ from `4b04cb8`. Runtime outside the executor and its tests is unchanged. No new quarantine policy, quarantine creation, native path or contract was added.
- The test `stopping_refusal_preserves_quarantine_precedence` covers both `run_command` and `discover_tools`. It asserts capability `quarantined`, `Failure { Quarantined, Some(Quarantined) }`, the durable row `refused`/`quarantined`/`dedicated_account`, 0 registered workers and 0 `run` calls. Plain stopping still gives `Stopping`/`shutdown`, which the existing late tests assert (`tests.rs:742,748,824`).

## Standards

- **EX-S1 (low, docs).** AGENTS.md requires per-task provider, effort and outcome. The new PROGRESS bullet says "Coordinator corrected N3-R1" but gives no provider or effort. It sits under the heading "Local N3 stopping-only dispatch correction - Sol high", so a reader could take the correction to be Sol high work. Add the actual provider and effort, for example `OpenAI / gpt-6.1-sol / <effort>`.
- **EX-I1 (info, test coverage).** The fake's `calls` counts only `Fake::run`, and `Fake::preflight` is uncounted and returns `Ok`. So "backend calls 0" does not show that no backend preflight happened. Probe b2 adds a `self.backend.preflight()` call to the stopping branch, and the suite still passes 27/27. The current code is correct by inspection. If no backend preflight while stopping is meant as a binding rule (the native preflight resolves config and accounts), count fake preflight calls and assert 0. This is optional and does not block.
- The prior advisory smell (the duplicated stopping gate) is reduced. The guard now routes through `preflight()`, so its copy can no longer diverge. The worker's post-`active` check stays as it was and is out of scope. The PROGRESS dispositions of the three prior reports and the retained pipe-review boundary breach are accurate. I did not repeat the breach.

## Checks run by this reviewer

Ordinary user, a `git archive` export of `a26570d` in a session scratch directory, fakes and owned temp only.

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass. The first attempt was a cache hit from the sibling export sharing a target dir, so I touched the sources and re-ran. arxd was then checked from the n3 path, and it passed.
- The four late and stopping tests: 4/4 (`f94f8081-d9fd-4fcd-9ade-97524dc9d292`). `test(service::executor::tests::)`: 27/27 (`de6849bd-2588-4ead-810a-42291796fd16`, rebuild confirmed).
- Mutations: each rebuild was confirmed by a `Compiling arxd (…n3…)` line, and each file was restored from `git show` and byte-compared against `a26570d`.

| id | mutation | result |
| --- | --- | --- |
| b1 | revert to `let refusal = Unavailable::Stopping;` | new test fails: `left: Shutdown`, `right: Quarantined` (`9b83fc81`) |
| b2 | add a `self.backend.preflight()` call before `self.preflight()` | **not caught**, 27/27 (`b3c691bb`) → EX-I1 |

I did not run a full workspace suite. The earlier 223/223 belongs to the earlier builder and reviewer heads and is not re-claimed for `a26570d`.

## Limits

- No pipe open or connect. No production constructor, SCM (including read-only), LSA, privilege, account, owner, admin, service-manager, VM or Lab Manager call. No ignored fixture was invoked directly.
- No push, PR, GitHub write, status, approval, merge, watch or dispatch. Local only, so there is no PR status. Other branches and the shared checkout were not touched.
- Native, installed and dedicated execution is **unverified on real hardware**. There is no M2 acceptance claim.

**Totals:** Spec has 0 findings, and N3-R1 is resolved. Standards has 1 low finding, EX-S1, the missing provider and effort for the correction, plus 1 info note, EX-I1, that fake preflight calls are not counted.
