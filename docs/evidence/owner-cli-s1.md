# Owner CLI S1 library evidence — 2026-11-12

Provider: OpenAI / gpt-6.1-sol / high. Local implementation only; independent
Opus source review and all hosted required gates remain outstanding. M2 is
incomplete. All installed/native paths are **unverified on real hardware**.

## Boundary and manifest

Exact prepared base: `b1663060452f8fed25bf64986bc3892bc37c7313`, docs-only
ledger on actual main `d70e80818569491238656255ee415669f04a731d` (PR47).
Initial HEAD matched and status/index were clean. PR47's gates and independent
review are prerequisite evidence supplied by the coordinator; no GitHub writes
or new hosted results are claimed here. Main47 run43204089423 remains pending
in the supplied evidence; main46 run43200970512 passed in614s.

Source manifest:

- `crates/arxd/src/service/config.rs`: DEFAULT_LIMITS, unchanged principal rule,
  borrowed non-Debug/non-Clone ConfigureRequest; private Wire -> serde -> full
  parse. macOS admin and duplicate env guards precede every account/revision/
  workspace observation; build/client must be User. Existing validation is reused.
- `crates/arxd/src/service/executor.rs` and `executor/tests.rs`: shared
  `windows_component_safe`; accepted COM0/LPT0/COM10 behavior preserved.
- `crates/arxd/src/service/native.rs`: state_members extracted unchanged;
  service state_guard retains require_service_identity. Windows owner start
  runs observe_layout, inventory read, state members with retained pins and
  drop on success/error. Private operation/parser seams exercise the actual
  shared sequence and inventory parser without native owner calls.
- `crates/arxd/src/service/owner_unix.rs`: require_owner uses existing four-ID
  rule; Manager::new gates previous-byte reads, no-follow pinned private parent,
  optional missing object, mode0600, bounded bytes and same-inode check.
  Current-user owned-temp tests use existing fixture infrastructure only.
- `crates/arxd/src/service/owner_windows.rs` and `owner_windows_tests.rs`:
  ConfigureSession matches existing signatures; only production implementation
  delegates to WindowsOwnerSession. No batch/store/right/guard constructor added.
- `docs/PROGRESS.md` and this evidence: local task/provider/gate ledger.

No main.rs, owner_cli, client_config, runner, native bridge, CLI integration
test, dependency, workflow, packaging or ADR0019 file changed. S3 composition
and fake-SCM tests remain S3 work; S1 adds only its supporting library seams.
No owner argv, production owner session/guard construction, service, account
administration, privilege enable, VM, private-state or Lab Manager operation ran.
Existing normal tests retain their owned-temp/native-observation fixtures.

## Isolation correction

The initial task named D:/Code/Arxcovo as the isolated checkout. Work began
there on clean b166306. Coordinator corrected that interpretation: the already
prepared worktree is `.t3/worktrees/Arxcovo/t3-2df499cf-e954cc9cf7a311f8`.
At the next mutation boundary all eight task-owned paths were captured as raw
bytes plus binary unstaged/staged patches (index was empty), copied into the
clean prepared worktree at the same base, and verified byte-for-byte with
SHA256 manifests. Commit `dd07995789cce3f6771cb3800c24347af9d618a1`
preserved that exact transferred source before root cleanup. Every root byte
and root HEAD were rechecked; only those eight paths were restored to b166306.
Root was then verified clean. No broad reset, clean, stash, branch switch,
new worktree or unrelated restoration was used. Capture and local test logs
remain outside version control; no private host environment dump is committed.

All focused tests/mutations below ran in the canonical checkout before
migration. Final fmt/Clippy/full suite run in the prepared worktree. Migration
alone caused no repeat full run: none had run before transfer. A subsequent
Unix-only fixture setup correction uses 0755 for shared ancestor directories
and 0700 for private leaf directories; source inspection caught the mismatch
with existing manager fixtures before any Unix test execution. No production
architecture deviation is introduced.

## Local checks and limitations

- `cargo fmt --all`: initial failure from new Unicode text written with the
  Windows local encoding; corrected to UTF-8. No behavior test failure.
- First Clippy after encoding correction rejected unused test-build hooks;
  tests now reference hook signatures without invoking production constructors.
  Later locked all-target workspace Clippy passed before migration.
- `cargo nextest run --locked -p arxd -E 'test(service::config) | test(service::native) | test(service::executor::tests::shared_windows) | test(service::executor::tests::windows_console) | test(service::executor::tests::ordinary_device) | test(service::owner_windows) | test(service::owner_unix)'`:
  69 passed,202 filtered; `fa870e80-00f4-4808-b249-1c79a08a7b47`.
- `cargo nextest run --locked -p arxd -E 'test(service::config::request_tests) | test(service::native::owner_preflight_tests) | test(shared_windows_component)'`:
  9 passed,263 filtered; `727fcf03-c1d2-420f-8f53-b46059772b69`.
  Adds stale-previous-workspace/exact-previous-bytes coverage on Linux/macOS
  test layouts, using fake resolvers on Windows.
- `cargo check --locked -p arxd --all-targets --target x86_64-unknown-linux-gnu`:
  blocked before project code by missing `x86_64-linux-gnu-gcc`. No toolchain,
  dependency or environment modification attempted. Unix-only source/tests
  are not compiled or executed locally; hosted Linux/macOS gates are required.
- Prepared-worktree `cargo fmt --all -- --check`: exit0, repeated after the
  Unix fixture-only correction; `git diff --check` exit0.
- Prepared-worktree `cargo clippy --locked --workspace --all-targets -- -D warnings`:
  exit0 (31.95s first build); final source check exit0 (2.31s).
- Exactly ONE `cargo nextest run --locked --workspace`, default profile/default
  concurrency, no instrumentation: **271 passed,2 skipped**, test runtime10.561s,
  build49.87s; run `9f1416b4-f139-4185-a981-d49f74cf85a1`, exit0. This ran in
  the corrected prepared worktree. The Unix-only fixture setup correction is
  cfg-excluded on Windows; no repeat full run was made. No ExecutionFailed
  recurred in this run; the prior unknown failure remains unresolved.
- Source commits: `dd07995789cce3f6771cb3800c24347af9d618a1` plus Unix-only
  fixture correction `b25797d` (full identifier in Git). Documentation-only
  evidence commit follows; it does not change tested behavior.

Tests cover canonical JSON, exact fields/defaults/order, parser roundtrip,
macOS/duplicate-env observation ordering, both account kinds/absence, full
workspace/rights/env/agent validation, stale prior config revalidation, inventory
no-build/no-workspace/no-revision observations, preflight order/error/pin drop,
and Windows reserved-name controls. No new authority was needed for tests.
The existing BatchObservation can be obtained only through observe_batch with
OwnerStore/BatchRight fakes (existing owner_transactions fixtures); no new
constructor or visibility was added for future S3 compositions.

## Restored mutation checks

Each command was `cargo nextest run --locked -p arxd -E 'test(FILTER)'`.
Every listed mutation compiled, failed a runtime assertion (exit100), and was
restored from captured original bytes in a finally block. Build failures earn
no mutation credit. Logs and manifests are retained locally under target/s1-*.

| Mutation | Filter | Run ID | Result |
| --- | --- | --- | --- |
| mac-first | request_guards_precede | cf13fda3-818e-4ea5-934b-2f38f3f22446 | compiled assertion failed; restored |
| duplicate-env | request_guards_precede | dba68d49-5cb6-4625-97e1-38209d79b1a7 | compiled assertion failed; restored |
| build-user | request_group_missing | 0f628c9a-bc04-4868-9b6b-b9b798f46991 | compiled assertion failed; restored |
| inventory-parser | native_read_parser_makes_only | 468cff0b-27cd-4b9e-9b2f-819892949c56 | compiled assertion failed; restored |
| start-order | owner_preflight_order_errors | f2c4658c-899c-467f-ae37-fa3e02d0721a | compiled assertion failed; restored |
| client-user | request_group_missing | fab6011f-039e-4d06-ae33-beae83be6d50 | compiled assertion failed; restored |

The mutations remove the early macOS guard, ignore duplicate env, skip build
or client User guards, replace the native inventory parser with full parse,
and move config before layout observation. No mutated native/system path ran.
Unix mutation coverage awaits a suitable ordinary-user Unix test host.

## Gates and rollback

Exactly one final normal full workspace nextest run is authorized, with default
concurrency and no instrumentation. A natural red is retained, never retried to
green. The prior unexplained ExecutionFailed bdbf032b is still open; no fix or
failure-rate claim follows from a local pass. Fresh Opus exact-head source
review and all hosted required gates must precede coordinator merge. No push,
PR, check/status write, watch, merge, rerun or babysitter change was performed.
Rollback is a coordinator-reviewed revert of these S1 commits; no installed
state exists to undo. S2 may consume windows_component_safe; S3 removes its
unused-hook allowances when wiring. SYSTEM-private state policy remains
unchanged; elevated-admin access to that state remains unverified.
