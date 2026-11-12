# Service artifact CI port onto main after #41: integration review

Reviewer: Anthropic / claude-opus-5-5 / high, as a cross-provider review sub-agent of a Sol (OpenAI / gpt-6.1-sol / medium) coordinator port. One agent did the review with no subagents. This is a narrow equivalence and integration review. It was local only: no GitHub reads or writes, push, PR, status, watch, approval, merge, rerun or settings change.

## Target

| item | value |
| --- | --- |
| head | `57b0f713d82d710838d57a173dce2e6bd6fd5bfa` (`ci: run reviewed service artifact checks on every platform`) |
| base | `a10751af5a6b4c4c0f642daff034e6544980e7bf` (PR #41 merge, parent of head) |
| original reviewed change | head `e0dc0fabf2b4e3aa812ccc4a956459c024e6e901`, base `b5e65561b001a3616fa8a6623512e7ff8aa0e346` |
| retained report | `docs/reviews/service-artifact-ci-independent-review.md`, from `cdb6b813d56a867cfd0c5bbc2cba018bb53c64fe` |
| diff | `.github/workflows/ci.yml` (+2), `docs/PROGRESS.md` (+9, 0 deletions), `docs/evidence/service-artifact-ci-port.md` (new), retained report (new on this line) |
| spec | the original reviewed change; ADR 0009 native artifact-check advisory (partial closure); AGENTS.md/KICKOFF CI under 15 minutes, gates not bypassed, no install or service operation |

## Verdict

- **Standards: pass.** 0 findings.
- **Spec: pass.** 0 findings. The port is equivalent to the reviewed change, and the docs describe it accurately.

## Equivalence verified

- **Workflow delta.** `git diff a10751a 57b0f71 -- .github/workflows/ci.yml` adds exactly two lines and removes none: `- name: Read-only service artifact checks` and `run: python -I scripts/test-service-artifacts.py`. They are the same two lines `git diff b5e6556 e0dc0fa` adds, at the same indentation. Only the hunk offset differs (41 → 49), because main changed between the two bases. The step has no `if:`, so it runs in every matrix job. It sits directly after `cargo nextest run --workspace --locked` and before the Windows probe guard step, as it did in the original.
- **Everything else is unchanged.** The diff touches no other workflow line. In the head file, the triggers, `concurrency` group and PR-only cancellation, `permissions: contents: read`, `fail-fast: false`, the `[ubuntu-latest, windows-latest, macos-latest]` matrix, the `verify (${{ matrix.os }})` job name, the `timeout-minutes: 12`, the four pinned action SHAs and every other step are the same as at the base. Because the workflow and job names did not change, the required check names did not change either.
- **The checker and packaging files are byte-identical.** The `scripts/test-service-artifacts.py` blob is `54b0baf3bfc9a9262a1d9deb7a0b46a2bfcd5437` at `e0dc0fa`, at the base and at the head. The `packaging/` tree is `5c0c9d164b3501fefc63afbc06086443cd0ecd8a` at all three. `git diff a10751a 57b0f71 -- scripts packaging` is empty.
- **The retained report is byte-identical.** Blob `1095a00ab3784890809206dac99809b258c7f8fa` is the same at `cdb6b81` and at the head.
- **Interpreter.** Line 35 already runs `python -I` on all three platforms, so the step adds no runner prerequisite.

## Script read (unchanged source)

- It imports only the stdlib. `-I` blocks planted-module imports from the working directory.
- The three portable tests parse the checked-in unit, the standard-rights drop-in and the plist with strict `configparser` (no interpolation) and `plistlib`. Nothing is written.
- The two native tests use argv lists (no shell), `capture_output` and `timeout=20`:
  - `plutil -lint` runs only on darwin when `plutil` exists.
  - `systemd-analyze verify` runs only on Linux, and only when `systemd-analyze` exists and `/usr/local/libexec/arxcovo/arxd` is already a file. Nothing in the workflow creates that file, so on hosted Ubuntu the test skips.
- There is no `systemctl`, `launchctl`, `sc`, installer, `sudo`, package install or service-manager call.
- `unittest.main()` exits nonzero when a test fails, so a portable failure fails the step. That conclusion comes from reading the code. The actual failure-propagation run (`ProtectHome=no` → exit 1) belongs to the retained report at `e0dc0fa`. I did not repeat it, and I credit it only to that report.

## Checks run by this reviewer (Windows, ordinary user, at the head)

- `python -I scripts/test-service-artifacts.py -v`: exit 0, **5 run, 3 passed, 2 skipped** (`macOS plutil unavailable`; `Linux systemd-analyze or fixed binary unavailable`), 0.003s.
- `node --test scripts/ci-workflow.test.mjs`: exit 0, 1/1 passed (concurrency and the single 12-minute timeout).
- `git diff --check a10751a 57b0f71`: clean.
- The four changed files decode as UTF-8, with LF line endings, no BOM and a final newline. Every relative file link in the evidence doc, the retained report and the new PROGRESS lines resolves. The only non-resolving link target in PROGRESS is a pre-existing `t3-thread://` URI outside this diff.
- Git object comparisons as listed above.

I ran no Cargo, native validator, service, SCM, LSA, account, privilege, owner, pipe, VM, Lab Manager, private-state, experiment or ignored-fixture operation. No Rust source changed, so no Rust suite repeat was needed.

## Spec

There are no missing, extra or incorrect requirements.

- **The port matches the reviewed change.** It is the same unconditional two-line step after nextest, with the checker and packaging files byte-equal.
- **The ADR 0009 advisory stays partially closed.** The evidence doc claims no native validator pass on hosted macOS or Linux. One point to keep in mind for the hosted run: on `macos-latest`, `test_native_plutil` will execute rather than skip, so the macOS job is the first real exercise of that path. A `plutil` failure there would fail CI correctly. It is not a defect in this port. At the coordinator's last snapshot, PR #42's Windows/macOS jobs were still pending, so hosted macOS behaviour is not yet evidenced on any line.
- **CI budget.** The step adds milliseconds to a 12-minute job timeout. The last measured main run (after #40) took 428s, so the change cannot move CI near the 15-minute `ci-time-budget` limit. The step bypasses no `ci-time-budget` or `agent-review` gate.
- **The evidence doc is accurate.** It states the exact delta, byte equality, the unchanged matrix, timeout, permissions, pins, concurrency, steps and gates, and the read-only native prerequisites. It credits the earlier failure-propagation run to the retained report rather than claiming it as new. Its coordinator check results match the ones I reproduced. It has before/after and reversibility statements: this is a two-way door, and reverting removes one CI step without changing product behaviour. It also carries the **unverified on real hardware** label.
- **PROGRESS.**
  - The hunk is a pure insertion: 9 added lines, 0 deleted. The new "Current state — 2026-11-12" section comes first. The prior current-state bullets are kept verbatim under "Historical checkpoint before artifact CI port", so main's history is preserved.
  - The current section has a single queue-priority line: PR #42 first, then this artifact-CI candidate as the next one-concern PR, with E2 held on run bdbf032b.
  - Main after #41 (run 43190243712) is recorded as pending and not claimed green.
  - Main after #40 is recorded at 428s, which the preserved historical bullet corroborates (run 43187047371).
  - M2 is recorded as incomplete, with native, installed and dedicated paths **unverified on real hardware**.
  - Statements about PR #42's body, review state and pending CI are coordinator claims. I did not check them, because this review made no GitHub reads.

## Standards

No documented standard (AGENTS.md, CONTRIBUTING.md) is broken, and no baseline smell applies to a two-line step and documentation. The commit is one conventional commit with one concern. It claims no check that was not run, and it labels hardware paths correctly.

One out-of-scope advisory, which is not a finding and not counted: ADR 0009 also lists `{{` as a forbidden interpolation marker, but the unchanged checker forbids only `$`, `%` and shell names. This predates both reviewed revisions and belongs to a separate checker change, not to this byte-equal port.

## Independence and limits

- This is a different-provider review of a Sol coordinator port. I make no independence claim over Opus-authored architecture, including the ADR 0009 advisory itself.
- This review does not grant merge permission. The candidate has no PR yet. Publication waits for the queue coordinator's disposition, and merge requires all hosted gates: CI on three OSes, `ci-time-budget` and `agent-review`.
- Hosted Linux and macOS execution of the step was not observed.
- Installed, native and dedicated M2 paths remain **unverified on real hardware**, and M2 is incomplete.

**Totals:** Standards has 0 findings and Spec has 0 findings. Neither axis has a worst issue.
