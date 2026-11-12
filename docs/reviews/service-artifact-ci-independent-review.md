# Read-only service artifact CI step: independent review

Reviewer: Anthropic / claude-opus-5-5 / high, as a cross-provider review sub-agent of a Sol (OpenAI / gpt-6.1-sol / medium) change. One agent did the review with no subagents. This was local only: no push, PR, status, watch, approval, merge or dispatch, and no hosted run.

## Target

| item | value |
| --- | --- |
| branch | `ci/service-artifact-checks` (read through pinned objects) |
| base | `b5e65561b001a3616fa8a6623512e7ff8aa0e346` |
| head | `e0dc0fabf2b4e3aa812ccc4a956459c024e6e901` (`ci: run read-only service artifact checks`) |
| diff | `.github/workflows/ci.yml` (+2), `docs/PROGRESS.md` |
| spec | ADR 0009 native artifact-check advisory (partial closure); AGENTS.md/KICKOFF CI under 15 minutes, gates not bypassed, no privileged install or service operation |

## Verdict

- **Standards: pass.**
- **Spec: pass.** The PR is correctly described as a partial, advisory closure.

## What was verified

- **Exactly two workflow lines**, a named step `python -I scripts/test-service-artifacts.py`, are added after `cargo nextest run` in the unconditional `verify` job, so they run on all three matrix OSes. The matrix, `fail-fast: false`, the 12-minute timeout, `permissions: contents: read`, pinned action SHAs and every other step are unchanged.
- **Interpreter availability:** `python -I` already runs unconditionally on all three platforms at line 32, so the new step adds no runner prerequisite.
- **Script safety** (`scripts/test-service-artifacts.py`, unchanged by this diff and byte-identical at the N3 head together with `packaging/`):
  - It imports only the stdlib, and `-I` isolation blocks planted-module imports from the working directory.
  - The portable tests read `packaging/linux/arxd.service`, the standard-rights drop-in and the plist from the checkout. They parse them with `configparser` (no interpolation, strict) and `plistlib`. Nothing is written.
  - The native tests use argv lists (no shell) and a 20-second timeout. `plutil -lint` runs only on darwin. `systemd-analyze verify` runs only on Linux, and only when `/usr/local/libexec/arxcovo/arxd` already exists. That file never exists on a stock runner, so the test skips. The step never runs `systemctl`, `launchctl`, `sc` or an installer, and needs no sudo.
- **Cost:** 5 tests in milliseconds. The impact on the 15-minute budget is negligible.

## Checks run by this reviewer (Windows, ordinary user)

- `python -I scripts/test-service-artifacts.py -v` at the N3 head, with the script and `packaging/` identical to `e0dc0fa`: **5 run, 3 passed, 2 skipped** (`plutil` and `systemd-analyze` are unavailable on Windows).
- Failure propagation: in an owned-temp `git archive` export, I changed the drop-in to `ProtectHome=no`. The script **exited 1**, so a CI step would fail. The export was then deleted.
- I did not check hosted execution on Linux or macOS, so I do not claim that `plutil -lint` passes on a hosted macOS runner. Per the brief, I wrote no tests that only mirror the workflow.

## Spec

There are no missing, extra or incorrect requirements. PROGRESS correctly limits the claim: macOS can run `plutil -lint`, Linux `systemd-analyze verify` stays skipped without the fixed binary, and there is no claim of Linux native artifact validation and no binary provisioning on the owner PC. The ADR 0009 advisory stays only partially closed.

## Standards

No documented standard is broken and no baseline smells apply. The PROGRESS "Current state" hunk is identical to slice 2's and to the N3 line's, so it merges cleanly and is not a finding.

## Limits

Installed, native and dedicated M2 paths are **unverified on real hardware**, and M2 stays open. I ran no service-manager, privileged, install, VM or Lab Manager operation.

**Totals:** Standards has 0 findings and Spec has 0 findings.
