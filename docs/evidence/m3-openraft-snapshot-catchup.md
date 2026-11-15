# M3 OpenRaft T3 snapshot catch-up

Status: local test-only proof passed; fresh independent Opus review and hosted
gates remain required. Sol high, 2026-11-15. Parent
`3fb24d280d60a8a83e6e34e8493c2b6c03932b84`; main
`38750cf1e179e14da6fa871972027b6d66028ec9` (PR #144).
T1/T2 source remains separately pending independent review. This follow-up is
for separate publication after the parent review, gates and merge; no acceptance
or publication is inferred from these local checks.

## Retained test

Only `cluster/consensus/three_node.rs`, minimal test-router counter instrumentation
in `cluster/consensus/mem_net.rs`, and this evidence changed. Production consensus,
adapters, stores, dependencies, lockfile, workflows, gates, ADRs and PROGRESS are
unchanged. The coordinator records this Sol/high outcome in PROGRESS separately.

T3 uses actual OpenRaft over three persistent local SQLite directories and the
existing bounded in-memory RPC double. Params 8 snapshot / 0 keep / 1 purge / 16
lag pass the existing config validator. Initialization commits membership; a
leader is discovered, an initial unknown-code command commits, and all nodes
converge before stopping and removing a follower. Its persisted directory is
reused without altering mandatory `ConsensusNode::start` preflight.

With that follower stopped, exactly 40 commands commit with `UnknownCode`
outcomes. `Raft::wait().metrics(...)` requires the actual leader's purged index
to pass the stopped follower's applied index before restart. No fixed sleep,
direct snapshot install, synthetic counter or manufactured outcome is used.
The destination counter increments inside `full_snapshot`, after sender,
availability and payload checks, immediately before invoking the target's real
`install_full_snapshot`. T3 requires at least one such RPC to this follower.
Successful catch-up also requires exact applied positions and byte-identical SM
blobs across all three nodes. A further command must occupy the consecutive
index and all three must converge again.

Fixture shutdown removes routing before joining owned work; it checks both store
Weak references released, 64 permits restored, empty proposal owners and stopped
proposal refusal. Post-shutdown files are scanned for deterministic synthetic
secret/proof/owner-transfer markers in raw and hex forms. Each scanner has a
positive control with the corresponding marker embedded between prefix/suffix.
RPC bounds remain append <=64, snapshot <=16384 bytes and sender mismatches zero.
T3 has a 45-second outer deadline and five-second condition waits.

## Actual local checks

Every build used `--locked --offline`, with no dependency install or deletion.
`CARGO_TARGET_DIR=D:/Temp/arxcovo-openraft-t3-target` and
`TEMP=TMP=D:/Temp/arxcovo-openraft-t3-target/tmp` isolated this work from other
builders and reviewers. No broad owner fixture suite ran.

- Focused `cargo test -p arxd --lib --locked --offline
  cluster::consensus::three_node::t3_snapshot_catchup_then_consecutive_append --
  --exact --nocapture`: 1 passed, 1.79 s execution; fresh target build 1m10s.
  Stopped follower 2 applied=2, leader purged=39, snapshot RPCs to follower=1,
  snapshot maximum=198 bytes, append maximum=17, sender mismatches=0;
  final applied=43. Fixture timer including shutdown/scans: 1.7824 s.
- `cargo test -p arxd --lib --locked --offline
  cluster::consensus::three_node -- --nocapture`: all 4 passed, 7.81 s execution,
  0.34 s build. T3 fixture=1.8208537 s; same positions and one 198-byte snapshot,
  append maximum=9, sender mismatches=0. T1/T2/U6 remained green; U6 restored all
  64 owners. All fixtures' largest append was 64, mismatches zero.
- After restoring the counterfactual below, `cargo nextest run -p arxd --locked
  --offline cluster::`: **99 passed**, 456 skipped, 10.133 s execution, 36.16 s
  rebuild. T3=3.367 s, T1=1.149 s, T2=8.101 s, U6=2.522 s. Existing 36 cluster
  tests and both four-process-kill probes (eight probes total) passed.
- `cargo fmt --check` and `git diff --check`: passed.
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`:
  passed, 57.56 s including build-directory lock wait and fresh check artifacts.

Local logs: `three-node.txt`, `counterfactual.txt`, `cluster-final.txt` and
`clippy-final.txt` under the dedicated D: target. The focused initial result is
recorded above from tool output. No snapshot or preflight production bug was
observed, and no production fix was attempted.

## Counterfactual

Temporarily inserted `if self.target > 0 { return
Err(StreamingError::Unreachable(unreachable())); }` inside `full_snapshot`,
before routing/counters/installation. Every valid fixture destination is nonzero.
The exact focused T3 command compiled, then failed (exit 101) in 5.80 s on the
real follower convergence condition: `applied_index>=42`, follower last log and
applied stayed at 2 while cluster committed reached 42. This is an actual lost
snapshot route, not an assertion or counter mutation. The original router was
restored in `finally`; the subsequent 99-test green run uses restored source.
No counterfactual code is retained.

## Limits

This is in-process graceful follower restart and local persistence evidence.
It proves neither separate hosts/process transport nor TCP, mTLS, CA, signing,
physical expiry, power loss, nor full KICKOFF M3 acceptance. Those paths remain
**unverified on real hardware**. Local cluster execution was below 60 seconds
and T3 below the typical 20-second target; hosted OS results and full CI under
15 minutes remain unverified. No push, PR, merge, watch, subagent, native M2,
admin, VM, Lab Manager, dispatch, repository-variable or owner-data action occurred.
M2 remains open: A7 FINAL NO-GO; A12 consumed with cleanup uncertain, no retry.
