# Independent Opus review of Sol A12 residue producer source

2026-11-15, Anthropic Opus 5.5 / high, review sub-agent, no subagents.
Author: OpenAI gpt-6.1-sol / high.

- Branch `work/m2-a12-residue-source`, review parent `0e64d82`.
- Sol source `8b20ab14f3b1057198730931911c1bda9d9529f5`, original parent
  `f7697f2` atop main `07d8869`.
- Retained as `b6dc735`. Verified: `git diff 734c3e6 b6dc735` is byte
  identical to `git diff f7697f2 8b20ab1`, with the same five paths and
  693+/1-.
- Coordinator commit `734c3e6` (PR135 final A7/A11 NO-GO checkpoint) is the
  inherited parent and outside A12 source scope. `0e64d82` touches PROGRESS
  only, which is coordinator-owned. Neither was reviewed as A12 source.
- Hosted A7/A11 is closed as a FINAL NO-GO. This review does not debug, fix,
  rebind or revive it.

Read: AGENTS, KICKOFF, brief 0020 (A12 and the privileged-acceptance list),
ADR 0007/0009/0017/0020 residue clauses, the evidence file
`linux-a12-residue-source-preparation.md`, and the earlier SDK consumer join
evidence `m2-normal-cancel-residue-join.md`. Also read all five changed paths
and every production callee they reach: `Runner::{new,run_phased,cleanup_residue}`,
`unix::{run,trusted_root,walk}`, `ChildGuard::kill_tree`,
`cgroup::Group::{create,kill_and_confirm,drop}`, `hierarchy::{locate,parse,setup,linux}`,
`fake::{prepare_a1a2,a5::{preflight,empty}}` and `service::unix::Notify::send_to`.
I also compared the fixture with the existing A1/A2 and A7 fixtures in
`exec/disposable.rs` and `disposable/sudo_nnp.rs`.

## Standards: PASS, no blocking findings

- Scope is exactly the five declared paths:
  - new `#[cfg(test)]` file `exec/disposable/a12.rs`;
  - a four-line `#[cfg(test)] #[path] mod a12;` in `exec/mod.rs`;
  - one path entry and one exact declaration in the W8 inactive inventory;
  - one S2b `PINS` hash constant;
  - the evidence file.
- Nothing changes in production, A7, rights, package, workflow, helper, Cargo,
  ADR or PROGRESS.
- The commit message is conventional (`test(exec): ...`). All four source
  blobs match the evidence table, and all are LF (`i/lf w/lf`, `eol=lf`).
- Why `exec/mod.rs` and not `disposable.rs`: `disposable` is gated
  `cfg(all(test, linux))`, but the portable tests must compile and run on every
  OS. The cost is S2b pin maintenance (see below), which the evidence states
  openly. No new framework or seam is added. `mock_result` and `a5::empty` are
  reused rather than duplicated.
- S2b pin: the SHA-256 of the committed LF `exec/mod.rs` is
  `c46c69bf36b3e3225f94e6fafe732771971244c6716e9aa617e17f7fbfa3c90e`, equal to
  the new constant. The parent `f7697f2` blob hashes to the old `f8d425...`.
  The other six pins, `MANIFEST_SHA`, `TARGETS` and the workflow are unchanged.
  `tools/unix-exec-validation/mutation_check.py` slices `exec/mod.rs` only from
  `pub struct OwnerConfig` to `pub enum ExecError`. The new lines are above that
  range, so its slicing is unaffected. Old hashes that appear in historical S2b
  evidence and review documents are records of past state and are left as is.

## Spec: PASS, no blocking findings

### Closed gate

`disposable_linux_a12` is `#[ignore]` and Linux-only. Apart from reading the
`Instant` clock, its first statement is `prepared_delivery().expect(..)`. That
function's body is literally `None`, so even an explicit ignored selection
stops before any environment, UID, umask, filesystem, cgroup, account, setup,
Runner or notify work.

Nothing selects it:

- `ci.yml` runs `cargo nextest run` without `--run-ignored`.
- No driver or workflow names `a12`.

The portable source test enforces three things:

- the body is literally `None`;
- `prepared_delivery()` comes before env/getuid/LinuxFs/prepare/Runner;
- the identity ACK comes before `run_phased`, and the Missing-runs collision
  check comes before `prepare_a1a2`.

### Behind the gate

The fixture matches the brief:

- the same fresh hosted/root/umask gates as A1/A2 and A7;
- A5 `preflight`, which requires Standard hardening, root identity and the
  supervisor's sole PID;
- a literal `runs` Missing check before `prepare_a1a2`, then a check that the
  new `runs` is empty;
- `Runner::new` (validation only, no child), then a startup check of at most
  15 s, then `READY=1`, then the 5 s fixture-local ACK, all before any Runner
  child;
- fixed targets only: `/usr/bin/true` with no arguments and `/usr/bin/sleep 15`,
  with a 15 s timeout.

Budgets fit inside the unchanged packaged limits (30 s startup, 120 s monitor,
140 s runtime): 15 s preparation + 5 s ACK + 2 × 25 s calls is at most 70 s,
under the 90 s total assertion.

### Cancellation boundary: the populated-leaf proof is genuinely post-spawn

The proof requires all of the following:

- exactly one `runs` child named `arx-<test pid>-<digits>`;
- the same dev/ino on a held directory before and a reopened one after reading
  `cgroup.procs` and `cgroup.events`;
- exactly one nonzero PID;
- `populated 1`.

In `unix::run`, the last `recheck` (cancel/shutdown) happens immediately
before `command.spawn()`. The child writes `0` to the leaf's `cgroup.procs`
only inside `pre_exec`, after fork. So any PID in the leaf means the parent is
already inside or past `spawn()`. A cancel sent after the proof therefore
cannot produce `NotStarted(Cancelled)`.

`accepts` also requires `Ok(Cancelled)`. Because `pre_exec` forces Rust's
fork/exec path, `spawn()` returning `Ok` means `exec` succeeded. It further
requires:

- a `None` exit code (cgroup.kill SIGKILL);
- the child-attested UID/GID with `daemon_was_root`;
- `LinuxCgroupV2`;
- `tree_cleanup_confirmed` (kill, `populated 0`, rmdir and reap all succeeded).

Together, the proof and the result show cancellation of a task that was
started and had exec'd. They do not show only a token flip. The proof's
`owned: true` is a label: ownership actually comes from the exclusive fresh
empty `runs`, the root-only 0700 leaf, the exact prefix and the refusal of
unknown or multiple children (advisory A2).

### Normal case

Normal makes no sample, which is correct. `Ok(Exited, Some(0))` with
attestation and confirmed cleanup is enough on its own to show the child
started and finished, even when the leaf is too short-lived to sample. After
the join, `a5::empty` checks the whole `runs` (tasks, recursive `populated 0`,
no child directories). That proves the brief Normal leaf is gone even though
it was never named.

### No fake credit

- Postconditions run only after the explicit `worker.join()`.
- They use the real `Runner::cleanup_residue()` flag. `Group::drop` latches
  this flag inside the worker, before the join returns.
- They require an empty `runs` and literal no-follow `ENOENT` for the held
  observed leaf name.
- The fixture contains no rmdir, kill, cgroup write or janitor call after
  setup, so nothing can substitute for Runner cleanup.
- Wrong reason/exit/identity/boundary, cleanup false, residue true, any
  `RunError`, missing proof, failed join, leftover leaf or task, or more than
  25 s elapsed all reject.

### Errors, panics and stalls

- Cancel is always signalled before the join.
- Observation `Err` and panics are caught by `catch_unwind` and become a
  refused proof, which still cancels and joins.
- A worker panic gives `joined=false`, which rejects.
- The scoped join keeps ownership through a synchronous OS stall. Elapsed
  limits only reject late results.
- The code comment and the evidence both state that these limits are not a
  hard OS interruption guarantee. The outer hard bound is the unchanged
  `RuntimeMaxSec=140`.

### Portable tests

There are three portable tests: counterfactuals, mock owned-call, and fixed
targets with the source gate. They use only:

- `accepts`;
- `Case::spec`;
- `exercise` with mock closures, a `Cancellation` and atomics;
- `include_str!`, which reads at compile time.

They make no Runner, LinuxFs or host read. The mock worker requires a cancel
within 1 s or it panics. So removing the cancel, or joining before cancelling,
would fail the fault-0 acceptance. `observed` proves each fault case actually
sampled. The postcondition closure asserts phase 2, so it only runs after the
worker finished.

A fourth, Linux-only, non-ignored test, `a12_delivery_closed`, calls only
`prepared_delivery`. The old SDK consumer join is not duplicated.

## Security: PASS, no blockers

- Test-only module with no production surface, MCP, route, secret or owner
  data. The native path is unreachable while delivery is `None`.
- `unsafe` is limited to `getuid`/`geteuid`/`umask` and one `fstatat`. That
  `fstatat` gets a valid NUL-free `CString`, a held runs directory fd, and
  `AT_SYMLINK_NOFOLLOW`. The `MaybeUninit` is never read.
- Leaf and runs opens go through `trusted_root` (root-owned, no 022 on every
  ancestor, `O_NOFOLLOW`) and LinuxFs magic/owner trust. Listings are bounded
  (512), reads are bounded (4096) and entries are classified, with refusal by
  default.
- The ACK uses `O_NOFOLLOW|O_NONBLOCK` and requires root 0600, nlink 1 and the
  2-byte value `1`.
- No A7 sudo, profile, view, bind or grant is borrowed. The comment forbids
  consuming A7's attestation.

## Compile and verification evidence

I did not repeat the builder's builds. After the call-graph audit I ran only
these pure checks, with TEMP/TMP and outputs on D:.

- `python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py
  -k test_pinned_manifest_applicability_in_memory_only`: 1 test OK (0.048 s).
  In-memory only, no subprocess or native entry.
- `cargo test --locked --offline -p arxd --lib --target-dir
  D:/Code/arxcovo-a12-source/target exec::a12::a12_owned_call_mock_counterfactuals
  -- --exact`: 1 passed, 0 failed, 411 filtered out (0.04 s). It reused the
  builder's target directory, so this was an incremental build of about 21 s.
- Standalone lint probe, not repository code: with Rust 1.95.0, `rustc` and
  `clippy-driver --edition 2024 --test -D warnings` accept the same two
  patterns the Linux module uses:
  - a private `struct Prepared` that is only ever returned through
    `Option` (`-> Option<Prepared> { None }`) and never constructed;
  - a tail expression `(unsafe { .. }) < 0 && ..`.

  So neither should fail Ubuntu `-D warnings` from `dead_code` or
  `unused_parens`.

The builder reports that the four exact tests, fmt, Windows locked offline
Clippy and S2b applicability all passed. I accept that as reported and did not
rerun those beyond the two checks above.

The Linux type-check was **not** run. `x86_64-unknown-linux-gnu` std is
installed, but `rusqlite` bundled needs a Linux C cross toolchain, and
installing one is out of scope.

Instead I audited every name the native module uses against its real
definition:

- module paths and visibility (`hierarchy::linux` and `fake::a5` are
  `pub(crate)`, `unix::trusted_root` is `pub(super)` from `exec`);
- the `CgroupFs` trait imported into scope;
- signatures: `list`, `open_child(&[u8])`, `read(&str, usize)`, `empty`,
  `preflight`, `prepare_a1a2(&dyn Clock)`, `Notify::send_to(&OsStr)`;
- the `Runner: Sync` capture.

I found no mismatch. Linux compile is still **pending**. The first evidence
will be the required Ubuntu Clippy/nextest jobs on the PR. The non-required
S2b workflow will also run automatically, because both `exec/mod.rs` and
`tools/ci/s2b-native/**` are in its `pull_request` path filter. It runs the
existing S2b baselines and mutations as a non-root user. That is neither A12
evidence nor a dispatch.

## Advisories (non-blocking; optional, none required for publication)

1. Evidence completeness. The evidence file does not mention the Linux-only
   non-ignored `a12_delivery_closed` test, which will run in Ubuntu nextest. It
   also does not mention the automatic non-required S2b trigger. The
   coordinator can record both in PROGRESS or the PR body without a source
   change.
2. `Population.owned` is hard-coded `true` on the native path, so the
   `owned` term in `proven()` only has mock counterfactual coverage. The real
   ownership facts are listed under Cancellation boundary above. A later
   cleanup could derive the field from those facts or drop it.
3. `case == Case::Cancel || !proof` in `exercise`: `!proof` is unreachable,
   because Normal always has `proof=true`. Harmless.
4. The source-gate test orders a fixed list of host-touching calls. It does not
   prove that nothing else runs before the gate. Keep `prepared_delivery()` as
   the first statement, or tighten the test to assert that, when delivery work
   begins.
5. Delivery slice, which is separate. Before each Runner call, add a held
   delivery recheck of workspace/account/runs identity, as A7's
   `recheck_delivery` does, together with the exclusive stage/account/
   workspace/artifact pins the comment already lists.
6. `let _ = held.as_raw_fd();` is a no-op, because the borrow of `self.held`
   already keeps the fd open. Cosmetic.

## Verdict

**Eligible for publication as dormant source.** No blocking Standards, Spec or
Security finding. This eligibility comes with all of the following:

- Linux compile pending on the PR's required checks;
- A12 delivery `None` and unselected;
- native fixture not run;
- no A12, A7/A11, M2, full F/G or PA6 acceptance credit.

A future delivery source and any run grant need their own review and an
explicit grant. A12 remains **unverified on real hardware**. There is no new
A7 attempt or reset.
