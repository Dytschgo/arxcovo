# ADR 0018 S2 correction attempt 1: independent Opus recheck

Reviewer: Anthropic / Opus 5.5 / high. This is a single-agent review with no
subagents. It rechecks the OpenAI / gpt-6.1-sol / high S2 correction attempt 1
against every finding in [the prior review](0018-s2-independent-review.md).
Date: 2026-11-12. The review grants no implementation, publication,
acceptance or activation authority.

## Pins and scope

- Target: `d3cf449f54be5f9c292b5be68abc39850de6a5c2`. Correction source:
  `fd57bbd20181c807df296a623fb43f8e7f63cda7`. Evidence:
  `753e9cfaba1251e58eaf923849288bba24e007df`. Correction base:
  `0890dc1531cf709cc04df9a8bdf1311034e303f0`. Main: `77dcce64f097ecbf5c66601920a60fb03436c57b` (#48).
- Worktree: `C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-a22ca452-3db8d2fb0cf625c8`.
  I checked that pwd and the Git top-level match, that HEAD equals the
  target, and that the tree is clean. It is not the canonical
  `D:/Code/Arxcovo`, which I did not touch. All builds and mutations ran in
  an owned scratch `git archive d3cf449` extraction with a scratch
  `CARGO_TARGET_DIR`. The only file I added to the worktree is this report.
- Diffs reviewed: `0890dc1..fd57bbd` (source), `fd57bbd..753e9cf` (evidence),
  and `753e9cf..d3cf449`. The last one changes only `docs/PROGRESS.md`, +4/−2:
  a coordinator checkpoint plus two CP1252 `0x97`/`0x96` dash bytes turned
  into UTF-8 `—`/`–`. The old file failed `iconv -f utf-8` and the new one
  passes. No history was removed.
- Spec: ADR 0018 decision 7; brief `docs/briefs/owner-cli-wiring.md` §3 S2,
  §7 test 9 and its mutations. Standards: AGENTS.md, CONTRIBUTING.md,
  KICKOFF rules, and the Fowler smell baseline (judgement only).

## Verdict

- **Standards: PASS with advisories.**
- **Spec: CHANGES REQUIRED.** There is one blocker, SP-3R: the Windows native
  `Verify` call site is still not protected. Suppressing its result, or
  mapping an acquisition failure to `Ok`, passes every test. Both mutants
  compiled and survived. The brief requires both cases to fail a test. I
  missed this gap in the prior review; it has existed since the original S2.
  SP-1, SP-2 and SP-4 through SP-8, plus the Windows Write/Flush part of
  SP-3, are resolved or correctly marked pending hosted.

S2 publication is not supported until SP-3R is corrected and rechecked. This
was correction attempt 1 of 3.

## Spec

**SP-3R (Medium, blocker, required test): the Windows native Verify result is
not protected, `windows.rs:365-366`.** Brief §3: "Both acquisition errors
and checker rejections are required negative cases". Test 9: feed actual Err
results for "handle-fact acquisition"; "Tests must also catch skipping the
checker or suppressing its rejection." Mutation list: "bypass the Windows
checker, or ignore its rejection". Every Windows negative reaches
`verify_with` directly: `tests.rs:639-643` (synthetic facts) and
`Ops::verify` with `acquisition` (`tests.rs:220-221`). No test calls
`NativeFileOps::verify` on a handle that should fail. `Fixture::private`
calls it only on valid files. On the unmodified target tests (15/15):

| Mutant (`windows.rs:366`) | Run | Result |
| --- | --- | --- |
| `let _ = self.verify_with(\|\| facts(f), expected); Ok(())` | `ffbc900e` | **survived** |
| `match facts(f) { Ok(x) => self.verify_with(\|\| Ok(x), expected), Err(_) => Ok(()) }` | `f289dc7b` | **survived** |

Production `create` passes `NativeFileOps`. Either mutant would let a
wrong-length or wrong-DACL file, or a failed GetSecurityInfo call, return the
hash. Exact correction: add a Windows ordinary-user test. I ran this one in
scratch only and did not commit it:

```rust
#[cfg(windows)]
#[test]
fn windows_native_verify_rejection_and_acquisition_reach_partial() {
    use std::os::windows::{fs::OpenOptionsExt, io::AsHandle};
    let f = Fixture::new();
    f.run(&mut Ops::new()).unwrap();
    let before = f.bytes();
    let len = before.len() as u64;
    let held = fs::File::open(&f.out).unwrap_or_else(|_| panic!("held fixture"));
    let mut native = platform::NativeFileOps::new().unwrap();
    assert!(native.verify(held.as_handle(), len).is_ok(), "valid native control");
    assert!(matches!(partial(native.verify(held.as_handle(), len + 1)),
        Err(ClientError::Partial)), "native checker rejection");
    // Attributes-only handle: real GetSecurityInfo acquisition failure.
    let meta = fs::OpenOptions::new()
        .access_mode(windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES)
        .open(&f.out).unwrap_or_else(|_| panic!("attributes-only fixture"));
    assert!(matches!(partial(native.verify(meta.as_handle(), len)),
        Err(ClientError::Partial)), "native acquisition failure");
    drop((held, meta));
    assert!(before == f.bytes(), "verify changed bytes");
    f.private();
}
```

It passed on the target (16/16, run `62506f93`). It killed both mutants with
compiled assertions: `07361eac` failed at "native checker rejection" and
`1d294c21` failed at "native acquisition failure". Afterwards the scratch
`tests.rs` was restored and byte-checked (SHA-256 `44f8e556…` equals the
`d3cf449` blob). The builder should record both mutants and their kill runs
in the correction evidence. On Unix, `unix_real_invalid_verify_retained_states`
already goes through `Ops::verify` to the native `verify`
(`tests.rs:571-582`). That path is **pending hosted**.

**SP-1: RESOLVED (Windows).** `open_dir` requests
`FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | READ_CONTROL` with share
read/write and no delete (`windows.rs:132-133`). The test at `tests.rs:953-979`
runs the real walker under a private chain. It asserts that renaming the
immediate parent `inner` and the intermediate `private` fails while they are
pinned, that the credential does not exist, and that after release both
renames succeed and are restored. I reproduced both kills: removing the data
right gave `2899e187` and adding `FILE_SHARE_DELETE` gave `5e9ff7ac`. Each
failed at "pinned directory rename refused". Advisory A-1 (end of Spec) covers what
the intermediate assertion does not isolate.

**SP-2: RESOLVED by inspection, pending hosted.** `unix.rs:50` passes
`libc::c_uint::from(mode)` to the variadic `openat`. I ran no Unix build or
stub check this time. Real hosted Linux and macOS build/test remain required.
I make no local Unix claim.

**SP-3: Write/Flush RESOLVED; Verify call site open (SP-3R).** `result_with`
is gone from both platforms. The native methods return raw `write_all`,
`bool_result` and `cv` results (`windows.rs:357-364`, `unix.rs:169-180`). The
synthetic matrix now returns `Err(error())` directly (`tests.rs:185-192`,
`204`) and claims nothing about native calls. `native_readonly_handle_errors_reach_partial`
(`tests.rs:900-950`) drives the real native Write and Flush on an owned
read-only handle through the actual `partial` classifier. I reproduced the
kills: write-ignore `2744c61a` ("native Write failure") and flush-ignore
`9a71bd94` ("native Flush failure"). On Unix, the O_RDONLY write and the
pipe-descriptor fsync and parent fsync exercise the native call sites
(`tests.rs:927-945`). `cv(-1)`/`cv(0)` cover the converter (`unix.rs:188-192`).
All of that is **pending hosted**. If macOS `fsync` on a pipe were to
succeed, the hosted test would fail red rather than pass falsely. See A-2 for
Unix `mode`.

**SP-4: RESOLVED.** `fixture_negative_parent` (`windows.rs:481-500`) creates
owned-temp parents: one with a real deny ACE for the synthetic
`S-1-5-21-100-200-300-999`, one with `D:NO_ACCESS_CONTROL`. The current-user
SID is used only to build the fixture; no test asserts on it or prints it. The
test at `tests.rs:982-1015` expects `Parent`, no post-create call, no target,
an empty directory, unchanged facts and mtime, and wipe (1,1). I reproduced
the kills: skip-deny `68a67cfe` and NULL→empty `0e5209e0`, both at "real ACL
refused/no hash". There is no account or privilege change.

**SP-5: RESOLVED (Windows), pending hosted (Unix).** In `tests.rs:1018-1069`,
the junction is the intermediate `private`. The final parent `inner` is
genuinely private, so only the per-ancestor reparse check can refuse. Kill
reproduced: `da2a5a43` at "intermediate link refused/no hash". The Unix arm
uses a real intermediate symlink with a 0700 final parent. I only inspected
it.

**SP-6: RESOLVED by inspection, pending hosted.** `tests.rs:396-446` pins an
existing private parent first. It then asserts no group/other bits before
Mode, exact `0600` when `last != Mode`, and unchanged parent mode, UID and
GID. The `last` logic is correct for the fail, prefix and acquisition rows.
See A-3.

**SP-7: RESOLVED by inspection, pending hosted.** The pure
`elevated_ids` (`unix.rs:31-38`) refuses a root real or effective UID, a UID
mismatch and a GID mismatch. That matches ADR 0018 "real and effective IDs
that differ" and S1 `owner_identity`. The matrix at `tests.rs:1075-1088` is
synthetic and performs no identity switch.

**SP-8: RESOLVED; new gates at the final head are still required.** The
blobs at `6c452ec` and `0890dc1` are equal for `client_config.rs`
(`0b96530b`), `tests.rs` (`671f432e`), `unix.rs` (`9da58203`), `windows.rs`
(`accb36ed`) and `lib.rs` (`b43adc8d`). `77dcce6..0890dc1` changes `lib.rs`
by `+mod client_config;` only. `git diff 77dcce6 d3cf449` is empty for
`crates/arxd/src/service`, `main.rs`, `Cargo.toml`, `Cargo.lock` and
`.github`, so every S1 follow-up is preserved. The retained review blob
`b9c904a1` equals `9d47041`. The historical evidence base `3aa43887` equals
`fe8e9a7`. Source blobs at `d3cf449` equal `fd57bbd`, and they match the
correction evidence table. Required checks still have to pass at the head
that gets published.

**Spec advisories (non-blocking):**

- **A-1 (Low, test strength), `tests.rs:962-968`.** A compiled mutant that
  keeps only the final-parent pin (`if final_parent { pins.push(handle); }`)
  **survived** (`70b7b5fc`). On NTFS, the open immediate-parent handle with a
  data right and no delete sharing already blocks renaming its ancestors. So
  the intermediate assertion holds without the intermediate's own pin. The
  pins' walk-time role, between checking component *i* and opening *i+1* by
  path, is verified by inspection only. Record this limit in the evidence. A
  race test is not required.
- **A-3 (Low, test strength).** Under a typical umask, `openat(0600)` already
  yields mode 0600, so the exact-0600 assertion cannot tell a no-op native
  Mode apart. This is consistent with the carried S3/hosted items.

## Standards

**ST-1: RESOLVED.** `owner-cli-s2.md` now labels itself historical. It says
the original metadata-only pins did not enforce rename refusal, that
`result_with` did not prove native propagation, and that the synthetic facts
did not prove real ACL acquisition. It records S1 as merged in #48. The blank
line that broke the blob table is removed. The original results remain
credited only to `6c452ec`.

**ST-2/ST-3: RESOLVED.** The `result_with` middleman is removed. The
duplicate final-parent `check` is removed, leaving one fresh facts check after
the loop (`windows.rs:299-305`). The junction, public-parent, deny and NULL
tests still refuse. Consolidating the identity helpers, the ST-4 fixture enum
and naming, and the ST-5 scoped `dead_code` are deferred. That is acceptable.

**Standards advisories (non-blocking):**

- **A-2 (Low, evidence accuracy), Unix `mode`, `unix.rs:169-170`.** fchmod
  result propagation is covered only through the `cv` converter. The prior
  review accepted that option. A hosted mutant that ignores fchmod's result
  would survive under an ordinary umask. Do not list "mode" among the expected
  hosted kills unless a Linux-only `O_PATH` descriptor test (fchmod → EBADF)
  is added. Otherwise record it as converter-only.
- **A-4 (Low, docs).** `docs/PROGRESS.md:3` and `:15` are both
  `## Current state — 2026-11-12`, and the new section repeats the "Last
  merged PR" and "Milestone" bullets. Rename the older heading to a historical
  checkpoint on the next PROGRESS update.

## Retained invariants (inspection plus Windows tests)

`client_config.rs` is byte-unchanged since the original (`0b96530b`). The
wipe owner is built before RNG, and partial-fill Err and unwind go through
Drop. The SHA-256 of the ASCII token is computed before any wipe. There is a
single owned buffer with an escaping-aware bound checked before RNG and no
growth. Exclusive create is followed only by the single `partial` classifier,
with no hash, later operation, reopen or rollback. The new tests do not call
production `create`, `ClientConfig::load` (they use `serde_json::from_slice`),
owner `NativeOps`, OwnerGuard, sessions, native preflight or store, the
resolver, fixed system paths, pipes, SCM/LSA/account/privilege/service, VM,
Lab Manager or experiments. They use the library `Parent::open`,
`NativeFileOps` and `partial`. Assertions use fixed labels, and `Facts` has no
`Debug`.

## My commands and results

All runs used Windows 11 as the current ordinary user, on the scratch
extraction of `d3cf449`, with
`cargo nextest run --locked --no-fail-fast -p arxd -E 'test(client_config::)'`.

- Baseline: `e048d8dd`, 15 passed, 272 skipped. Final run after all
  restorations: `9fdb0c00`, 15/15. `cargo fmt --all -- --check`: exit 0.
  `cargo clippy --locked -p arxd --all-targets -- -D warnings`: exit 0
  (arxd only, not the workspace).
- Mutations. One span was replaced each time and the captured bytes were
  restored in `finally`. Every SHA-256 restore check was true. At the end,
  `git hash-object` of all four scratch sources equaled the `d3cf449` blobs.

| Mutation | Run | Result |
| --- | --- | --- |
| pin data right removed | 2899e187 | killed (rename test) |
| pin `FILE_SHARE_DELETE` | 5e9ff7ac | killed (rename test) |
| native write ignored | 2744c61a | killed (readonly test) |
| native flush ignored | 9a71bd94 | killed (readonly test) |
| facts skip non-allow ACEs | 68a67cfe | killed (deny/NULL test) |
| facts NULL DACL → empty | 0e5209e0 | killed (deny/NULL test) |
| per-ancestor reparse removed | da2a5a43 | killed (intermediate test) |
| Partial → Io | 113406c4 | killed (3 tests) |
| native Verify result ignored | ffbc900e | **survived** (SP-3R) |
| native Verify acquisition → Ok | f289dc7b | **survived** (SP-3R) |
| intermediate pins dropped | 70b7b5fc | **survived** (A-1) |
| SP-3R candidate + verify-ignore | 07361eac | killed |
| SP-3R candidate + acquisition → Ok | 1d294c21 | killed |

**Evidence credits, not reproduced by me:** the builder's single full normal
workspace run `b51167a4-e3aa-4305-9673-b5a423869e12` (286 passed, 2 skipped)
at `fd57bbd`, its workspace all-target Clippy, and its focused run
`f84ea768`. I ran no full workspace, stress, instrumentation or
retry-to-green. No natural ExecutionFailed occurred in my runs. Coordinator
checkpoint: main CI `43213333692` at `77dcce6` succeeded in 637s. I credit
that to the coordinator and did not verify it.

## Trust, secrets and limits

All credentials were deterministic and synthetic, under owned temp roots. I
read no real credential and not `private-state.json`, and I did not run the
ignored fixtures directly. I made no GitHub writes, push, PR, watch, merge,
dispatch or rerun, and I changed no installation or toolchain. Unix behavior,
compilation and mutations are **pending hosted** Linux/macOS. Native, installed
and dedicated paths remain **unverified on real hardware**, and M2 is
incomplete. No installed state exists. Source rollback is a reviewed revert.
Correction attempt 1 of 3 did not pass. If three attempts fail, hand off to
the other provider with failure evidence.

Findings: Standards 0 blockers and 2 advisories (worst: A-2, the hosted
mutation list implies a Unix fchmod kill the tests cannot provide). Spec 1
blocker and 2 advisories (worst: SP-3R, the Windows native Verify result can
be suppressed without any test failing).
