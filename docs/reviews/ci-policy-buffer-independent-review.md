# Independent review: trusted-policy response-buffer repair

Reviewer: Claude Opus 5.5 (`claude-opus-5-5`), high effort, security/source review, 2026-11-13. This is a single-agent review with no subagents. It is independent of the builder, GPT-6.1 Sol, which uses a different provider. Read-only on the target.

## Review target (explicit)

| Item | SHA |
| --- | --- |
| Prepared base / this branch's parent | `2f1e223863a16a9eec307384fe02616a24b87874` |
| Repair source/test commit | `bed6e5dcc9bf95b0de0fd106409b7862507e09b3` (`fix(ci): bound policy API responses at eight MiB`) |
| Repair evidence commit | `80d14ea3222a313652359422aca31946e4882ac9` (child of `bed6e5d`) |
| Measured PR #54 head (not modified) | `c5bb1915ceada44935c51d25913257bdf51236bd` |
| Local `origin/main` at review time (last fetch, may be stale) | `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0` |

Identity: the review ran in an isolated, clean T3 worktree on branch `t3/f9c6d748` at `2f1e223`. Its path differs from the canonical checkout. I read the target commits as Git objects (`git show`, `git diff`, `git archive` into a session scratch directory). I did not check out the repair branch, rewrite this branch or cherry-pick anything.

Reviewed diff, `2f1e223..bed6e5d`: `scripts/ci-budget.mjs` changes 1 line (+1/−1), `scripts/ci-budget.test.mjs` is new (+59), and `scripts/fixtures/ci-budget-buffer.mjs` is new (+91). Evidence diff, `bed6e5d..80d14ea`: `docs/reviews/ci-policy-buffer-repair-evidence.md` (+59).

Spec sources: AGENTS.md, docs/KICKOFF.md §1 (CI time budget and merge flow), the docs/PROGRESS.md line 5 blocker statement, and the coordinator's task statement.

## Checks run (reviewer, local, Node v24.18.0 = `.node-version`, Windows)

All runs used a `git archive bed6e5d` export in scratch. No GitHub calls were made. No Rust suite, lint or UI suite was run: eslint and node_modules are absent locally, so CI `npm run lint` is the authority on lint.

- `node --check` on all three changed scripts: exit 0.
- `node --test scripts/*.test.mjs`: 28 tests, 28 pass, 0 fail. The 7 new tests take about 0.04–0.40 s each.
- Fixture byte counts reproduced: large candidate 1,311,676 B, no transport error, `pending → success "Main run 20: 1200s; budget satisfied"`. Oversized candidate 9,438,140 B, `["ENOBUFS"]`, `pending → failure "CI budget evaluation failed; retry required"`. Oversized main 9,437,678 B, `["ENOBUFS"]`, `failure "Main CI measurement unavailable; retry required"`. Removed label: `failure … ci-speed requires a successful measured reduction`, with `error = null`.
- Node boundary probe (synthetic): with `maxBuffer: 8 MiB`, 8,388,608 B passes and 8,388,609 B fails with ENOBUFS.
- Mutants of `scripts/ci-budget.mjs`, run in scratch only and restored byte-for-byte after:

| Mutant | Result |
| --- | --- |
| M1 `maxBuffer` back to 1 MiB | killed (3 fail) |
| M2 `maxBuffer` removed (Node default 1 MiB) | killed (3 fail) |
| M3 `maxBuffer: Infinity` | killed (2 fail) |
| M4 64 MiB | killed (2 fail) |
| M5 2 MiB | **survives** |
| M6 9 MiB (9,437,184 B, below the 9,438,140 B oversized payload) | **survives** |
| M7 candidate `head_sha` filter removed | killed (3) |
| M8 candidate `head_repository.id` filter removed | killed (3) |
| M9 candidate sort ascending | killed (3) |
| M10 candidate uses listing timing instead of attempt 1 | killed (2) |
| M11 main uses listing timing instead of attempt 1 | killed (4) |
| M12 repository regex removed | killed (1) |

- Policy identity: `scripts/evaluate-budget.mjs` (blob `75c21e6d…`) and `scripts/policy.mjs` (blob `3586a49b…`) are identical in `origin/main`, `c5bb191` and `bed6e5d`. `.github/workflows/policy.yml` is identical in all three. `scripts/ci-budget.mjs` differs from `origin/main` (blob `b3b7df41…`) and from `c5bb191` only by the maxBuffer line (blob `61ac6e9c…`, sha256 `6330d197149c524798eadb40940fe13dc5cd47e4e85b1362d5c421e8c15f6305`).

## Security/source assessment

- **Production change.** Exactly one token changes: `maxBuffer: 1024 * 1024` → `8 * 1024 * 1024`. The bound stays finite, and the tests prove it from both sides (M2–M4 killed). Nothing else changes: request paths, `per_page=100`, the head/repository filters, newest-run-number selection, first-attempt timing, thresholds, the current-label re-read, the head-race check, the status cache/dedup and fail-closed paths. M7–M12 confirm the tests protect those semantics.
- **Test seam does not bypass the real options.** `fakeExec` spreads the production `options` object (`encoding: 'utf8'`, `maxBuffer`) into a real `execFileSync(process.execPath, …)`. It overrides only `input` and `stdio`. The native spawnSync byte limit is therefore exercised with the value the production code supplies, and M1–M4 show the tests are sensitive to it. `gh` is never spawned: the fake asserts `command === 'gh'` and an `api repos/example/arxcovo/` prefix, then runs a fixed stdin→stdout Node program.
- **Closed modules and environment.** The VM context holds only `{ process: { env: { GITHUB_REPOSITORY, GITHUB_EVENT_PATH: 'synthetic-event' }, argv: [] } }`. The linker accepts only `node:child_process`, `node:fs`, `./ci-budget.mjs`, `./evaluate-budget.mjs` and `./policy.mjs`. Any other specifier, unexpected API path, non-synthetic event path or `writeFileSync` call fails the run. `--record` cannot run. Caveat: `node:vm` is not a security sandbox, because outer-realm functions such as `fakeExec` expose the outer `Function`. This is acceptable because the loaded code is the repository's own reviewed policy and is not untrusted input. "Closed" here means no accidental access.
- **Secrets and logging.** The harness emits only synthetic endpoint names, byte counts, status state and description, `error.code` and a caught error message. It does not serialize error buffers. The child Node process inherits the test environment but runs a fixed program that does not read env. No token, payload or private state is written or committed. The oversized-main `error` string contains the local Node executable path (`spawnSync <node> ENOBUFS`). That string is only parsed by the test, and the test prints it nowhere on pass. Negligible.
- **Resource impact.** Each `gh api` call can now hold up to 8 MiB of stdout. On ENOBUFS, the error object retains up to about 8 MiB. Calls are sequential, so the plausible worst case is tens of MB on a hosted runner: bounded. No new dependency, workflow, permission or flag is added at policy runtime. `--experimental-vm-modules` is used only in the spawned test child. CI test time grows by about 2 s.
- **Headroom.** About 1.2 MiB was observed for 100 PR runs, which is roughly 12.6 KB per run object. `per_page=100` caps every listing, so 8 MiB gives about 6–7× headroom per page. That is reasonable for a single-concern repair.

## Standards

Sources: AGENTS.md, CONTRIBUTING/eslint config, Fowler smell baseline (judgement only).

1. **Low: the test does not pin the bound at exactly 8 MiB** (AGENTS "every behavior needs a meaningful test"). Mutants M5 (2 MiB) and M6 (9 MiB) survive. The tests prove 1,311,676 B < bound ≤ about 9.0 MiB, not 8 MiB, so a later change to 2 MiB would leave only about 1.7× headroom and still pass. Optional fix: add a padded scenario at 8,388,608 B (accept) and 8,388,609 B (ENOBUFS). Payload size counts serialized JSON bytes, so the padding must be computed. This does not block delivery: the shipped value is reviewed directly and is the only change.
2. **Informational: lint was not run locally.** A static read shows no undeclared globals: `process`, `Buffer` and `URL` are configured for `*.mjs`. CI `npm run lint` remains the authority. Builder and reviewer both label this.
3. **Smell baseline:** nothing material. The fixture's `response()` path cascade is a deliberate synthetic router, not a Repeated Switch in production. The fixture's `run()` helper is local and clear.
4. **Pre-existing, not introduced (follow-up candidate):** when a spawn error escapes uncaught, Node prints it with `util.inspect`, including the `stdout` and `output` strings. Each string is truncated at 10,000 chars, so up to about 20 KB of raw API response can reach the Actions log. A synthetic probe printed 21,837 B with a 1 KiB limit. The data is repository metadata readable by the same token, not a secret. The 8 MiB change does not increase this, because the truncation does not depend on `maxBuffer`, and it makes ENOBUFS rarer. A redacting error wrapper belongs in a separate concern after delivery, not in this PR.

## Spec

1. **Met:** the production change is limited to maxBuffer 1 → 8 MiB, finite (PROGRESS line 5, "bounded response-buffer repair"). Verified by the diff and blob identities.
2. **Met:** semantic and business logic is unchanged. Selection, first attempt, labels, thresholds, head race and fail-closed behavior are verified by diff identity of `evaluate-budget.mjs`, `policy.mjs` and `policy.yml`, and by M7–M12.
3. **Met:** the 7 new tests use a fake `child_process` and `fs`, run the real execFileSync on synthetic 1,311,676 / 9,438,140 B JSON, and exercise the actual api→evaluator path, selection, first attempt, labels and oversized failure. No actual GitHub calls. Reproduced.
4. **Met:** "no relaxed budget/attempts/selection, fake green, retry". There is no pagination, projection, retry loop or status shortcut. Oversized responses fail closed with a `failure` status and never `success`.
5. **Partial, evidence only:** "exact … finite bound 8MiB". The code is exact. The test pins a range, not the value (see Standards 1). Not blocking.
6. **Not in scope / unchanged:** run 43326387032 and run 43318031363 are coordinator-supplied remote facts. Neither builder nor reviewer re-fetched them. This review does not claim remote CI green.

Summary: Standards has 1 low finding (bound not pinned at exactly 8 MiB), 1 informational item and 1 pre-existing follow-up. Spec has 0 blocking findings and 1 partial-evidence note (the same bound precision). **Verdict: source repair approved as correct, minimal, finite and fail-closed. Delivery still requires the owner decision below.**

## Gate bootstrap deadlock (why ordinary delivery cannot work)

- `policy.yml` always checks out `github.event.repository.default_branch`. Policy code changes take effect only once merged to main. Dispatching from a branch still runs main's scripts.
- The latest completed main CI is longer than 900 s. Every PR without a measured `ci-speed` reduction therefore gets `ci-time-budget = failure`.
- PR #54 is the legitimate `ci-speed` candidate (567 s on the first attempt). It cannot be evaluated, because main's 1 MiB policy hits ENOBUFS on the completed-PR-runs listing (about 1.2 MiB). The policy then posts `failure "CI budget evaluation failed; retry required"`.
- The repair PR does not reduce CI time, so it cannot legitimately carry `ci-speed`. Main's policy will mark it `failure` for the over-15-minute baseline. A `ci-speed` label on it would fake a reduction, and its candidate read would hit ENOBUFS anyway.
- Result: neither PR can turn its required check green under a strict gate. Some owner decision is required. There is no agent-authorized path.
- Growing risk (estimate, **unverified**: no GitHub API used): the main-push listing (`branch=main&event=push&status=completed&per_page=100`) is still under 1 MiB, with about 51 first-parent main commits locally. At about 12.6 KB per run object, it reaches 1 MiB at roughly 83 main pushes. Past that point, `getMainRun` fails and every open PR gets `failure "Main CI measurement unavailable"`. The permanent repair should land soon after the bootstrap.

## Delivery proposal (PROPOSAL ONLY; requires explicit owner approval; nothing below was executed)

### Recommended route A: one-time owner-authorized execution of the exact reviewed policy, then a normal merge of #54

This is a full normal policy evaluation with only the transport bound repaired. It is **not** a manually forced status. The status content comes from the unmodified decision code, and the owner must not post, edit or choose any status by hand.

Pre-execution read-only checks, all of which must hold. Abort on any mismatch, with no retry:

1. Fetch fresh main. `scripts/evaluate-budget.mjs` blob is `75c21e6d3342f73ed22c34b40a7262d7a10ae385`, `scripts/policy.mjs` blob is `3586a49bd349f882ca535967ea988a326636476f`, and `git diff origin/main bed6e5d -- scripts/ci-budget.mjs` is exactly the one maxBuffer line. If main's policy changed since this review, re-review is needed.
2. Execution tree: a clean detached worktree at `bed6e5d`, separate from the canonical checkout and from the old Lab Manager installation and its VM. `scripts/ci-budget.mjs` sha256 is `6330d197149c524798eadb40940fe13dc5cd47e4e85b1362d5c421e8c15f6305`, `evaluate-budget.mjs` is `01d09ce75ce53906c52794a43c01b3d3cf4c79d042b0545f6827456ff22033eb` and `policy.mjs` is `03ee2073a6efaac78bc2d25d0276ea25811adf07dea36b1cb922e9b8f87bb3ea`. Node 24.18.0. No local edits.
3. PR #54 is open with head `c5bb191`. Its `ci-speed` label was applied legitimately and is still present. CI run 43318031363, attempt 1, is `success` on `c5bb191`. `agent-review` is `success`, posted by a different provider than the builder.
4. Branch protection or ruleset (read-only): confirm whether the required `ci-time-budget` check is pinned to the GitHub Actions app as its source. **If pinned, route A cannot satisfy the gate, because a user-posted status is ignored. Stop and use route B.**
5. Note that the run evaluates **every** open PR head, as the scheduled policy does. The owner accepts that statuses on other open PRs are refreshed by the same code.

One-time execution:

- `GITHUB_REPOSITORY=<github-owner>/arxcovo`. Set `GITHUB_EVENT_PATH` to a file containing `{}`, which is equivalent to a scheduled event: there is no triggering head, so no targeted discovery-failure post. Run `node scripts/ci-budget.mjs` once, without `--record`.
- Credentials: the current owner `gh` credentials are acceptable if the owner approves. A short-lived fine-grained token scoped to this repo is preferable, with only contents, actions and pull-requests read plus commit-statuses write, mirroring `policy.yml` permissions. Revoke it afterwards. The reviewed code issues only GETs plus `POST statuses/{sha}` with context `ci-time-budget`. Do not echo the token, and do not save raw output. Record only the exit code and status summaries.
- Expected computed result: for `c5bb191`, `success "Main run <latest>: <s>s; budget satisfied"`, because latest main is over 900 s, `ci-speed` is present and the successful first attempt took 567 s, which is less than the latest main duration. Any other outcome is final for this route.

Post-execution checks before any merge:

- Exit code is 0 (no AggregateError).
- The `ci-time-budget` status on `c5bb191` reads back as `success`, with the expected main run id.
- The PR #54 head is still `c5bb191`, and the label is still present.
- CI and `agent-review` are green.

Merge only through the normal flow, with the coordinator or auto-merge and branch protection enforcing all required checks. Do not merge if any gate is not green at that moment.

Race risk: any CI or Policy-signal `workflow_run`, or the 30-minute schedule, runs main's 1 MiB policy. That run hits ENOBUFS on #54 again and overwrites the status with `failure`. Merge promptly after verification. If the status is overwritten first, **stop and return to the owner**. Do not re-run.

Afterwards:

1. Main CI runs on the merge commit. Record its first-attempt duration. If it is ≤ 900 s, that is the fresh baseline. If it is > 900 s, stop: the owner decides, and route B applies to the repair.
2. Create one PR for the single policy-repair concern: rebase or cherry-pick `bed6e5d`, the `80d14ea` evidence and this review onto fresh main. It gets normal CI and a normal `ci-time-budget`: main ≤ 900 s, so no candidate listing is read and main's 1 MiB policy can decide it. The required `agent-review` comes from a different provider than the builder (Sol → Opus). Merge only when all gates are green.
3. Record in docs/PROGRESS.md (coordinator) and as an ADR exception note: the one-time execution, its SHAs, the owner approval and the outcome.

Risks to accept explicitly:

- The status is attributed to the owner account, not github-actions.
- The owner token is broader than the workflow token, which the scoped token mitigates.
- The overwrite race described above.
- Execution outside GitHub's audit context.
- Other open PRs get re-evaluated.
- This sets a precedent, so it should be recorded as a single, named exception.

It is reversible in the sense that a status can be superseded and a merge reverted. The merge of #54 is the consequential step.

Not permitted under this proposal: hand-written or forced `success`, relaxed budget, changed attempts or selection, a retry loop, a `ci-speed` label on the repair PR, a modified workflow file or injected code beyond the reviewed `bed6e5d` tree, and use of the earlier A5 exception as authority. These are new paths outside that scope.

### Alternative owner decision (route B), if route A is unacceptable or step 4 shows source pinning

The owner, as repository admin, merges the one-concern policy-repair PR with a recorded, explicit bypass of `ci-time-budget` only. CI and the different-provider `agent-review` must still be green. Main's policy is then fixed. The next scheduled or triggered run evaluates #54 with the bot token and normal code. #54 merges through the normal flow only if computed green, and that produces the fresh baseline.

Trade-off: statuses stay bot-sourced, there is no local credential use and no race. The cost is that one merged PR, the gate's own repair, lacks a green `ci-time-budget`. This is an owner admin action. Agents must not perform it.

If the owner rejects both routes, the strict gate stays deadlocked. #54 and the repair stay red, and per the growing risk above, the main listing will eventually fail all PRs.

## Boundaries observed

No source or PROGRESS edits, GitHub API calls or writes, push, CI, labels or merge. No runtime policy execution against GitHub. No owner, account, service, cgroup or VM actions, no experiment and no private state. The old Lab Manager installation and its VM were untouched. No product-stack work, and E1/E5 stay held. Milestone 2 stays incomplete. Native paths remain **unverified on real hardware** and are outside this change. The only commit is this report.
