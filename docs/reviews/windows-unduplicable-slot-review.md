# Independent review of the handle-slot fixture correction

Reviewer: OpenAI / gpt-6.1-sol / medium, single agent. Diagnosis and test-only
patch author: Anthropic / claude-opus-5-5 / requested high, retained in
`windows-handle-reuse-marker-failure.md` and its patch. Reviewed the full patch
from `7f0f7577c631d56151ff36201739efb6e4e22e27` against main `0ceb174`.

## Standards

No blocking findings. Every source hunk is inside the existing cfg(test) module.
No production spawn, inheritance, Job, token, transport or storage code changes.
The forced unsupported-duplication seam uses only fixed test environment keys.
It scans at most 4096 child-local slots and closes every successful temporary
copy. No production adapter or CLI can select this seam.

## Spec

No blocking findings. An inherited sentinel event remains duplicable and takes
the original copy-and-compare path. The fallback applies only to error 50 plus
a valid occupied slot. Opening the named reference still follows the snapshot;
if it occupies the vacated slot the test fails. Unknown failures do not become
a passing marker. Standard-handle, identity, Job and cleanup assertions remain.

The forced-ETW test covers the fallback, not a real sentinel leak. The original
real-path test retains that separate responsibility. The diagnostic author's
reference-before-snapshot mutation did not reproduce its historical failure;
no new coverage claim is made for that older guard.

## Checks personally run on Windows, unelevated

- Fixed deterministic regression: passed.
- Disabled only the new fallback: deterministic regression failed.
- Removed the real production handle-list update in scratch: original real-path
  test failed with `unintended inherited sentinel object`.
- Restored all source bytes, then formatting and locked workspace/all-target
  Clippy passed.
- Full nextest on the main-based candidate: **61 passed, zero skipped**, run
  `a8b35cca-4c7a-45db-8323-92e2f046dbdb`, 7.416-second test phase.

The author's 1300 original-test repeats and full 147-test branch runs are
separate reported evidence, not checks repeated by this reviewer. Hosted checks
remain pending. Reverting affects only tests and records. No account, right,
privilege, service, manager, owner installation, Lab Manager or VM operation ran.
This does not establish installed-service or dedicated-account M2 acceptance.
