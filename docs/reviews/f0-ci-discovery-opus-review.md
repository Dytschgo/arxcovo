# F0 CI test-discovery correction: narrow independent review

2026-11-14. Anthropic / claude-opus-5-5 / high, review sub-agent (single
agent, no subagents). Builder: OpenAI / gpt-6.1-sol / medium. This is the
different-provider review of one CI correction. The PR70 reviewer flagged that
`test_f0_blockers.py` was not part of required CI. Scope is only the
discovery-pattern change and its PROGRESS lines. Previously accepted F0 source
and tests are not re-reviewed.

**Verdict: Standards PASS. Spec PASS. No blocker.** The PASS allows
publication through the coordinator's normal required gates only. It grants
no run, dispatch, retry, merge, experiment or activation authority, and it
widens no permission.

## Isolation and exact pins

The cwd equalled the Git top level, an isolated T3 worktree on branch
`t3/review-ci-test-discovery`. Status was clean before review and after every
check. HEAD was the target commit. I read AGENTS.md and the dytschgo-review
skill.

| Object | Pin |
| --- | --- |
| Base (main, PR70 squash) | `affaec91b83d3a3c06a67dc796578a98972c0c2b` |
| Target head | `a6f76051e02a7714bce4b31a1952ab5fb5aec558` |
| `.github/workflows/ci.yml` | `c84515f4…` → `fc065af9786e75c52cfb7668b726d0a89931cd5b` |
| `docs/PROGRESS.md` | `44b29112…` → `35940ca05244196fac451188ae7eea6004c97eaa` |
| `test_driver.py` (unchanged) | `0baeeb58f2e789bf9babd32322fd7ee7000e3093` |
| `test_f0_blockers.py` (unchanged) | `6545f5d7245c9bd5dc0eee311a0245968a8d0f0d` |
| `driver.py` (unchanged) | `cf5a0ca64707ccf95036490a06c7dc3b4cb86573` |
| `mutation_check.py` (unchanged) | `fe5b976bd1b946c29012d33879536d172406dc9f` |
| `unix-execution-experiment.yml` (unchanged) | `e5f1a96dbfa522bda602db45bf24db000ebddd5c` |
| `scripts/ci-workflow.test.mjs` (unchanged) | `2b8154b5097c331b4df55c134560857c13d5fb00` |

`git diff --name-only affaec9 a6f7605` lists only `ci.yml` and `PROGRESS.md`.
`git diff --check` is clean. One line changes in `ci.yml`: in the existing
"Pure Unix experiment guards (no provisioning)" step, `-p test_driver.py`
becomes `-p 'test_*.py'`. No other workflow, job, matrix, permission, step,
branch protection, policy, Cargo or product source changed. `ci-time-budget`
and `agent-review` are untouched.

## What the wider pattern adds

The pattern matches exactly two modules in `tools/unix-exec-validation`:
`test_driver.py` (already required) and `test_f0_blockers.py` (new to
required CI). `driver.py`, `mutation_check.py` and `README.md` do not match.
Discovery went from 37 to 47 tests; the 10 new tests are the `F0Blockers`
class.

`test_f0_blockers.py` is unit and mock only:

- Its imports are stdlib, `driver` and `test_driver.valid_env`. It does not
  import `mutation_check`.
- Each `scenario` patches `plan`, `Path`, `getuid`/`geteuid`, `cg_preflight`,
  `protected`, `copy_tree`, `fixture_cleanup`, `shutil.rmtree`,
  `subprocess.check_output`, `subprocess.run`, `os.environ` (cleared),
  `sys.argv`, `sys.stdout` and `sys.stderr`, and forces `sys.platform` to
  `linux`. Every module-global feedback field is patched in `setUp` and
  restored, including the one test that assigns `feedback_phase` directly.
- The only file read is `driver.py` source text, for AST and in-memory
  mutants. Nothing is written.
- The in-memory mutation harness `mutations()` is a plain function. It runs
  only under `__main__` with `--mutants`, so discovery never runs it.

`test_driver.py` is unchanged and already required. Its one real subprocess
call (`test_cli_wrong_platform_refuses`, non-Linux only) runs `driver.py run`,
whose first gate check refuses with `E_PLATFORM` before any other action.

No real driver run, native step, provisioning, account, service, cgroup,
privilege or owner action is reachable from the discovered tests.

## Shell quoting on all three runners

The step sets no `shell:`, so Linux and macOS use bash and Windows uses pwsh.
Bash single quotes stop glob expansion. In pwsh, `'test_*.py'` is a literal
string, and Python on Windows does not glob argv. Either way unittest receives
`test_*.py` unchanged. The manual `unix-execution-experiment.yml` "Safe guard
tests" step already uses the same quoted `-p 'test_*.py'` on the same start
directory. `enterContext` needs Python 3.11 or later, but `test_driver.py`
already uses it in required CI, so the runner Python requirement is unchanged.

## Checks run

All ran locally on Windows, Python 3.12.10, Node 24.18.0:

- `python -I -m unittest discover -s tools/unix-exec-validation -p 'test_*.py' -v`
  in bash: 47 tests OK, exit 0. All 10 `test_f0_blockers.F0Blockers` tests were
  listed and passed.
- The same command in pwsh: 47 tests OK, exit 0. This confirms the Windows
  quoting.
- Old pattern `-p test_driver.py` for comparison: 37 tests OK.
- `node --test scripts/ci-workflow.test.mjs`: 3/3 pass.
- The generated, git-ignored `__pycache__` was removed afterwards. Status was
  clean.

**Not run:** the step on hosted Linux or macOS runners. The new tests force
`sys.platform` and mock every runtime effect, so OS dependence is low. Hosted
CI must still confirm the step. I did not run `mutation_check.py`, the
`--mutants` harness, native, ignored or system tests, or any admin, account,
service, cgroup, VM, private-state, dispatch, install or GitHub operation.

## Standards

No findings. The change uses the established discovery idiom and the same
quoting as the manual workflow. Small-diff, one-concern and conventional-commit
rules are met. No Fowler smell applies to a one-token pattern change.

Advisory (no action required): `docs/unix-exec-validation.md:156` still cites
`-p test_driver.py` with "28 passed". It is a dated historical evidence line,
not a current claim, so it may stay. A later doc touch could note that required
CI now uses `test_*.py`.

## Spec

No findings. The spec is the PR70 reviewer finding that `test_f0_blockers.py`
lacked required CI coverage. The change closes it: the required `verify` job
now runs those tests on all three OS matrix legs. The PROGRESS lines describe
the change accurately. They record PR70's merge pin, say that no privileged
step, policy, protection or product source changed, and keep the fresh Opus
review, hosted checks and F0b design review as pending. Nothing outside that
request was added. The added time is under one second, well within the
15-minute CI budget.

## Status carried forward

M2 remains incomplete. Hardware paths are **unverified on real hardware**.
Internal capture F0b and real-record replay remain OPEN. No diagnosis,
dispatch, historical retry or activation is authorized.

Standards: 0 findings (1 advisory, stale historical doc line). Spec: 0
findings.
