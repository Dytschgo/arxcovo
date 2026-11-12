# Service artifact checks in ordinary CI

Coordinator: OpenAI / gpt-6.1-sol / medium, 2026-11-12.
Base: `a10751af5a6b4c4c0f642daff034e6544980e7bf` (PR41).
Original reviewed change: `e0dc0fabf2b4e3aa812ccc4a956459c024e6e901`.

The workflow patch is the same two-line unconditional step after normal
nextest: `python -I scripts/test-service-artifacts.py`. The checker and all
packaging files are byte-identical to the original reviewed source. The
matrix, 12-minute timeout, permissions, pinned actions, concurrency, existing
steps and required gates stay unchanged. No product source changes.

The [retained independent Opus review](../reviews/service-artifact-ci-independent-review.md)
verified safety and failure propagation at its original revision; those
checks are not claimed as new runs. The portable tests parse repository
artifacts only. The native validators are bounded read-only subprocesses:
macOS plutil, and Linux systemd-analyze only if its fixed binary prerequisite
already exists. This change installs no binary and calls no service manager.

Actual coordinator checks on this candidate, all exit 0:

- `python -I scripts/test-service-artifacts.py -v`: 5 run, 3 passed and
  2 conditional native skips on Windows, 0.002s.
- `node --test scripts/ci-workflow.test.mjs`: 1/1 passed.
- Git object comparisons of the checker/packaging and `git diff --check`.
- Changed-doc UTF-8/LF and relative file-link checks.

No Rust source changed, so no Rust suite repeat. No new mutation, native
validator, service/SCM/LSA/account/privilege/owner, VM/Lab Manager, private
state or experiment operation. Fresh independent integration review is next;
publication waits for queue disposition, and merge requires all hosted gates.

Before: ordinary CI omitted existing artifact checks. After: the same checks
run on all three OS jobs; portable failures fail CI. Native validator passes
on hosted macOS/Linux are not claimed. Reverting removes this extra check
without changing product behavior. Installed/native/dedicated paths remain
**unverified on real hardware**; M2 is incomplete and E2 stays held.

## Review completion checkpoint

Independent Opus high integration review passed at 57b0f71; its
[report](../reviews/service-artifact-ci-main41-integration-review.md) is
retained from dca9314. The reviewer independently reran the artifact checks
(3 passed / 2 skips) and workflow guard (1 passed), and verified all relevant
Git blobs and workflow invariants. This checkpoint changes docs only.
Main after #41 passed in 481s. PR42 still awaits Windows CI; this candidate
remains local to avoid refreshing an extra public branch as that queue lands.

## Publication refresh onto main after PR42

Actual publication base is 4c2264d6c3caee9507e11870ad361daea6c1dfdf,
PR42 merged 09:32:22 UTC through all five gates. The workflow at this candidate
is byte-identical to reviewed 57b0f71; the script/packaging remain unchanged.
Only the same two lines are added against the new base. Both retained reviews
are unchanged. On the refreshed candidate the coordinator reran the artifact
script (3 pass / 2 native skips, 0.002s) and workflow guard (1/1 pass), plus
Git byte checks, UTF-8/LF/relative links and whitespace. No Rust suite repeat
or hosted/native pass is claimed. Main run43192357855 is pending. Fresh
published-head review and all hosted gates are required before merge.
