# E2 authority correction: independent delta recheck

Status: **PASS. SP-B1 and ST-1 are resolved. No blocking findings.** This is a
code review, not acceptance. It approves no merge or publication and does not
complete M2.

Reviewer: Anthropic / Opus 5.5 (claude-opus-5-5), high effort, review sub-agent,
2026-11-11. No subagents. I independently reviewed this Sol (gpt-6.1-sol, medium,
coordinator) change. I did not author it.

## Pinned target and scope

- Head: `3ad3c803739710fe0fa2aa5c0cafb1eff3b7d115`. Parent/base:
  `42718df0005e89c184c75368b4ec8bf2087ad1f1`, which contains the full E2 review
  of `d28cbd7` (`0016-e2-independent-security-review.md`). I verified both with
  `git rev-parse` before starting work. The worktree was clean.
- Delta (`git diff 42718df 3ad3c80`): `owner_windows.rs`, `owner_windows_tests.rs`,
  `owner_install_windows.rs`, `owner_install_windows_tests.rs`, `PROGRESS.md`,
  and the new `0016-e2-authority-correction.md`. I read all of it.
- Read: AGENTS.md, the install-provisioning brief API section ("exact; private
  fields"), the prior full review and the correction report. This is a delta
  recheck. All other production source is the previously reviewed implementation.
  I did not re-review it, and I did not reopen carried-forward advisories.

## Checks actually run

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo nextest run --locked --workspace --all-targets --no-fail-fast`:
  **246 passed, 2 skipped** in two runs. The second run is
  `7f903fe8-736a-44a6-b909-15bf565f326b`. The skipped tests are the existing
  subprocess-only `#[ignore]` fixtures. I did not run them directly.
- The focused S1 baseline (`service::owner_windows::tests::s1_*`, 4 tests) passed:
  `3453a0f0-a1e7-4593-9c33-1ee1ab6cc367`.

### Source mutations

I applied each mutation to the real source and ran the focused S1 suite. I then
restored the source with `git checkout -- crates` and confirmed `git diff --quiet`
after every mutation. No mutation executed a bad pointer or any unsafe path.
Every kill below was an assertion failure in a test that compiled.

| Id | Mutation | Result | Run |
| --- | --- | --- | --- |
| M1 | `OwnerGuard.pins` made `pub(super)` (only that line) | killed: field-set assertion (`owner_windows_tests.rs:1111`) | `0a23f17a-4585-4001-8d44-f9410e522e02` |
| M1′ | every `    layout: Layout,` line made `pub(super)`, including the guard's | killed: same assertion | `262ee4c3-b9a6-4014-97d7-0fef4afc8156` |
| M2 | `Local(pub(super) *mut c_void)` | killed: `unique code sequence` assertion | `6f5b8a62-0b7f-4ce4-9a02-9010aa2cd338` |
| M3 | `#[cfg(any())]` sibling fn containing `OwnerGuard { layout, pins }` in `owner_install_windows.rs` | killed: "construction or destructuring outside its defining module" | `41e4d7ed-dc1b-4f3a-be57-e52a50b41143` |
| M3b | `let super::super::OwnerGuard { .. }` in nested `service/executor/tests.rs` (cfg'd out) | killed: same message | `a3e47118-9308-43c0-a335-483efd226d04` |
| M4 | decoys in a sibling: comment, string `"OwnerGuard { … }"`, char `'é'` | passed (the fence correctly ignores literals and comments) | `c5eab7c8-b89f-446c-b0ea-2cf5ba529cb7` |
| M5 | lexer char-literal rule reverted to the byte rule `b[i+2] == '\''` | killed: the new `'¹'` lexer assertion | `146c1f77-af9d-4d0a-89f5-1420a2ffe043` |
| M6 | lexer else-branch reverted to `i += 1` (UTF-8 advance removed) | **survived** (see ST-R1) | `4df31fa5-1bcf-42f3-8ae9-56dfbfa32c4c` |
| M8 | added `pub(super) fn pins_mut(&mut self) -> &mut Vec<File>` | **survived**, as expected (see SP-R1) | `79733616-93fc-4773-986c-c7cf61b6aecf` |

## Spec axis (brief API and prior SP-B1 requirement)

**SP-B1: resolved.**

- `OwnerGuard { layout: Layout, pins: Vec<File> }` has private fields again. This
  matches the brief's exact struct. The guard has no derive, Clone, Copy or Debug.
- The new methods are all `pub(super)`. None adds authority:
  - `layout(&self) -> &Layout` is a shared borrow.
  - `binary_directory(&self) -> Result<&File, OpenError>` is a shared borrow. It
    keeps the `len-3` index next to `acquire`, which pushes it. This is what the
    prior review asked for.
  - `into_install_pins(self) -> Vec<File>` consumes the guard. It returns
    read-only pins. Nothing can rebuild a guard from them.
- No constructor, mutable accessor or `&mut self` method exists. In the module,
  `OwnerGuard { … }` appears only as `Self { layout, pins }` in `acquire` and as
  the destructuring in `WindowsOwnerSession::open` and `uninstall`.
- Compiler privacy now prevents siblings, including `application`, from forging,
  destructuring or reordering a guard. The `len-3` invariant can again change only
  inside `owner_windows.rs`.
- **Behavior is unchanged.**
  - `binary_directory` uses the same `checked_sub(3)` and `get`. The error
    conversion is now explicit as `Error::Layout.into()`, where `?` used to
    convert it. Both produce the same `OpenError`.
  - `pin()` reads the state parent through the borrowed layout and consumes the
    guard afterwards, so the borrow ends before the move.
  - `binary_check` still runs `structural` and then fixed `BinaryIntegrity`.
  - `install()` still calls `WindowsScm::system(&layout)` first, which opens
    nothing.
  - The order guard → binary → observe → pin → enable → create → register is
    unchanged. The existing S1 order fences still pass.
- The three internal methods go beyond the brief's literal "exact" API, but they
  are `pub(super)`, add no authority, and are the remedy the prior review
  specified. I do not count them as an ADR-worthy deviation.
- **S1 fence.**
  - It asserts the exact private field tokens and the private `Local`
    declaration.
  - It recursively scans `crates/arxd/src` for an adjacent `OwnerGuard` `{`
    outside the defining file. It refuses symlinks rather than following them.
    On Windows the `path != defining` comparison works across `/` and `\`: the
    defining file contains `OwnerGuard {`, and the scan passes.
  - M1–M3b show that it fails on field exposure and on sibling construction or
    destructuring, including in cfg'd-out code. M4 shows that it ignores decoys.
  - As the correction report says, this is a structural fence, not native
    authority acceptance.

**SP-R1 (info, non-blocking).** The fence covers fields and literal
construction. It does not cover a future in-module accessor that leaks mutable
or constructive access, such as M8's `pins_mut`, a `from_parts` or an
`impl Clone`. Such an edit would land inside `owner_windows.rs`, where review
already concentrates. A test could pin the guard's `impl` method list
(`layout`, `binary_directory`, `into_install_pins`, `acquire`). This is optional.

## Standards axis

**ST-1: resolved.**

- `Local`'s pointer is private. Siblings get only `as_ptr(&self)`, a borrowed
  read.
- The only producers are `descriptor` (`pub(super)`, which wraps
  `ConvertStringSecurityDescriptorToSecurityDescriptorW` output) and the private
  `sid`. Neither accepts a caller-supplied pointer. `LocalFree` of an arbitrary
  pointer is therefore no longer reachable from safe code outside the module.
  (`native_windows.rs` has its own module-private `Local`, so it is not affected.)
- **Lifetimes at the changed call sites are sound.**
  - The installer's `open` takes `sd: Option<&Local>`, so `map_or(…, as_ptr)`
    borrows the descriptor and does not consume it. The caller (`create`) holds
    `sd` past `CreateFileW`.
  - `directory()` and the test `unsafe_directory` keep their `Local` alive across
    `CreateDirectoryW`.
  - I9 passes `sd.as_ptr()` while `sd` is live.
  - These are mechanical replacements of `.0` with no semantic change. Clippy is
    clean.
- **The UTF-8 lexer fix is correct.** `b[i] == '\''` is ASCII, so the slice at
  `i+1` is on a char boundary. Every other branch ends on an ASCII delimiter,
  which keeps `i` on a boundary. The slice-panic bug from the earlier focused
  failure is gone: `executor.rs` and `native.rs` contain `"¹"` in literals, and
  `executor/tests.rs` contains `'¹'`, `"C🦀:…"` and similar. M5 confirms that the
  new lexer test kills the old char-literal rule.

**ST-R1 (low, non-blocking test gap).**

- The else-branch UTF-8 advance (M6) has no test that kills it. No current source
  file has non-ASCII outside literals, and the new `'¹'` case is consumed by the
  char-literal rule. The correction report says the lexer "now handles UTF-8
  character literals and punctuation", but only the first half is exercised.
- Reverting that line would make a future non-ASCII identifier or token panic in
  `source[start..i]`. That failure is closed (the fence test errors), so it is not
  a bypass.
- Optional fix: add a non-ASCII identifier case, such as
  `code_tokens("café {}")`, to `s1_lexer_ignores_decoy_comments_and_literals`.

The carried-forward advisories from the full review (ST-2 to ST-6, SP-A1 to SP-A5,
guide notes) are unchanged, and I did not reassess them. The production `len-3`
branch still has no runtime coverage (SP-A4). This correction does not claim any.

## Evidence report and PROGRESS

- The correction report's claims match what I reproduced:
  - private fields and the three narrow methods;
  - no policy or order change;
  - 246/2;
  - both field-visibility mutations killed by assertions.
- I did not re-verify its run IDs. My independent runs are listed above.
- It labels effort as the actual medium coordinator effort and does not relabel
  it as the high route.
- Its "punctuation" wording is broader than the tests (ST-R1).
- The PROGRESS entry is consistent with the source and with this recheck. It
  correctly records that the recheck was pending.

## Limitations

- No production `OwnerGuard::acquire`, `Layout::system`, `WindowsScm::system`, SCM,
  LSA, privilege, service, account, owner-admin, VM or Lab Manager operation ran.
  I made no pipe or network connection and did not run ignored fixtures directly.
  I did not read `.git/acceptance/private-state.json`.
- Installed, native, known-folder, SYSTEM-owner and dedicated-account paths remain
  **unverified on real hardware**. M2 remains incomplete. Tests ran on one
  Windows 11 host. Hosted required gates have not run on this head.
- No push, PR, status, watch, approval or merge was performed. The public queue
  pause (4 PRs) is unchanged. Only this report is committed locally. Every mutated
  source was restored, and `git diff` was clean after each mutation.

## Summary

Spec: SP-B1 resolved, 0 blocking, 1 info item (SP-R1). Standards: ST-1 resolved,
0 blocking, 1 low test gap (ST-R1). Verdict: **PASS** for the delta.
