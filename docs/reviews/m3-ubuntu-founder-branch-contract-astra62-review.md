# M3 Ubuntu founder observation contract review

**Revise before implementation.** The three final branches can be
observed in place. The worker API loses the guard number, the cleanup wording
changes more of the proofs than needed, and the proposed procfs reader cannot
establish its own admission condition. The corrections below fit the five files.

Independent static review by Astra 6.2 high under the owner stuck-review override.
Reviewed the sole brief added by a54a0661..b10ac552. The crates, CI workflow and
CI helpers have an empty diff from PR176 f1bf99bc at this checkout. Evidence is
source and retained reports only. No build, runtime, probe or CI check was run.

## Standards

- **Preserve evaluation and drops before implementation.** At the final match in found.rs,
  keep `Ok(r) if Instant::now() < deadline` exactly once. Mark InnerOnTime inside
  that arm. Handle late `Ok(_)` and elapsed `Err(_)` with the original refusal.
  A cfg(test) late-Ok arm before the original wildcard plus test-only marks in
  the other arms permits literal baseline erasure. Never bind the discarded leaf
  or take another business clock reading. Issue-permit Busy returns before this
  match and has no final tag. Keep deadline_exit and the existing 24-event buffer.
- Apply the same rule at consensus.rs:438. Distinguish on-time inner result,
  late ready payload, closed oneshot (Ok(Err(_))) and wait elapsed. Owner checks
  use `>= ||` and `< &&` short circuits. A combined "owner stopped at this check"
  is honest if the individual operand is unavailable. Re-reading now/is_closed
  can select a different answer. Keep responder, guard, permit and store owners
  in the same scopes, including explicit drops before the completion reply.
  Inspect borrowed results at the mapping site. Avoid extending lock guards
  across statements or await, including through new match temporaries.
- **Visibility and gates.** The module is consensus::issue::found::observation.
  Both ancestors are already visible to consensus. Restricted visibility through
  `pub(in crate::cluster::consensus)` suffices for the module and types crossing
  the seam. Keep fields and formatting private. Use only test-gated observation
  statements/items in shared code. The two proof modules inherit their test gate.
  Adding literal #[cfg(test)] inside them fails assert_fixture_quarantine's empty
  attribute allowlist for conditional predicates. Treat inherited gating as sufficient.
  The quarantine guard scans source text, including consensus.rs and found.rs.
  Keep the bindings, inventories and proof tokens. Do not add target predicates,
  alternate production functions or conditionally remove original match arms.
- Future admission needs an independent exact-diff clock/drop/erasure review,
  the coordinator's separate compile budget and meaningful checks for opt-in
  isolation, slot bounds/lanes, branch selection and primary-error precedence.
  Pure recorder checks can live under the test-only observation module. Avoid
  adding cases to the two proof files with exact test inventories.

## Spec

- **Carry the guard number across spawn before implementation.** In fresh_read the first and second
  founder guards run identical code. A bare Option<Arc<Slots>> plus a relative
  Edge cannot distinguish slots 20..33 from 34..47 after spawn. Read pending once
  at helper entry in the caller task. Return an optional typed handle containing
  the fixed Arc and an immutable lane. Clone it into the existing owner and
  blocking closure. Equivalently preselect absolute typed edges at capture.
  No lookup inside either worker. Take diagnostic timestamps only with Some.
  Capture only the initial found in owned_anchor_query_single_voter and
  anchored_preproof_single_voter and the final issue in f10_drop_bounded. Leave
  F6's capture and output intact (no slots). All other helpers see None. The
  owned_query_timeout_shutdown_retains_submitted_resources case gets markers only.
- **Close the caller attribution gap.** Freeze the selected final
  arm, original FoundError class, phase events and available worker slots on
  return, before cleanup. An on-time Unavailable needs the original return site.
  For example, found.rs takes `self.raft.as_ref().ok_or(...)?` *between*
  preflight and initialize. There is no pending phase or initialize reply on
  that exit. Use a single test-only caller reason for missing Raft there. Preserve
  initialize API NotAllowed/other/fatal and leader wait failure before mapping.
  In preflight distinguish owners poison/closed, missing store, mutex poison,
  closure result, blocking helper error and caller receive failure. In proposal
  distinguish submit error, forwarded/other inner reply, dropped responder and
  caller receive failure. In each guard retain linearizer forward/quorum/other,
  both ready-error layers, store result and actual caller decision. Record only
  distinctions available on the path taken. No second read of Raft or the store.
- **Optional worker detail.** For a pending phase, owner and
  blocking submission/entry, mutex acquisition and read completion bracket the
  outstanding work. A start without acquisition leaves lock wait or descheduling
  unresolved. Successful gate/send bookkeeping is optional. Use the proposed
  48 words as capacity, with unique single-writer typed indices for used edges.
  Several failure alternatives may share one terminal slot if mutually exclusive.
  Successive successful gates must not overwrite the same slot. A blocking helper
  error with no store reply cannot distinguish join failure in adapter.rs:26.
  Distinguish a send attempt from delivery. No adapter change or event framework.
- Keep the cutoff at the decision if claiming a worker preceded that decision.
  A capture after found returns is a later observation. Mark it accordingly.
  Release/Acquire makes each published word readable independently. It gives no
  common snapshot instant. Absent, racing, saturated or ambiguously ordered
  samples stay unknown. In the packed word, zero means absent and offset+1
  occupies bits 8..55. Clamp at 2^48-2 microseconds and flag saturation before
  conversion. Caller-selected branch evidence survives a missing edge.
- **Narrow the cleanup change to returned refusal.** Match the capture
  result where each proof currently unwraps. Move Ok(leaf) straight into the
  existing success flow. Keep the scalar snapshot for emission. On Err save it
  and the original Copy FoundError outside the 35s work future, then return
  from its body. Both proofs have already stored
  the node and neither has submitted its blocking/TLS work at that point.
  Use the existing common router clear/drop and shutdown sequence once. On this
  branch convert lock poison, the shutdown return/timeout and weak-store/log
  checks to bounded statuses and emit before failing on the saved found error.
  Preserve the absolute entry+40s limit. The query proof's handles(n) removes
  handles under a lock and discards them. It proves no join. No query owner was
  created on this branch. Keep normal handle cleanup on the original success path.
  Later panics, missing startup and work timeout retain their original scope.
- F10 must reach its original release/join, read drain and uninitialized
  assertions first. Discard the successful final leaf at the original statement
  boundary. On returned refusal keep the error, attempt the existing shutdown
  once inside bounded!'s 20s, emit its outcome and fail on found first.
  Outer cancellation can still preempt emission. No universal cleanup guarantee.

## Security and overlap

- **Remove the procfs trust requirement by omitting the reader.** The hosted Ubuntu job
  supplies same-job provenance. Fixed names, capped reads and checks on path components
  do not bind File::open to the checked objects or establish a stable proc mount.
  A boot digest and PID/start ticks read through that path cannot certify it.
  Reading three files at each endpoint also spreads a marker across different
  instants. No mount checks, filesystem trust exception or new dependency needed.
- Use std SystemTime in checked integer epoch units and
  local Instant duration at the two boundaries outside found, and at entry/exit
  of the fourth test body. Keep both endpoints in one fixed record per case.
  Associate it externally with the existing nextest case, Ubuntu job and attempt.
  A case label scoped to that single invocation is sufficient. PID alone would
  allow reuse. Do not export identifiers for the host, boot, or other processes.
  Never compare process-local Instant origins. If all four pairs are visible and
  unique, report the intersection on the *recorded wall clock* using the latest
  start and earliest end, conservatively accounting for quantization. Label it
  conditional on a common stable wall clock. With floor-to-unit timestamps,
  require max(start+1 unit) < min(end) for an interior intersection. Regression,
  wall/elapsed mismatch, insufficient margin or missing provenance leaves overlap unknown. Agreement
  cannot exclude wall-clock steps between samples. Verified monotonic cross-
  process overlap stays Unknown under this design. Even a wall intersection
  describes scope lifetimes only. No claim about CPU use or historical cause.
- Format only closed enum classes and bounded scalars into a fixed ASCII sink
  after timed founder work. Reserve room for truncation and the emitted newline.
  Put the primary tag/reason before optional rows. No payload Debug, raw errors,
  keys, paths or process inventory. Three 8192-byte records plus one 512-byte
  pair cap the new diagnostic output at 25088 bytes/job. Keep F6 output separate.

## Coordinator PROGRESS proposal

Record Astra 6.2 high static review, corrections pending. Use one new reviewed
revision on normal three-OS CI, Ubuntu full suite first attempt. This preserves
the original workload selection. Scheduling may differ. Keep workflow, filter,
jobs, budgets, concurrency, output and retries unchanged. Passing nextest output
can be hidden. A red can identify a branch with unknown overlap. A green yields
no reproduction. Preserve the first result and stop, including on compile failure
or missing evidence. No recovery rerun, F10 treatment or implementation grant.

PR176 f1bf99bc held, Ubuntu failed, Windows/macOS passed. Last merged PR177,
main 05b9c829 green. C invalid, T unrun. Sol shared fixes 3/3 exhausted,
historical Opus 0/2 unchanged. M2 open, M3 unchecked. Hardware paths
**unverified on real hardware**.
