# Owner-store fixture isolation diagnosis handoff

OpenAI / gpt-6.1-sol / medium, 2026-11-11.
Production candidate2604939 integrated onto reviewed engine4b2b769.
No admin, rights, account, task, service or installed-layout operation ran.

Reproducer: cargo nextest run -p arxd --lib --locked -E 'test(owner_windows)'.
Original concurrent run failures: 3/6; serial --test-threads 1: 4/4 passed.
Failure: n4_parent_pin_allows_publication_blocks_parent_rename line256,
rename after drop(s), OS sharing violation32. It passes when run alone.

A two-sibling fixture probe holds another session after dropping the first;
rename first fails32 until the sibling is dropped. The final regression expects
independent fixture parents to release independently and failed before the
fixture-only change. This proves ancestor interference without assuming a leak.

Fixture-only attempts: stop retaining unowned global ancestors, then allow full
sharing during those brief structural observations. The private parent still
has its production-equivalent no-delete pin. Native owner suite: eight repeated
concurrent passes after both changes. Full workspace: 142/144 passed, n4 and
sibling still fail. Earlier full run: 142/143 passed, n4 failed. The second full
failure's exact stderr was truncated; do not invent its specific assertion.

Production native::open_impl and SQLite state fixtures retain common ancestors,
so isolation across the whole native suite remains to be established. Production
owner_windows.rs was not edited. Do not change its security policy to green a
fixture. Investigate the actual global resource/fixture boundary, or test runner
coordination if appropriate; preserve native rename/DELETE negative controls and
post-drop release checks. A module-only pass is insufficient. Nextest executes
tests in separate processes; the existing Rust static Mutex is process-local.
No arbitrary sleep/retry or disabled assertion counts as a resolution.

Remaining: native OS behavior as SYSTEM, LSA calls and installed-service/M2
acceptance are unverified on real hardware. This branch is a committed candidate,
not a passing delivery.

## Whole-suite cause and test-only correction (Opus follow-up)

Anthropic / claude-opus-5-5 / high, 2026-11-11, base 586c8ca. This was a research
handoff: a diagnosis and a patch artifact only. Production and test source in
the candidate are unchanged, and Sol applies the patch. Every run used
current-user owned temporary fixtures. Nothing called `WindowsOwnerSession::open`
or the real batch right, and no ignored fixture ran directly.

### Exact symptom (base 586c8ca, local Windows)

- `cargo nextest run --workspace --locked --no-fail-fast` failed 3/3 runs:
  142 passed, 2 failed, 2 skipped. The exact panics, identical in every run:
  - n4 at `owner_windows_tests.rs:255:39`, the post-drop
    `rename(d, moved).unwrap()`: `Os { code: 32 ... "being used by another process" }`.
  - sibling at `:502:43`: `dropped fixture parent is released: Os { code: 32 ... }`.
- `cargo test --workspace --locked` (one process, threads) failed 2/2 runs with
  the same two panics at the same lines. The process-local `FIXTURES` mutex does
  not prevent the failure in either runner.

### Interferer bisection (nextest filtersets, each pair run once)

- `owner_windows` alone: 13/13. With `store::`: 14/14. With
  `service::(native|tests|application)`: 45/45. With `development::windows`:
  n4 and the sibling fail.
- Each `development::windows` test paired with `owner_windows`: the five tests
  that launch a child failed n4 and the sibling: `actual_job_tree_*`,
  `child_token_mismatch_*`, `handle_slot_reuse_*`, `junction_cwd_*` and
  `results_count_*`. The four that launch no child passed.
- Minimal red case:
  `-E 'test(=service::owner_windows::tests::n4_parent_pin_allows_publication_blocks_parent_rename) | test(=development::windows::tests::handle_slot_reuse_is_not_inheritance)'`
  went red 6/6 runs. Five runs failed n4 at `:255`. One run failed the reverse
  direction: `development/windows.rs:731:9` `fixture root pin`.

### Cause

Proven:
- An owned-directory OS probe holds a parent directory handle opened with
  `READ_CONTROL|FILE_READ_DATA|FILE_READ_ATTRIBUTES` and backup semantics.
  - Without `FILE_SHARE_WRITE` (share 1 or 5), renaming a direct child fails
    with error 32, and the same rename succeeds after the handle closes.
  - With write sharing (share 3 or 7), the rename succeeds.
  - A pin on the grandparent does not block a rename one level below it, while
    a direct-child rename under the same pin still fails with error 32.
- `development::windows::pin` (production) opens every path component with
  `FILE_SHARE_READ` only, through `open_pinned`. The `%TEMP%` directory is one of
  those components for the tests' tempdir workspaces, and the pin is held while
  the child runs. This is the intended ancestor rule; it is not a leak.
- The n4 and sibling fixtures were direct children of the shared `%TEMP%`, so
  their post-drop renames needed `%TEMP%` opened for write.

Inferred from the NTFS rename semantics the probe exercised: the reverse
failure at `:731` is the same conflict in the other direction. While a fixture
rename holds `%TEMP%` open for write, the share-READ-only pin cannot open it.

Not the cause: owner-store pins outliving `drop`. Every failure is gone once
the renames move under an owned directory, and production `owner_windows.rs` is
unchanged.

### Correction (test-only): `windows-owner-store-fixture-isolation.patch`

- `fixture()` now creates `owned_outer/owner` and keeps the outer `TempDir`. The
  new `prepare(dir)` does the existing ACL and lock setup on that directory.
- The sibling test places both parents under one owned outer directory and
  holds `FIXTURES` directly. If a helper retained an ancestor pin, it would
  still block the sibling's rename.
- Production code, the root pin's sharing (`READ|WRITE`, never DELETE), n4's
  pinned rename and DELETE-open refusals, the post-drop rename check and every
  other assertion are unchanged. The patch adds no sleep, retry, skip, runner
  configuration or global framework.

### Green evidence (patched scratch worktree, same base)

- `cargo fmt --check -p arxd`; `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- Minimal pair: 12/12 runs passed (2/2 tests each).
- Full `cargo nextest run --workspace --locked`: 5/5 runs, 144 passed, 2 skipped.
- Full `cargo test --workspace --locked`: 3/3 runs, 144 passed, 0 failed, 2 ignored.
- Mutation checks on the owner suite (13 tests):
  - (A) The helper retains ancestor pins with `FILE_SHARE_READ`: n4 and the
    sibling fail, the sibling with `dropped fixture parent is released: code 32`.
  - (B) The root pin adds `FILE_SHARE_DELETE`: n4 fails at `:248`, its
    pinned-rename refusal.
  - Both mutations were reverted.

### Limits

- This was one Windows 11 host, current user, local disk. Hosted CI was not run.
- Other tests that rename directly under `%TEMP%` while a development child
  runs could still collide. None appear in the current suite, which passed 8/8
  full runs across the two runners.
- This is not a security review and not M2 acceptance. The installed SYSTEM
  path, LSA and production constructor are **unverified on real hardware**.
