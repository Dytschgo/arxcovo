# M3 joint-membership recovery: implementation ownership stop

Sol 6.1 high implementation sub-agent, 2026-11-17; no children.
Prepared isolated base/source HEAD:
`8f35d0b03c0e63a92b5a3c8df36cc6859e50ebc1`.
Outcome: BLOCKED before implementation and before the verification round.
This is source inspection evidence, not a reproduced runtime failure.

## Contract and stop condition

Read AGENTS.md, docs/KICKOFF.md, the accepted
[proof brief](../briefs/m3-joint-membership-recovery-proof.md), and
[root joint review](../reviews/m3-joint-recovery-contract-root-review.md).
Root accepted seven real-engine scenarios under one 75 s entry deadline,
including startup and cleanup, with each operation capped at 5 s. The active
fixture must reserve 5 s for cleanup, remove all router-held engines before
concurrent shutdown, and check all shutdown results and store release before
asserting the body result. Unexpected setup errors and timeouts must also clean
up; detached cleanup is excluded.

The task explicitly directs: "If API/ownership/cleanup needs frozen file change
stop report before workaround." The startup-timeout path needs disposition
before a compliant fixture can be supplied. No frozen file was changed and no
workaround was implemented.

## Source-supported ownership gap

1. `crates/arxd/src/cluster/consensus.rs:328-338` starts an internal
   `tokio::task::spawn_blocking` closure that opens the vote/log and state stores
   and runs preflight. The join handle is awaited internally. The caller has
   neither this handle nor store-release witnesses while startup is pending.
2. Release handles and weak store references are obtained only after that await
   (`consensus.rs:339-344`). `ConsensusNode` is returned only after subsequent
   asynchronous engine creation (`consensus.rs:346-365`). Existing shutdown and
   release assertions require that returned node.
3. Cached locked Tokio 1.48.0 `src/task/blocking.rs:106-124` documents that a
   started blocking task cannot be aborted. Runtime shutdown waits for it;
   dropping a timed-out async caller does not prove completion or store release.
4. Engine startup also reads through the frozen adapter's `blocking` helper
   (`consensus/adapter.rs:30-47`), which similarly awaits an internal blocking
   task. Public engine construction is not an external ownership witness for
   those pending reads.

Consequently a timeout wrapper around `ConsensusNode::start` alone cannot
establish the requested ownership/cleanup contract: an open/preflight task may
still be running when the wrapper returns. Retaining the startup future into
cleanup preserves the ability to await it, but if it remains pending through
the reserved window, the fixture still has no returned node/release witnesses
to shut down or inspect. Awaiting it without a bound abandons the shared
deadline; dropping it or detaching a cleanup task abandons the ownership rule.
No runtime stall was induced, and no assertion is made that ordinary startup
will stall. This gap concerns the explicitly required timeout path, before any
membership scenario or checker can run.

Root must settle the startup ownership contract or explicitly revise its
failure-path requirement before implementation resumes. This report does not
prescribe a production/helper change or treat one as authorized. It also does
not reopen the accepted parked-future or equivalent-control decisions.

## Exact inspected source and retained log

Git blob pins at the prepared source HEAD:

| File | Blob |
| --- | --- |
| `crates/arxd/src/cluster/consensus.rs` | `5b83422bf8d0cc56b7c76ad15609543c40c14e00` |
| `crates/arxd/src/cluster/consensus/adapter.rs` | `181e9dce6765b1db9339c254ec2dcead9b88a0b9` |
| `docs/briefs/m3-joint-membership-recovery-proof.md` | `2f58a101420cc05fe02abbfffe7fe214aaae48f3` |
| `docs/reviews/m3-joint-recovery-contract-root-review.md` | `65c5443c39abee8535b77159dd2d5bb6e945bb4b` |

Cached Tokio `src/task/blocking.rs` SHA256:
`821efe697c70bf6b4d3203d5fad43d466ade5707fe03f5e925db6d6b91535e71`.

Retained repository-relative log:
`target/m3-joint-membership-58ce4f45fbf8424f9bb53c74acebc9ba/startup-ownership.log`.
SHA256: `43f7f0356b3088bd62b7fb2f5827e352a765a68fc65ec3262f6d8773a6c10477`.
It records bounded excerpts of the frozen startup/adapter and cached Tokio
sources. Its sibling `tmp` directory is unique to this task. Both are retained;
no existing cache was deleted. Offline and dev/test debug=0 were explicitly set
in the log-creation process; no Cargo command used them because no build ran.

## Actual checks versus unexecuted gates

Actual: local source/document inspection, initial exact HEAD and clean-worktree
inspection, source blob pins and SHA256 log capture. The evidence-only commit
also checks this file for ASCII, UTF-8 without BOM, LF line endings and
whitespace errors. These are document checks, not engine acceptance evidence.

Unexecuted: fmt, locked/offline workspace all-target Clippy, pure quarantine
guard mutants, sampler tests, nextest listing, every Rust runtime scenario,
Ubuntu/macOS required CI and independent source review. The single permitted
implementation verification round has not started. No natural compile,
Clippy, static-suite or listing red occurred; no rerun or repair was attempted.
No source was compiled or tested in this task.

No new Rust test, binding, guard inventory or ADR classification was added.
The current quarantine remains 27 fixture cases with the original sample of 8;
28/sample8 is the accepted future implementation target, not achieved evidence.
Old bodies/helpers, mem_net, adapters, production, Cargo/lock, nextest,
workflows and sampler remain unchanged. PROGRESS remains coordinator-owned.
No engine or background process was launched; all inspection processes exited.

## Coordinator handoff and reversibility

Proposed PROGRESS update: Sol 6.1 high builder stopped before implementation on
startup-timeout ownership under the frozen `ConsensusNode::start` API; evidence
is this file and its pinned local log. No implementation checks or runtime
credit. Root disposition required before code or verification. Last merged PR
and existing repair counters remain untouched.

Owner-supplied reference (not queried): source main `a7200aa2`,
CI44081103783 attempt 1, 694 s wall / Windows 689 s. Green does not establish
Windows <=660 s headroom. PR170 documentation awaits gates and was untouched.
Independent source review, first positive/equivalent-control engine execution
through ordinary required Ubuntu/macOS CI, exact hosted feature-head Windows
<=660 s, all five normal gates and coordinator publication/merge remain pending.
No push, PR, remote status, CI action, watch, merge, installation, admin step,
VM action or network fetch occurred. Remove/revert this evidence-only commit to
undo the repository change; leave the retained target log for diagnosis.

Sol3/3 exhausted and Opus0/2 unchanged. M2 remains open, M3 unchecked;
hardware paths are **unverified on real hardware**.

## Resumed implementation round - 2026-11-17

Sol 6.1 high implementation sub-agent, no children. Prepared isolated base
`bce90cff5f42991b4f14ed30d8a46253ad564b86`; resumed under its amended startup
disposition and owner-supplied independent PASS 6948230778. No remote state was
queried. The previous bd9db531 stop above remains historical source evidence.

Source commit and exact tested source:
`73533941b76ae869bba43ae8947358f092eb7198`.
Outcome: STOP on the first natural compile failure in the single authorized
verification round. No fix, rerun, tuning or repair-counter reset occurred.
This is now an actual compile red; no Rust engine scenario ran.

Implemented source includes one new current-thread test, the closed seven-case
Scenario enum in the prescribed order, file-local real-engine fixtures and a
scenario-independent checker. Each fixture attempts the single-poll parked
change_membership_if seam; there is no constructed-joint fallback. ResumeFuture
alone resumes the original Box. Successful control credit requires real API
success, convergence and the designated oracle rejection; assertions are deferred
until router removal and concurrent shutdown. Shared 75 s admission includes
the five-second cleanup reserve. Pending startup remains owned into the reserve;
StartupIncomplete records unresolved cleanup and fails, without a detached reaper.
Runtime-owned blocking work can outlive that failure and runtime teardown; the
unchanged CI job outer bound is not graceful-release evidence.

Source uses public `with_raft_state(|st| st.log_ids.last().cloned())` for the full
tail. Cached OpenRaft's private LogStateReader::last_log_id delegates directly to
that public accessor. This is a test-local public API spelling, with no frozen
helper or production modification. A separate MEMBERSHIP_UNSAMPLED guard and ADR
addendum express the intended 28 fixture / eight sample classification. These
source additions have not passed compile, guard mutants or real listing and must
not be credited as accepted implementation or runtime evidence.

### Actual verification and exact failure

- `cargo fmt --all`: exit 0; `cargo fmt --all -- --check`: exit 0.
- `git diff --check`: exit 0 before source commit.
- Exact four-file implementation allowlist, binding-only byte comparison against
  base consensus.rs, and changed-file strict UTF-8/no BOM/LF: PASS.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  exit 101, first invocation. Compiler E0599 at the new file's lines 253, 352,
  and 681: `borrow_watched` requires its WatchReceiver trait in scope. Compiler
  help names `openraft::openraft_rt::WatchReceiver`. The source is preserved
  exactly as tested; no import or other correction was applied after this red.

The command explicitly set CARGO_NET_OFFLINE=true, CARGO_PROFILE_DEV_DEBUG=0,
CARGO_PROFILE_TEST_DEBUG=0, unique CARGO_TARGET_DIR and task-owned TEMP/TMP.
All build/cache/temp/log artifacts are retained under
`target/m3-joint-resume-7620ed896e0848fabf186454d7873116/`.
No existing cache was deleted. Cargo and its command session completed with
exit 101; no engine fixture, detached cleanup or surviving owned run was started.

Retained ASCII, LF, UTF-8 logs:

| Log | Bytes | SHA256 |
| --- | --- | --- |
| `clippy.log` | 11269 | `2655d816147867b04c2216e4a5a178a30c00232fe56232819bcc4dc614584c1d` |
| `source-checks.log` | 776 | `907b89f13cdb7fcd695acc6fe89e20096edd9bc8712a6d36a8809e7031f32b01` |

Exact tested source git blobs:

| File | Blob |
| --- | --- |
| `crates/arxd/src/cluster/consensus.rs` | `b001ce51dea4b37bf1c95adcb3aab0d701f9bea8` |
| `crates/arxd/src/cluster/consensus/membership_contract_tests.rs` | `8c0bd335789e5031dda5366682f212653a54be57` |
| `tools/ci/test_windows_cluster_quarantine.py` | `266d1cbcf022f11ea325985b1797c9ffad23439c` |
| `docs/adr/0024-windows-three-node-test-quarantine.md` | `a18cfe5ab7a40540c13e9b5277f96ecb28ddf3f4` |

### Unexecuted and coordinator disposition

Pure quarantine guard/mutant suite, sampler suite, and real nextest listing were
not run because the first compile red stops the round. Thus 28 ignored, eight
sample matches, new proof ignored and old pure/TLS/founder required cases remain
unverified by listing. No Windows engine execution, --ignored, focused runtime,
Unix runtime or source/build mutation ran. The six controls are equivalent caller
omissions, not compiled/killed source mutants; their behavior remains unproven.

Independent source review, normal required Ubuntu/macOS runtime, shared-deadline
fit, fresh main verification and hosted feature-head Windows <=660 s remain
pending. Owner-supplied main a7200aa2 / CI44081103783 success at 694 s wall and
Windows 689 s supplies no <=660 s headroom. PR170 and its pending gates were not
touched. No push, PR, remote status, link/watch, merge, network fetch, install,
admin, VM, workflow, sampler, Cargo/lock, nextest or production change occurred.
All old test bodies, helpers, mem_net and adapters remain bytewise unchanged;
PROGRESS is coordinator-owned and unchanged. Root disposition is required before
any correction or additional verification. Preserve Sol3/3 and Opus0/2 unchanged,
M2 open, M3 unchecked, and **unverified on real hardware**.
