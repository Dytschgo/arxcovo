# Owner CLI S2 correction attempt 1

OpenAI / gpt-6.1-sol / high, 2026-11-12. Implementation sub-agent;
no subagents, publication, owner operations or activation.

## Pins, isolation and manifest

Prepared target/base: `0890dc1531cf709cc04df9a8bdf1311034e303f0`, on main
`77dcce64f097ecbf5c66601920a60fb03436c57b` after #48. Initial pwd and Git
top-level matched the existing runtime-prepared worktree, clean:
`C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e1834c72-5ee767a67e1cc3d2`. This differs from canonical `D:/Code/Arxcovo`, which was never
written. Every command used this explicit workdir. No checkout was created.
Original source `6c452ec` / evidence `fe8e9a7` and the full Opus review are
inherited; original results remain historical, not current-head evidence.

Correction source and final normal-run head: `fd57bbd20181c807df296a623fb43f8e7f63cda7`.
The following evidence-only commit adds this record, updates historical S2
evidence and prepends a scoped PROGRESS checkpoint. Its pin is in the handback.

Source manifest: only `client_config/{tests,unix,windows}.rs` changed.
`client_config.rs` and private `lib.rs` module declaration are unchanged.
`git diff --quiet 0890dc1 -- crates/arxd/src/service crates/arxd/src/lib.rs
crates/arxd/src/main.rs Cargo.toml Cargo.lock .github` exited 0 before the
source commit. Thus ALL S1 sources/follow-up tests are byte-inherited; no
S1/service/executor/main/owner_cli/runner/bridge/Cargo/CI edits.
Documentation manifest: this file, `docs/evidence/owner-cli-s2.md`,
`docs/PROGRESS.md`. All main history/provider rows are retained.

## Review dispositions

- SP-1: `open_dir` adds `FILE_LIST_DIRECTORY` alongside metadata rights.
  Ordinary-user owned-temp tests exercise the real Parent walker and refuse
  rename of both the held immediate parent and a held intermediate ancestor.
  Both rename controls succeed after release and restore the owned names.
  Removing the data right and adding share-delete each compiled and failed
  the rename assertion. No NT architecture redesign.
- SP-2: variadic `openat` receives `libc::c_uint::from(mode)`, including Apple
  mode_t promotion. Real Linux/macOS build/test is still mandatory and
  **pending hosted**; no Unix or stub type-check result is claimed here.
- SP-3: removes the trivial `result_with` closure middleman. Native methods
  directly pass raw BOOL/integer syscall returns into `bool_result`/`cv`,
  return the actual `write_all` result, and propagate these io::Results to
  the real shared `partial` classifier. The real read-only owned file handle
  makes native Windows Write and Flush fail and reach Partial; swallowing
  either native result is killed by a compiled assertion. The test asserts
  retained bytes/privacy after both failures. Unix adds read-only Write,
  real pipe-descriptor Flush/SyncParent failures, and cv(-1)/cv(0) failure/
  success conversion coverage for the converter used by fchmod/fsync.
  Unix execution remains pending hosted. The older operation matrix remains
  explicitly synthetic pre-step error/order/prefix coverage; it is not claimed
  as evidence of native syscall failure. Fact acquisition remains verify_with.
- SP-4: ordinary owned-temp parents have a real deny ACE for a synthetic SID
  or a real NULL DACL (`D:NO_ACCESS_CONTROL`). Actual GetSecurityInfo and
  GetSecurityDescriptorDacl facts traverse Parent refusal before any
  post-create call. Tests compare original/final facts, mtime and empty
  directory, require no target/no hash, and observe wiping. Compiled mutations
  dropping non-allow ACEs or turning missing/NULL DACL into an empty ACL fail.
  This covers real NULL-DACL acquisition; synthetic facts retain the absent
  DACL predicate case. No account/privilege manipulation.
- SP-5: adds a real intermediate junction whose final private parent passes
  independently, so omitting the per-ancestor reparse check fails a compiled
  assertion. The Unix counterpart uses a real intermediate symlink and
  private final directory; execution/mutation pending hosted.
- SP-6: Unix operation matrix now starts with an existing private parent,
  requires no group/other bits before Mode and exact 0600 after successful
  Mode, and compares parent mode/UID/GID before/after. Existing invalid Verify
  fixtures retain their explicit exceptions and unaffected assertions.
  Pending hosted, not credited as a local Unix test pass.
- SP-7: pure `elevated_ids` rejects root, real/effective UID mismatch and
  real/effective GID mismatch, matching S1/ADR0018's IDs rule. Synthetic
  identity matrix only, no identity switching. Pending hosted.
- SP-8: correction uses the prepared actual-main parent; all S1 follow-up
  bytes remain intact. Fresh Windows gates below apply to correction source;
  historical original run remains credited only to 6c452ec. Hosted Unix
  gates and independent Opus recheck remain required.
- ST-1: historical evidence now labels original ineffective Windows pins,
  real native-result coverage limits, and merged/reviewed S1 accurately. Its
  source blob table is repaired. Current pin claim is supported by actual
  rename tests, not omission of DELETE sharing alone.
- ST-2: removed trivial middleman as part of SP-3. ST-3: removed duplicate
  final-parent check; one real facts/check remains after the loop. Identity
  helper consolidation is deferred. Optional ST-4 fixture-enum/naming and
  ST-5 scoped dead-code cleanup are deferred; no broad redesign.

The wipe owner still precedes RNG including partial-fill Err/unwind. ASCII
hashing, borrowed serialization, escaping-bound reservation before RNG,
same-serializer pointer/capacity observation, and Partial's exclusive-create
boundary are unchanged. No rollback/reopen/deletion or later method/hash after
error is introduced. Assertions never format token, SID, path or pointer.

## Commands, results and mutation restoration

Windows, ordinary current user; owned-temp deterministic synthetic credentials.
Logs are ignored local `target/s2-c1-*`, with no secret values printed.
No compiler/toolchain was installed. Installed cross target libraries alone
are not a supported native Unix execution environment; no Unix test was run.

| Command | Exit | Result/log |
| --- | --- | --- |
| `cargo fmt --all` | 0 | initial formatting, before source baseline |
| `cargo nextest run --locked -p arxd -E 'test(client_config::)'` | 0 | 15 passed, 272 skipped; `f84ea768-e8e8-429b-8e07-8560edafe1a9`; s2-c1-focused.log |
| `cargo fmt --all -- --check` | 0 | s2-c1-fmt.log |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | 0 | s2-c1-clippy.log; 28.40s |
| `cargo nextest run --locked --workspace` | 0 | **286 passed, 2 skipped**; `b51167a4-e3aa-4305-9673-b5a423869e12`; s2-c1-full-final.log; 11.759s execution |
| `git diff --check` | 0 | before source commit; repeated on docs only |

The first focused invocation did not launch Cargo: PowerShell log redirection
failed because target did not exist. Created the local log directory, then
ran the focused command once. This was not a test failure or retry-to-green.
The shell misleadingly exited 0 after the redirection error; no pass is credited.

Exactly ONE final normal default-profile/default-concurrency workspace run
occurred, after the final source correction and all mutation restorations,
at the source commit above. No instrumentation, stress, retries, timeout or
environment changes. No natural ExecutionFailed occurred. Existing baseline
ExecutionFailed remains unexplained; this pass is no fix or waiver.

Each mutation command is `cargo nextest run --locked -p arxd -E 'FILTER'`;
FILTER appears below. Seven mutants compiled and failed runtime assertions;
none was a noncompile kill. Script/log/manifest are retained locally as
s2-c1-mutate.py, s2-c1-mutation-NAME.log and s2-c1-mutations.jsonl.

| Mutation | Filter | Run ID | Outcome |
| --- | --- | --- | --- |
| pin-data-right | `test(windows_pinned_parent_and_intermediate)` | `6a6dc0b4-0d93-4ac8-a964-387745405d66` | 100; compiled assertion; restored |
| pin-share-delete | `test(windows_pinned_parent_and_intermediate)` | `1d16e4c9-f66f-4018-bcba-5896ce163539` | 100; compiled assertion; restored |
| native-write-ignore | `test(native_readonly_handle)` | `deea562b-e936-48ff-b343-a05c9e24da42` | 100; compiled assertion; restored |
| native-flush-ignore | `test(native_readonly_handle)` | `3ae17503-35be-4478-855b-91c37cd09d98` | 100; compiled assertion; restored |
| facts-skip-deny | `test(windows_real_deny_and_null)` | `13fce042-8127-4ae3-96dc-6a149eebaae0` | 100; compiled assertion; restored |
| facts-null-empty | `test(windows_real_deny_and_null)` | `f9a2b027-f024-4c50-aa8b-4b2b85232da7` | 100; compiled assertion; restored |
| ancestor-reparse | `test(intermediate_link_ancestor)` | `d46ea6d8-7ba9-4835-9e6a-e1c0d4035d71` | 100; compiled assertion; restored |

Captured Windows baseline SHA-256:
`85aac020e5e5343c5b269fa47a6488a4b3d2f88bcb4c5d19a0427876f6f582d5`. Each mutation replaced exactly one source
span and restored the captured bytes in finally; every byte comparison was
true. The first script detector initially labeled its assertion false because
it searched for the literal word assertion; inspection confirmed the panic
at the fixed rename assertion label. Only manifest detection was corrected;
that mutant was not rerun. Remaining runs checked their actual fixed assertion
labels. No source correction was needed after baseline tests or mutations.

Current source blobs at the tested source head:

| Path | Blob |
| --- | --- |
| crates/arxd/src/client_config.rs | `0b96530b0fd6737657d62a3749b9a771c1e6f09d` |
| crates/arxd/src/client_config/tests.rs | `dcdfcc262d718aaeb1e422dc403a4966daef95b0` |
| crates/arxd/src/client_config/unix.rs | `fdc351273e641cf416591895bd50e3155581ddb7` |
| crates/arxd/src/client_config/windows.rs | `4e53527cf95e20c4602ae88c5dc66fd38c30abd2` |
| crates/arxd/src/lib.rs | `b43adc8d8415577f18c84a52b1f59d3316c5a65e` |

## Limits and next gate

Fresh independent Anthropic / Opus 5.5 / high exact-head recheck is REQUIRED
before publication. All hosted checks remain mandatory, particularly real
Linux/macOS build/test and relevant Unix compiled assertion-kill mutations
(write/sync conversion, nofollow, mode/length checks, elevation IDs and parent
preservation). Native Mode currently has only cv conversion/source coverage;
normal creation is already 0600, so no no-op Mode mutation kill is claimed. Stub harnesses, if later used, are type-check evidence only.
Dispatch/output assertions remain S3; no acceptance/activation is claimed.

Correction tests/manual work invoked no production create, ClientConfig::load,
valid owner argv, owner NativeOps,
OwnerGuard/session/nativepreflight/store/resolver, fixed system paths/pipes,
SCM/LSA/account/privilege/service, VM or Lab Manager operation.
The authorized existing benign workspace fixtures ran normally, including
their established synthetic configuration/bridge tests. No private-state/ignored
credential fixture was read directly. No public GitHub/push/PR/watch/dispatch/
merge or babysitter change. No installed state exists to roll back; source
rollback is a reviewed revert. Owned TempDir cleanup is synthetic fixture
cleanup only. Native/installed/dedicated paths are **unverified on real
hardware**; M2 incomplete. This is correction attempt 1; after three failed
fix attempts, hand off to the other provider with failure evidence.
