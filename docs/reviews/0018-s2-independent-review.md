# ADR 0018 S2: independent Opus source review

Reviewer: Anthropic / Opus 5.5 / high. Single-agent review, no subagents. It
reviews the Sol (OpenAI / gpt-6.1-sol / high) S2 private client credential
library. Date: 2026-11-12. No implementation or activation authority.

## Pins and scope

- Target (off-checkout): `fe8e9a711dfabfd64e8be0fc1cc2ed702f158b5a`
  (evidence/PROGRESS). Source commit: `6c452ecf30555764e86af966e34e91e80df1c5e5`.
- Base: `813dd1e575e1f1cbfe50237c43d115a6134b39bd` (S1 local head).
- This report is committed on the prepared worktree at root
  `21638d9c7e6f467689dcd8ca6a167929b62e360f` (S1 PR48), which is **not** the
  target. Worktree:
  `C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-f1834364-70db007e8a61395b`.
  pwd and Git top-level were checked, clean and different from canonical
  `D:/Code/Arxcovo`, which was not touched. The target was read via Git
  objects and an owned scratch `git archive fe8e9a7` extraction.
- Diff `813dd1e...fe8e9a7`: new `crates/arxd/src/client_config.rs` (197),
  `client_config/{tests,unix,windows}.rs` (877/177/488), one private
  `mod client_config;` in `lib.rs`, `docs/PROGRESS.md`,
  `docs/evidence/owner-cli-s2.md`. S1 sources, Cargo, CLI and workflows are
  untouched. Source blobs at `6c452ec` equal the evidence table and `fe8e9a7`.
- Spec: ADR 0018 decision 7, brief `docs/briefs/owner-cli-wiring.md` §3 S2,
  §6, §7 test 9 and its mutations. Standards: AGENTS.md, CONTRIBUTING.md,
  KICKOFF rules, and the Fowler smell baseline (judgement only).

## Verdict

- **Standards: PASS with advisories.** Fix ST-1 (evidence accuracy) with the
  spec corrections.
- **Spec: CHANGES REQUIRED.** SP-1 is a Windows security property that the
  contract claims and the target does not deliver; I showed it with an
  ordinary-user test. SP-2 is a macOS compile error that blocks the hosted
  gate. SP-3 and SP-4 are required-test gaps that I proved with surviving
  compiled mutations.

The rest of the core contract holds, by inspection plus Windows tests:
the wipe owner exists before RNG, the hash covers the ASCII token before
wiping, the JSON bound has no reallocation, and the single Partial
classifier does no rollback.

## Standards

**ST-1 (Medium, evidence accuracy), `docs/evidence/owner-cli-s2.md`.**
(a) "Ancestors remain pinned; … Windows fixed drives and no delete sharing"
says the pins work. SP-1 shows a "pinned" parent can be renamed. (b) "S1 is an
unreviewed local dependency" is out of date: the S1 review and its
follow-up are now at `21638d9`. (c) The source-blob table has a blank line
between the header and the rows, so the rows do not render as a table. (d)
"Operation errors and injected syscall-shaped results meet at
`NativeFileOps::result_with`" overstates the coverage (see SP-3). Correction:
fix these four items when the target is rebased (SP-8).

**ST-2 (Low, judgement: Middle Man / seam strength), `unix.rs:136-139`,
`windows.rs:350-353`.** `result_with` is just `call()?; Ok(())`. It adds no
handling, and the native bodies (`cv`, `bool_result`, `write_all`) sit
outside it. The seam pins only itself. Correction: see SP-3.

**ST-3 (Low, judgement: Duplicated Code).** `windows.rs:297-299` and
`302-308` run the final-parent `check` twice, the second time with a fresh
`facts` call. Keep one. `sid_text`, `token` and `user` (`windows.rs:37-83`)
repeat `local_windows::sid_text`/`current_identity` and
`service::native_windows::sid_string`. That is acceptable here because the
error types differ. Consider sharing them in S3 or later.

**ST-4 (Low, judgement: Primitive Obsession / Mysterious Name).**
`NativeFileOps::fixture_facts(kind: u8)` (`windows.rs:382-418`) maps the magic
numbers 0–10 to fixture meanings, and the test loops over `0..=10`
(`tests.rs:625`). A small test-only enum would name each negative and stop a
case from being dropped silently. The short names `cv`, `fail`, `check` and
`Local` hide what they do.

**ST-5 (Low), `client_config.rs:2`.** The module-wide `#![allow(dead_code)]`
also applies under `cfg(test)`. The brief's pattern is
`#[cfg_attr(not(test), allow(dead_code))]` with an S3 comment. That pattern
cannot work alone, because tests must never call `create`, so `create` is dead
under test. Correction (optional): put `#[allow(dead_code)]` on `create`
only, and use `cfg_attr(not(test), …)` elsewhere, so unused test helpers
still warn.

## Spec

**SP-1 (High, security), Windows ancestor pins do not stop rename,
`windows.rs:128-145` (`open_dir`), `315-327` (path-based `CreateFileW`).**
Decision 7 and brief step 4 say Windows "holds every ancestor handle until
return, so no held component can be renamed or deleted." `open_dir` requests
only `FILE_READ_ATTRIBUTES | READ_CONTROL`. Windows does not enforce share
modes against opens that request no data or delete access. So leaving out
`FILE_SHARE_DELETE` does not protect anything. Evidence: a scratch-only
ordinary-user test (not committed) calls `platform::Parent::open` on an
owned private parent, then `fs::rename(parent, sibling)`. The rename
**succeeded** on the unmodified target (scratch run with the 5 candidate
tests: 4 pass, the pin test fails at its "pinned parent rename refused"
assertion). The file is then created by path. So between the parent and
ancestor checks and `CREATE_NEW`, a principal with DELETE on an ancestor, or
`FILE_DELETE_CHILD` on its parent, can swap a component for a junction.
Intermediate components of the path are followed, so the file is created
somewhere other than the checked directory. Handle Verify cannot detect
this. The explicit `D:P` file DACL still protects the file's contents. The
property that fails is the location and integrity of the credential.
Correction: request a data access right in the pins. Adding
`FILE_LIST_DIRECTORY` to `open_dir` made the rename test pass, and all 11
existing tests plus the 5 candidates passed (run `ec6d8567`). Creating
relative to the parent handle (`NtCreateFile` `RootDirectory`) is a stronger
alternative, if an ADR records it. Required tests: rename of the pinned
parent and of a pinned intermediate ancestor fails while pinned; a control
rename succeeds after release; adding `FILE_SHARE_DELETE` or removing the
data right fails the test. Unix is not affected: it creates with `openat`
relative to the pinned descriptor.

**SP-2 (High, hosted-gate blocker), macOS compile error, `unix.rs:35`.**
`libc::openat` is variadic, and `mode: libc::mode_t` is `u16` on Apple, so
rustc reports `E0617: can't pass u16 to variadic function`. I copied the
target `client_config.rs`, `unix.rs`, `windows.rs` and `tests.rs` unchanged
into a stub-harness crate. `cargo check --tests --target aarch64-apple-darwin`
failed at `src\client_config\unix.rs:35:13`. `x86_64-unknown-linux-gnu`
checked clean. With `libc::c_uint::from(mode)`, `cargo clippy --tests -- -D
warnings` was clean for both targets. Correction: apply that cast. This
type-check uses stubs for `crate::` items. It is not Unix compile, test or
mutation credit for arxd, which hosted Linux and macOS must still provide.

**SP-3 (Medium, required test), native method error propagation is not
exercised, `windows.rs:364-373`, `unix.rs:158-173`, `tests.rs:185-192`.**
The brief requires that "a native-only Io mapping or ignored Err is
detectable", and that the production syscall and the fixture result "meet at
the same result-handling statement." The matrix injects a synthetic `Err`
*before* forwarding to the native step, through `result_with`. The native
bodies never fail. Compiled survivors on the unmodified tests:
`{ let _ = file.write_all(json); Ok(()) }` (run `97367649`, 11/11 pass);
ignoring the `FlushFileBuffers` result (run `3bfa5c55`, 11/11 pass).
Correction: make each native method pass the raw syscall return into the
shared seam, and add real failing-handle tests. A scratch candidate opens a
read-only handle and asserts that `NativeFileOps::write` and `flush` return
`Err`. It killed both mutations (runs `7dbcf5e4`, `b5a6b3a2`). On Unix, use an
`O_RDONLY` descriptor for write. For fchmod/fsync, use a non-fsyncable
descriptor or unit-test `cv(-1)`. Unix remains **pending hosted**.

**SP-4 (Medium, required test), native fact acquisition negatives,
`windows.rs:196-220`.** The synthetic facts bypass `facts()`. These
compiled acquisition mutations survived: drop non-allow ACEs in `facts()`
(run `66b9a135`), which turns the strict rule back into
`security::private`'s skip-deny behavior (the A-A2 mutation); and map
absent/NULL DACL to `Some(vec![])` (run `f27298ad`). The brief says real
GetSecurityInfo gaps are to be recorded only when it cannot be exercised. An
ordinary user can exercise it on an owned directory. Correction: add owned-temp
parent fixtures with a real deny ACE for a synthetic SID
(`D:P(A;OICI;FA;;;<user>)(D;OICI;FA;;;S-1-5-21-100-200-300-999)`) and with
`D:NO_ACCESS_CONTROL`. Each expects `Parent` and no post-create call. Both
candidates passed on the target and killed their mutation (runs `88070290`,
`00d604f3`).

**SP-5 (Low, required test), intermediate ancestor reparse, `windows.rs:294`;
`tests.rs:502-527`, `685-733`.** Both the junction and symlink fixtures are
the *immediate* parent, which the final `check` also rejects. Removing
`|| f.reparse` from the per-ancestor check survived (run `a925ceb1`). A scratch
intermediate-junction test killed it (run `f9df9481`). On Unix, the
intermediate case uses the same `open_at` `O_NOFOLLOW` (inspection only). Add
both intermediate cases.

**SP-6 (Low, Unix, inspection only), matrix assertions,
`tests.rs:111-129`, `415-430`.** After Mode succeeds, the brief requires an
exact 0600 file mode, and it requires no parent change. `f.private()` checks
only `mode & 0o077 == 0`, and the parent check is just an entry count.
Assert `mode & 0o777 == 0o600` for steps after Mode, plus unchanged parent
mode and owner.

**SP-7 (Low, contract wording), `unix.rs:21-23`.** Unix elevation compares UIDs
only. ADR 0018 says "real and effective IDs that differ". The S1
`owner_unix::identity` compares both UID and GID. Either add the
`getgid() != getegid()` refusal with a test, or clarify that the ADR means UIDs
only.

**SP-8 (Process).** `fe8e9a7` is based on `813dd1e`, not on root `21638d9`.
The source paths merge cleanly, but `git merge-tree 21638d9 fe8e9a7`
conflicts in `docs/PROGRESS.md`. Rebase the target, refresh the S1
disposition, and rerun every gate at the new head. The builder's full run
counts only at `6c452ec`.

**Carried to S3 (acceptable, not waived).** The dispatch
`arxd: client-config-partial\n` with empty stdout and exit 1, token-echo and
output capture, and the Unix fchmod-before-create, existing-parent chmod,
mode/length omission and Unix native-result mutations. The evidence records
these as S3 or hosted gates.

**Confirmed by inspection plus Windows tests.** Lexical checks and the
elevation check run before RNG. The non-Clone/non-Debug owner holds `raw`,
`token` and a preallocated `json` before RNG. Partial-fill `Err` and unwind
go through `Drop` (observer). The hex is encoded once, and SHA-256 is computed
over the 64 ASCII bytes before any wipe. `Wire` borrows the token with
`from_utf8`. `serde_json::to_writer` writes into the owner Vec with no
intermediate String. The checked 6× bound is applied before RNG, and
pointer/capacity stay unchanged for both endpoints. File creation is exclusive
(`CREATE_NEW` share 0 `D:P(A;;FA;;;user)` / `O_EXCL|O_NOFOLLOW|O_CLOEXEC 0600`
at the parent fd). Every post-create `Err` goes through the single `partial`
classifier, with no hash, rollback, rename or reopen. The handle is
non-inheritable. The strict allow-only checker uses `AceType == 0` and three
recipients. No test calls `create`, `ClientConfig::load`, `NativeOps`,
`OwnerGuard` or an owner session. No test is `#[ignore]`d. Assertions use
fixed labels, and `Facts` has no `Debug`. Unix wrong-mode, hard-link and
length fixtures have explicit end states and unaffected assertions
(inspection; **not compiled or run by me**).

## My commands and results

All runs used the owned scratch extraction of `fe8e9a7`, a scratch
`CARGO_TARGET_DIR`, Windows 11, as the current ordinary user, and
`cargo nextest run --locked [--no-fail-fast] -p arxd -E 'test(client_config::)'`.

- Baseline: `c1a768e7`, 11 passed / 272 skipped. Final run after restoring
  all scratch files to the target blobs (verified by `git hash-object`):
  `ec3accf8`, 11/11. `cargo fmt --all -- --check`: pass.
- Mutations on the unmodified tests. Each was restored, and the restored blob
  was verified as `accb36ed…` or `0b96530b…`:

| Mutation | Run | Result |
| --- | --- | --- |
| native write swallows Err | 97367649 | **survived** |
| native flush swallows Err | 3bfa5c55 | **survived** |
| `facts()` drops non-allow ACEs | 66b9a135 | **survived** |
| `facts()` NULL DACL → empty | f27298ad | **survived** |
| drop per-ancestor reparse check | a925ceb1 | **survived** |
| add `FILE_SHARE_DELETE` to pins | a0c15bac | **survived** (pins ineffective anyway, SP-1) |
| file/dir SDDL grants Everyone FR | cae91cf6 | killed, 7 tests (real GetSecurityInfo recipient) |
| drop Windows length check | f8fa9d19 | killed |
| Partial classifier → Io | 9f9e5819 | killed |

One more attempt (`facts-drop-nonallow`, first form) had unbalanced braces
and **did not compile**. It earns no credit and was redone.

- Scratch-only candidate tests (5 tests, not committed). They kill the
  survivors as cited in SP-1, SP-3, SP-4 and SP-5. With a scratch
  `FILE_LIST_DIRECTORY` pin, 16/16 passed (`ec6d8567`).
- Unix type-check harness: macOS E0617 at `unix.rs:35`. Linux clean. Both
  clippy-clean with the cast (SP-2).

I did not run the full workspace. The builder's full run
`62ad402c-9a88-413c-81d3-9c3e4bba7a9d` (282 passed, 2 skipped, at `6c452ec`)
is credited to the builder at that pin. I did not reproduce it. I also did not
run Windows Clippy on the target. The builder reports it passed.

## Trust, secrets and limits

Every credential was deterministic and synthetic, under owned temp roots. I
read no real credential and no `.git/acceptance/private-state.json`. I did not
call production `create`, `ClientConfig::load`, owner NativeOps, OwnerGuard
or sessions, native preflight/store, fixed system paths or pipes,
SCM/LSA/account/privilege/service, VM, Lab Manager or any experiment. The
synthetic SIDs in scratch fixtures are not real accounts. Wiping is best effort
only. The sha2 hasher's stack block and allocator, OS or compiler copies are
outside the guarantee, as the contract says. Native, installed and dedicated
paths remain **unverified on real hardware**, and M2 is incomplete. I made
no GitHub writes, push, PR, merge, watch or rerun. After three failed
correction attempts, the work goes to the other provider.

Findings: Standards 5 (worst: ST-1 Medium, evidence states an ineffective
pin and stale S1 status). Spec 8 (worst: SP-1 High, Windows ancestor pins
allow rename, so the create path can be redirected after the checks).
