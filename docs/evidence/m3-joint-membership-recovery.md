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
