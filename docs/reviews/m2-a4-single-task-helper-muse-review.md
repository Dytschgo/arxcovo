# M2 A4 single-task helper review — Muse (independent architecture/security)

2026-11-14; reviewer Muse (Muse Spark, high effort), independent
architecture/security review of coordinator Sol medium proposal. Document
only; no source, ADR, PROGRESS, build, test, native, fork, process, service,
account, owner, probe, product CLI, network (beyond read-only primary
man7.org pages cited below), GitHub, dispatch, or subagent action. Read-only
git CLI used for pin/diff verification only.

## Reviewed target (exact pins)

- Head under review: `a14057154580eb9eb894b7d1a27657e63ac15309` —
  `docs(adr): propose single-task A4 helper branch`.
- Parent: `32d549ffe70c9fdeeda1354dbc84bb1ca5a70dc` —
  `docs(exec): record A4 passive reentry blocker`.
- Blocker retained from `d8e53321bed9245efe7c83d6041e1ba60978ee53`
  (same blocker document; `d8e5332..32d549f` adds only 2 PROGRESS lines):
  [m2-stagef-a4-source-blocker.md](/abs/path/docs/reviews/m2-stagef-a4-source-blocker.md) —
  ordinary libtest ignored re-entry has a live harness thread plus a worker
  at the handshake, so `--test-threads=1` does not establish a one-task
  helper compatible with `pids.max=1`; E4 `current_exe` re-entry pattern is
  not reusable for A4 passivity.
- Delta verified by `git diff 32d549f..a140571 --name-only`: exactly two files,
  no other tree change:
  - [0020-linux-execution-hierarchy-and-janitor.md](/abs/path/docs/adr/0020-linux-execution-hierarchy-and-janitor.md)
    (D9a binding 1 rewritten; bindings 2–5 untouched in the diff).
  - [m2-a4-single-task-helper-proposal.md](/abs/path/docs/reviews/m2-a4-single-task-helper-proposal.md)
    (new, 18 lines: fixed post-fork waiting branch contract).
- Accepted prior contract (treated as settled, not re-litigated):
  amendment `b4ffb437f0d08e37206421e310d299970c24c628`, recheck
  [m2-a4-a6-amendment-muse-recheck.md](/abs/path/docs/reviews/m2-a4-a6-amendment-muse-recheck.md)
  (verdict Pass — narrow recheck), underpinned by
  [m2-a4-a6-practical-contract-muse-review.md](/abs/path/docs/reviews/m2-a4-a6-practical-contract-muse-review.md)
  (conditional pass with five binding corrections, all incorporated into D9a).
- Proposal scope as worded: replace only D9a's executable-re-entry
  requirement with one fixed test-only post-fork waiting branch; no extra
  executable, Cargo target, production entry, or restart harness. Ignored
  parent fixture forks exactly one single-task child via libc fork; child
  forks nothing, creates no threads; divergent post-fork call graph uses only
  fixed stack state and direct async-signal-safe
  `close`/`clock_gettime`/`poll`/`read`/`write`/`_exit`; parent prepares fixed
  pipes plus an absolute 15 s monotonic deadline; migration limited to one
  child task with Standard profile, >6 s margin, S7-only kill, parent-held
  `waitpid`/SIGKILL; all other five bindings unchanged.

## Standards — PASS (docs-only proposal)

- One-concern conventional commit (`docs(adr)`), two-file scope (ADR binding
  + its correction proposal), exact rationale links (blocker `d8e5332`,
  prior review bound), explicit "no source or run authority" boundary, and
  explicit **unverified on real hardware** labelling satisfy
  [AGENTS.md](/abs/path/AGENTS.md).
- No product-approval flow, no public-publishing default change, no Lab
  Manager/VM contact, no secret or owner data, no CI-budget or review bypass,
  no PROGRESS/ADR-beyond-binding-1/source edit in the proposal itself.
- No Standards findings. This axis grants no implementation or run permission.

## Spec — CONDITIONAL PASS (blocker direction correct, with binding gaps below)

- Blocker direction is correctly preserved: `pids.max=1` is not relaxed, and
  migration above the task limit is not treated as proof. The proposal keeps
  all D9a limits (64 MiB, swap 0, pids 1 before placement, absolute 15 s child
  deadline, >6 s remaining before the single setup, 5 s S7 bound, only S7
  kill on success, parent-held identity/`waitpid` proving SIGKILL, exact
  postconditions, failure-only cleanup, no-adoption rules) and keeps pure
  tests on a mock-only seam that never forks.
- The replacement shape matches the blocker ask: a fixed test-only branch
  that reaches its blocked handshake with one live task while preserving
  passivity, absolute lifetime, parent-held identity/`waitpid`, and the other
  D9a bindings — instead of copying E4's storage-reading/marker-writing
  helper or adding a custom harness, constructor, extra binary, Cargo change,
  production branch, or fork workaround (all still excluded).
- Conditional because three multithread-fork consequences are stated as
  requirements but not yet closed tightly enough to implement against:
  at-fork handler inheritance, inherited-FD closure, and post-fork call-graph
  enforceability (see Security and Actionable findings). None is a rejection
  of the shape; each needs one minimal contract sentence before source work.
- A6 is untouched by this delta and stays on its independent contract; this
  review does not block the separately active A6 builder. M2, A4, remaining
  F/G, and real-Runner PA6 stay open; no acceptance is granted here.

## Security — CONDITIONAL (concrete fork risks; no unproven safety accepted)

Primary sources checked read-only (design constraints, not execution
evidence): [fork(2)](https://man7.org/linux/man-pages/man2/fork.2.html) and
[signal-safety(7)](https://man7.org/linux/man-pages/man7/signal-safety.7.html).

- **Multithread fork model (accepted).** Verified: "The child process is
  created with a single thread — the one that called `fork()`. The entire
  virtual address space of the parent is replicated in the child, including
  the states of mutexes, condition variables, and other pthreads objects."
  "After a `fork()` in a multithreaded program, the child can safely call
  only async-signal-safe functions (see signal-safety(7)) until such time as
  it calls `execve(2)`." The proposal's no-`exec`, wait-then-`_exit` child is
  therefore the strictest form of this rule: its *entire* lifetime must stay
  inside the safe set. The six named calls (`clock_gettime`, `close`, `poll`,
  `read`, `write`, `_exit`/`_Exit`) are each present in the
  signal-safety(7) table (confirmed by fetch). The call *list* is sound; the
  *closure* is not yet proven (see F3).
- **At-fork handlers (gap, F1).** Verified: "The glibc wrapper invokes any
  fork handlers that have been established using `pthread_atfork(3)`." The
  proposal binds "no atfork callbacks installed by this fixture", which
  covers the fixture but not the rest of the test-binary closure (libtest,
  std, transitive crates). Any pre-existing handler runs in the child between
  fork and the audited branch. The contract must require a source-review
  enumeration proving no such handler is reachable in the A4 test binary, and
  forbid adding any; if one is found, it must be removed or proven
  async-signal-safe. Do not assume zero handlers.
- **Inherited descriptors (gap, F2).** Verified: "The child inherits copies
  of the parent's set of open file descriptors." The proposal's "close known
  unused pipe/capture ends" plus "inherited descriptors are never used except
  the fixed handshake endpoints" is the right direction, but "known" cannot
  mean "all libtest/harness FDs". Post-fork enumeration (`/proc/self/fd`
  with `open`/`readdir`, or close-all loops) is itself not async-signal-safe
  and must be forbidden in the child. The sound discipline is: parent creates
  handshake pipes before fork (with `O_CLOEXEC` hygiene); child closes only
  the fixture-created opposite ends whose numbers were passed as stack
  values, touches no other inherited FD, and holds the rest untouched until
  `_exit`. Extra held FDs do not create tasks (no `pids.max=1` effect), but
  the contract must state they stay open-but-unused and that no handshake or
  harness EOF expectation depends on them. Require an explicit FD-ownership
  table at source review; forbid descriptor-value/host-identity logging
  (already bound, keep it).
- **Deadline/handshake (sound shape, needs F4 precision).** Parent-side
  absolute `CLOCK_MONOTONIC` deadline before fork, child recompute of
  remaining time with `EINTR` retry that never extends the deadline, fixed
  failure exits on wrong bytes/EOF/unexpected errors/expiry, and parent
  migrate → verify one child task, live membership/population, and all
  existing parent/supervisor conditions before release — this ordering is
  correct, and "expiry or early death never counts as success; scheduling
  delay is not a hard physical-time guarantee" keeps the race honest. What
  must be pinned: fork happens *outside* the pids-limited leaf (parent
  cgroup), migration moves exactly one task into the already-limited leaf,
  the single-task check reads task/membership state (not mere process
  existence), and poll-timeout arithmetic is saturating/bounds-checked by
  construction (never negative, never wrapping to infinite).
- **Rust-specific closure (gap, F3).** Copying a multithreaded Rust parent
  copies allocator/lock/TLS state; the child is safe only while it never
  touches any of it. "No allocation, Rust I/O, formatting, panic/unwind,
  destructors, locks, environment access, or callbacks" is the right bar, but
  it must explicitly cover *implicit* Rust behavior: no `std` wrappers around
  the six calls (use direct `libc` raw calls), no `Result::unwrap`/`expect`
  or formatting on the post-fork path, no `Drop` types alive across the
  branch, no TLS/`errno`-to-string, no environment lookup, and a divergent
  (`!`-returning) child entry that always ends in `_exit`. `EINTR` is the
  only retried error. Review must inspect the full post-fork call graph and
  enforce the native ignored fixture as sole caller (proposal already
  requires this; keep it and make the forbidden list explicit).

## Actionable findings (minimal contract fixes, no helper framework)

- **F1 — At-fork audit.** Tighten binding 1 with one sentence: source review
  must enumerate `pthread_atfork` registrations reachable in the A4 test
  binary and prove none exists (or each is async-signal-safe); the fixture
  installs none and no new registration may be added.
- **F2 — Inherited-FD discipline.** Tighten binding 1 with: child closes only
  the named fixture-created opposite ends by passed-in stack FD values; it
  never enumerates, closes, reads, or writes any other inherited FD; all
  other inherited FDs stay open-but-unused until `_exit`; handshake pipes are
  created parent-side with `O_CLOEXEC`; source PR includes a pipe-end
  ownership table per phase; no descriptor/host logging.
- **F3 — Post-fork closure.** Tighten binding 1's call list with: direct
  `libc` raw syscalls only (no `std` I/O/time/process/env wrappers), no
  implicit allocation/panic/format/`Drop`/lock/TLS/callback on the reachable
  post-fork graph, divergent child entry always ending in `_exit`, `EINTR`-only
  retry with recomputed remaining time and saturating arithmetic that never
  extends the deadline. Full-graph inspection plus sole-caller enforcement
  stays mandatory.
- **F4 — Fork placement and single-task proof.** Tighten with: fork executes
  outside the pids-limited leaf; parent migrates the one-task child into the
  already-limited leaf, then verifies one child task plus live
  membership/population and all parent/supervisor preconditions before
  release; expiry/early death fails; all D9a numeric bounds unchanged
  (64 MiB / swap 0 / pids 1 / 15 s / >6 s / 5 s / S7-only / `waitpid` SIGKILL).

## Verdict

- **Conditional pass — incorporate F1–F4 as exact contract sentences, then
  proceed.** The shape (ignored parent, one fork, fixed single-task child,
  six-call divergent branch, parent-prepared pipes and absolute deadline,
  preserved pids1 and all other D9a bindings, no extra executable/Cargo/
  production entry) is the correct minimal answer to blocker `d8e5332`
  retained in `32d549f`. Do not implement, run, or claim acceptance until the
  four sentences above are in the ADR/proposal text and the normal
  different-provider review plus green required checks pass with coordinator
  merge.
- Permitted next scope after this review: narrow doc correction only (F1–F4
  wording), then small independently reviewed source slices, then separately
  authorized native execution. No run permission is granted here. M2, A4,
  remaining F/G, and real-Runner PA6 stay open; A6 proceeds independently;
  hardware paths remain **unverified on real hardware**.
