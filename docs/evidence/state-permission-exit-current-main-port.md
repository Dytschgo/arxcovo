# Preserve typed state permission exit codes after N2

Coordinator: OpenAI / gpt-6.1-sol / medium, 2026-11-12.
Original integration base: `4c2264d6c3caee9507e11870ad361daea6c1dfdf` (PR42).
Original reviewed source: `7781239b699ab026553ab2957286670d0025785d`,
base 7fefe4d. The [independent Opus report](../reviews/service-state-permission-exit-opus-review.md)
and [original evidence](../reviews/service-state-permission-exit-correction.md)
are retained unchanged, with their checks/mutations credited to those heads.

Native::store now classifies typed layout::Error Permissions/OwnerRequired
as ExitCode::Permissions (4), even under anyhow context. All other errors
remain ExitCode::Store (5), including an untyped I/O PermissionDenied.
The production patch is exactly the reviewed one-line call-site change plus
the six-line private classifier. No other production code changes.

The old test patch failed a Git apply preflight at its previous end-of-file
context; that check changed nothing. The exact reviewed 33-line test was
then appended after the N2 tests. Every pre-existing N2 test byte is retained.
No test assertion, error category or report contract was changed.

Actual coordinator checks on this port, all exit 0:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --workspace --all-targets -- -D warnings`
- One `cargo nextest run --workspace --locked --no-fail-fast --status-level
  fail --final-status-level fail`: **242 passed, 2 skipped**, default profile,
  run `ec5774c0-e8d0-4e79-b87e-532bd1b42301`, 10.057s test phase.
- Exact patch/test comparisons, preserved N2 test prefix, UTF-8/LF/relative
  file links and `git diff --check`.

No new mutation or instrumentation. The skipped subprocess fixtures were
exercised only by normal parents. No actual production store constructor or
state guard, privileged/owner/account/service/SCM/LSA, VM/Lab Manager,
private-state or experiment operation. The ordinary full suite uses its
existing private random test pipes; no fixed service-pipe probe was added.

Open prior advisories: the real Native::store call site is checked by source
inspection, and typed Layout errors still map to 5 as before. This patch does
not claim to settle every layout exit category. ExecutionFailed did not recur,
but its cause remains unknown and E2's red run remains held.

Fresh independent integration review precedes publication, and all hosted
gates remain required for merge. Reverting changes only exit classification.
Native/installed/dedicated paths remain **unverified on real hardware**;
M2 is incomplete and no owner CLI or native executor is activated.

## Independent integration review checkpoint

Opus high review passed at 8c6f69c with no new findings; report4b2e489 is
[retained](../reviews/service-state-permission-exit-current-main-review.md).
The reviewer verified exact original production/test blobs, unchanged typed
error propagation and all existing N2 test bytes. Its synthetic classifier
test passed 1/1 (c5b2a01e); prior full runs/mutations remain credited to their
actual heads. Earlier Native::store call-site and Layout-exit advisories stay
open. This checkpoint changes docs only; E2's red run is not waived.

## Publication refresh onto main after PR44

Actual base: 0fbf8df0b9dbe204a196bc3422c81d12d1bec94c (PR44).
Both Application files are byte-identical to reviewed 8c6f69c. Since the old
base, only artifact CI/docs and the read-only SYSTEM regression were added;
production error propagation and the N2 tests are unchanged. All three
retained review/evidence files are unchanged. Fresh published-head recheck
and all hosted gates are required before merge.

Actual checks on this refreshed source: fmt and locked workspace/all-target
Clippy passed, and the exact synthetic classifier test passed 1/1 with243
not run, run cebb11b1-bb81-4874-9e47-2ce7ab4daa5c, 0.024s. No full suite
repeat is claimed; the earlier242-pass run belongs to8c6f69c. Git byte checks,
UTF-8/LF/relative file links and whitespace passed. Main after43passed623s
(run43194236390); after44run43195912105 is pending. E2 remains held.
