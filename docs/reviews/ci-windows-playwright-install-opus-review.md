# Opus review: C1 Windows Playwright install probe

Reviewer: Anthropic / Claude Opus 5.5 / high, independent review sub-agent, 2026-11-16. Builder: Sol.
Target: branch `work/ci-windows-playwright-install`, head `43b1dc31696753208b77d4c3d9c02edd0972b411`, base main `58d06c1647f6c8238b188226a9434c861b968641`. One commit (`43b1dc3`), four paths: `.github/workflows/ci.yml`, `scripts/ci-workflow.test.mjs`, `docs/evidence/ci-windows-playwright-install-probe.md`, `docs/PROGRESS.md`.
Spec: C1 in Opus brief `2e3a388` (retained as `181604c`, same file and content), plus the C1 paragraph of Sol's contract review `9f3ee35`.
Standards: AGENTS.md, CONTRIBUTING.md, KICKOFF.md CI rules, and the Fowler smell baseline.
Method: single-agent review covering both axes, with separate findings for each. The change is low-risk CI configuration.

**Verdict: PASS for source publication as a measured probe.** C1 acceptance is still **pending**: no hosted run exists for this head, so no savings are claimed or measured here.

## Checks actually run by this reviewer

| Check | Result |
| --- | --- |
| `git diff 58d06c1...43b1dc3` / `git log` | 1 commit, 4 files, +23/−2. No source, dependency, lockfile, browser-channel, timeout, cache, quarantine or sampler change. |
| `git diff --check 58d06c1...43b1dc3` | pass |
| `node --test scripts/ci-workflow.test.mjs` (Node 24.18.0, Windows host, no install) | 3/3 pass |
| Workflow mutants run against copies of the test in scratch (10) | 6 killed: Windows `--with-deps` restored, conditions overlapping (`!= 'Linux'`), Windows step never running, full browser instead of `--only-shell`, a third install step, a `; exit 0` mask appended to the Windows command. 3 survived: smoke `if: false`, smoke `continue-on-error: true`, desktop command commented out (see S1). 1 unmutated control passed. |
| Primary source: `npm pack playwright-core@1.65.0` (official registry, unpacked to scratch, not executed) | `--with-deps` only adds `registry.installDeps(executables)` **before** the same `registry.resolveBrowsers(args, {shell})` / `registry.install(...)` path. On win32, `installDependenciesWindows` runs `powershell.exe -ExecutionPolicy Bypass -File bin/install_media_pack.ps1` only when targets include chromium. That script runs `Install-WindowsFeature Server-Media-Foundation` only when `ProductType -eq 3`. macOS has no installDeps branch. The installed headless-shell executable is identical with or without the flag. |
| `git ls-remote origin` | main = `58d06c1`. Branch not on origin; no PR (`gh pr list` empty); no runs for `43b1dc3`. |
| `gh run view 43815774741` | push / main / `58d06c1` / completed / success |
| `gh api …/jobs/133123407634` (reference) | Windows 06:06:27–06:16:25 = **598 s**; install 06:13:36–06:16:07 = **151 s**; UI smoke and Electron smoke success. Matches the brief. |
| `gh api …/runs/43815774741/jobs` (base main) | Windows 06:17:47–06:26:49 = **542 s**, install 06:24:17–06:26:36 = **139 s**; macOS install 7 s; Ubuntu install 34 s. |
| `git grep` for step-name consumers | No script or policy workflow reads step names, so renaming the step does not affect budget tooling. |

Reused, not re-run: Sol's own local `node --test` and `diff --check` reports (both re-run above independently), and Opus's brief mechanism (re-derived above from the packed source).

## Questions asked of this review

- **Are the Windows UI and Electron smokes still valid gates, with no bypass?** Yes. `npm run smoke` and both Electron steps are byte-identical, with no new `if`, `continue-on-error` or step timeout. The job still has `timeout-minutes: 12` and `fail-fast: false`. Dropping the flag removes only the Media Foundation feature install. The Chromium headless-shell binary is the same, and `playwright.config.ts` has no channel and no `headless: false`. Electron launches the `electron` package binary, not a Playwright registry browser. Previously the Windows Electron smoke ran after that feature install in the same job, so it could have depended on it implicitly. If Chromium or Electron needs Media Foundation on `windows-latest`, the smoke goes **red**: this fails closed and cannot pass falsely. Limit: neither smoke exercises media, so a green run does not prove media capability. Any future media test needs a reviewed install change.
- **Are the platform conditions mutually exclusive?** Yes. `runner.os != 'Windows'` and `runner.os == 'Windows'` are exact complements on one context value, so exactly one install step runs per matrix leg. The overlap and never-runs mutants are killed.
- **Could PowerShell mask a failure across multiple commands?** No. The Windows `run:` is a single native command under the default `pwsh` shell, and the runner propagates `$LASTEXITCODE` for the last command. The test anchors the line end, so an appended command such as `; exit 0` is killed. The B1 mask class does not apply to this step.
- **Is this cross-OS?** The regexes accept `\r?`. The test reads only files and passed on a Windows host. Ubuntu and macOS run the same test through `npm test`.

## Standards

- **S1, low (test strength; not blocking).** `scripts/ci-workflow.test.mjs:12-14` checks only that `npm run smoke` and `npm run test:desktop` appear somewhere in the workflow text, yet its message says they "remain required". The mutants for `if: false`, `continue-on-error: true` and a commented-out command all survive. This is still a net improvement, because the base had no smoke pin. Suggested fix: anchor the exact step blocks, e.g. `/^ {6}- run: npm run smoke\r?$/m` and the two Electron `name`/`if`/`run` triples, and assert `continue-on-error` is absent. Under AGENTS ("every behavior needs a meaningful test"), the install-branch assertions are meaningful; the smoke assertion overstates what it checks.
- **S2, low (evidence completeness).** The evidence file says local verification "is appended at its actual checkpoint", but the committed file has no local results yet. The PROGRESS entry names Sol but gives no effort or outcome (AGENTS: per-task provider/effort/outcome). Append the measured `node --test` 3/3 and `diff --check` results, plus effort and outcome, before merge.
- Smell baseline: Duplicated Code is limited to the two nearly identical install steps, which is the minimal shape and fine. Mysterious Name: none; the step names describe what each step does. No other smells.
- Security: `permissions: contents: read` is unchanged; no new actions, secrets, network endpoints or caches. On Windows, an elevated `powershell -ExecutionPolicy Bypass` feature install no longer runs, which slightly reduces privileged surface. No private data in the four files.

## Spec

- Matches C1 as stated in brief `2e3a388`. The existing line is kept under `if: runner.os != 'Windows'`; a Windows-only `npx playwright install --only-shell chromium` is added; both lines are pinned; caching and other steps are out of scope. It also matches Sol's `9f3ee35` C1 paragraph: smokes and the 12-minute cap are unchanged, and it does not assume the 151 s is all feature install. Linux and macOS behavior is unchanged; only the displayed step name changed (the command is identical).
- Nothing outside scope: no sampler, B1–B4, quarantine, timeout, `ci-speed` label or savings claim. The eight Windows three-node fixtures remain quarantined. M2 open, M3 unchecked, **unverified on real hardware**.
- **P1, medium (measurement method; advisory to the coordinator, not a code defect).** The acceptance compares against one reference job (598 s). Two adjacent reference runs already differ by 56 s at the job level (PR153 Windows 598 s vs base main 542 s) and by 12 s at the install step (151 s vs 139 s). That spread is close to the 60 s abandon threshold. Report the PR and next-main Windows **install-step** duration against both 151 s and 139 s as the primary causal signal. Report job duration against both 598 s and 542 s as secondary. If the job-level result lands in 60–100 s only because of the reference chosen, record that explicitly for the coordinator's decision. Plausibility is unproven: ≥100 s of job savings needs the headless-shell download plus extraction to take ≤ ~40–50 s of the 139–151 s step on Windows, which no run has measured yet.
- Acceptance items still open (need hosted runs, not available to this review): Windows UI and Electron smoke green on the PR run, a visible step-log split, and the PR plus next-main Windows timings measured against the thresholds. No rerun may be used to obtain savings.

**Summary.** Standards: 2 findings, worst is S1 (the smoke-requiredness pin is substring-only; low). Spec: 1 finding, P1 (reference-run noise is close to the stop threshold; medium, advisory). Hosted timing is **unmeasured**: head `43b1dc3` has not been pushed and has no CI run. Required gates for merge (CI, `ci-time-budget`, `agent-review`) are not yet evaluable for this head.
