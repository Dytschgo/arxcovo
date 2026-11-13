# Local trusted-policy buffer repair

Prepared by GPT-6.1 Sol, high effort, 2026-11-13. Independent Opus high review is pending. This is a local preparation outside the previous A5 scope; publication/delivery needs the coordinator's concrete owner decision after independent review.

## Identity and scope

- Clean initial checkout: `t3/ci-policy-buffer-repair`, worktree `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-c1fcf364-6d53dab40d44af2d`, distinct from canonical `D:/Code/Arxcovo`.
- Initial HEAD: `2f1e223863a16a9eec307384fe02616a24b87874`. Its only difference from measured head `c5bb1915ceada44935c51d25913257bdf51236bd` is a progress entry; evaluator source is identical. No checkout/base rewrite was needed.
- Read AGENTS.md, docs/KICKOFF.md, actual policy/evaluator source, existing policy/evaluator tests, and trusted policy workflow. No subagents used; docs/PROGRESS.md untouched.
- Source/test commit: `bed6e5dcc9bf95b0de0fd106409b7862507e09b3`.

The coordinator supplied the incident: PR #54 attempt 3, measured head c5bb191, run 43318031363 independently passed A1–A6 (567 s, 88/93 ratio); subsequent trusted-policy run 43326387032 failed with `spawnSync gh ENOBUFS` while retrieving completed PR runs at `per_page=100`, approximately 1.2 MiB. Those remote facts were not re-fetched here. This repair does not reinterpret the measured result or claim remote CI green.

## Change and trust boundary

The only production change is `scripts/ci-budget.mjs`'s `execFileSync` maxBuffer from 1,048,576 to 8,388,608 bytes (8 MiB). It remains finite and applies to the existing API helper; JSON shape, API requests, first-attempt reads, thresholds, candidate selection, repository validation, current-label reread, status publication/cache and workflow trust remain unchanged. No projection, pagination, count or threshold relaxation was introduced.

The regression loads the actual three policy modules in a Node VM with synthetic `node:child_process` and `node:fs` modules. Its fake `gh` validates the command and endpoint, then passes generated JSON through **real `execFileSync(process.execPath, ...)` with the production options**, exercising Node's native output buffer bound. It never launches gh. Event reads and status POSTs terminate in memory; unexpected paths/modules and record writes fail assertions. The VM receives only synthetic repository/event environment and empty argv. No tokens or inherited process environment are exposed to policy code. Child processes run a fixed Node stdin-to-stdout program.

Payloads are generated in memory. The accepted candidate response is 1,311,676 bytes; the oversized candidate is 9,438,140 bytes. No raw API payloads or private state were fetched, saved or committed. Harness output contains only synthetic endpoint names, byte counts, status summaries and error codes. Error buffers are not serialized.

## Red and green evidence

Host Node: `v24.18.0`, Windows. Commands run from the isolated checkout.

After correcting initial harness-only cross-realm array comparison and invalid-repository fixture issues, the old 1 MiB production line produced 4 passes / 3 failures in `node --test scripts/ci-budget.test.mjs`: large-response scenarios failed with aggregate evaluator errors. A focused repeat with the original production bound established:

```text
node --test --test-name-pattern='actual API/evaluator accepts' scripts/ci-budget.test.mjs
# exit 1; 1 test, 0 pass, 1 fail; actual aggregate evaluator error instead of null

node --experimental-vm-modules --disable-warning=ExperimentalWarning scripts/fixtures/ci-budget-buffer.mjs large-candidate
# harness exit 0 (reports caught evaluator error); payload 1,311,676 bytes;
# transportErrors ["ENOBUFS"]; statuses pending, failure; no success
```

Restored the proposed 8 MiB line, then ran:

```text
node --test scripts/*.test.mjs
# exit 0; 28 tests, 28 pass, 0 fail, 0 skipped; 2128.7912 ms final run

node --check scripts/ci-budget.mjs
node --check scripts/ci-budget.test.mjs
node --check scripts/fixtures/ci-budget-buffer.mjs
git diff --check
# all exit 0, no diagnostics

git diff c5bb191 -- scripts/evaluate-budget.mjs scripts/policy.mjs .github/workflows/policy.yml
# exit 0; empty diff
```

The seven new tests verify a >1 MiB candidate response succeeds; >8 MiB main and candidate responses generate real ENOBUFS and finish with failure without success; newest main push is measured via attempt 1; same-SHA/repository latest candidate is measured via attempt 1 (decoy IDs are excluded explicitly); a slow first attempt cannot inherit faster listing timing; removed labels defeat the speed exception; fast main avoids candidate retrieval; malformed repository fails before API access. Existing tests additionally protect head races, fail-closed discovery, status deduplication, no baseline, reduction thresholds and auto-merge guards.

No Rust suite, UI suite, dependency installation or lint was run (local node_modules/eslint absent). The new VM harness uses Node's experimental module API only in its spawned test process; runtime policy requires no new flags. Hardware paths are **unverified on real hardware** and outside this change.

## Delivery boundary

No actual GitHub API calls/status writes, push, PR, CI run/rerun, labels, merge, running-policy injection or direct success posting occurred. No owner/account/service/cgroup/VM/experiment action occurred. No product stack work was started; E1/E5 remain held. Existing red ci-time-budget remains authoritative; no override or merge is authorized by these local checks. Coordinator must arrange fresh different-provider review against the source/test commit, then obtain the owner decision before any publication/delivery.
