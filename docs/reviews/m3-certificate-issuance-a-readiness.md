# M3 PR A certificate issuance: readiness check

2026-11-16. Anthropic / Claude Opus 5.5 / high, research sub-agent, no children. Docs only. Base `4ca2b96` (pending PR149 A0 head over main `a18e95c`).
Read: `docs/briefs/m3-certificate-issuance-first-slice.md`, its Sol review and rechecks, `m3-bounded-read-resumption.md` and its Opus delta review, and actual
`consensus.rs`, `consensus/three_node.rs`, `consensus/adapter.rs`, `enrollment.rs`, `enrollment/code.rs`, `enrollment/registry.rs`, the quarantine guard, the sampler
filter and `ci.yml`. No source, brief, PROGRESS or ADR edit. No build, resolve, native, VM, admin, CI, push, PR, status, merge or watch.

**Verdict: ready once the coordinator applies the edits E1–E6 below. No redesign.** The settled scope is unchanged: test-only in-memory CA, CSR proof of possession with
exact algorithm pairs and the original SPKI, leader-own commit with fresh pre/post guards, 30-day leaf under a 31-day test CA, private `IssueTicket` in trusted
`issue.rs`, and no TLS stack, endpoint, sealed CA or production transport. Three findings are real contract/API conflicts (F1–F3). The rest are precision gaps that
Sol would otherwise have to decide alone. **Source work starts only after:** PR149 merges, exact-main CI on its merge SHA is green, and the root Sol check accepts E1–E6.

## Verified against current source

- `fresh_read` (`consensus.rs:389`) returns `(LeaderId, T)`, where `LeaderId` is `leader_id_adv::LeaderId<u64, u64>`. That is the type of `ClientWriteResponse.log_id.leader_id`
  (`pos()` at `:27` already reads `.leader_id.node_id`), so `commit.leader == read leader` needs no conversion. Admission uses `try_acquire_owned` (8). The unit is owned
  in `Owners`, and the permit drops inside the blocking closure.
- `propose` (`:355`): an owned task holds one of the 64 permits until the engine reply. The caller's `rx.await` has no deadline. A dropped caller loses only the send.
  The `propose_response` delegation fits without changing `propose`'s signature.
- `SignerClock`, `RedeemInput`, `IssuedLeaf`, `check_grant` and `ISSUE_PAUSE` do not exist yet. `RedeemProof` is `Clone`, its Debug is `<redacted>`, and it is not
  zeroized. `Command::redeem` takes `&RedeemProof` and keeps only the `CodeVerifier`.
- `Cargo.lock` is unchanged since the brief's base `6e9edd5` (sha256 `7941da2fae68…`, 243 packages), and PR149 adds no dependency. The 38-package allowance and pins
  therefore still refer to the current lock. I did not re-resolve them.
- `Cluster`, `new`, `node`, `router`, `diag`, `leader`, `write`, `converge`, `attributed_read`, `finish`, `attribute_stop`, `unknown` and `report_*` are **already
  `pub(super)`** on main (PR153 D1). Still private: `APPLY_PAUSE_OWNER`, `code`, `mint`, `founders`, `start`, `stop` and `unfounded`.
- The guard (`tools/ci/test_windows_cluster_quarantine.py`) reads **only** `three_node.rs`. The sampler FILTER is `test(/^cluster::consensus::three_node::/)`. The
  required step is `cargo nextest run --workspace --locked`, with no filter or retry profile (the repo has no `.config/nextest.toml`).

## Findings

**F1, API conflict: where `enroll` lives.** The brief's "Changed files" puts `enroll` and `ISSUE_PAUSE` in `consensus.rs`. Decision 2 says `committed` and the
construction of `IssueTicket` exist only in `issue.rs`, and that `issue.rs` takes responses "only from its own `propose_response` call". A parent module cannot call a
child's private function, and making `committed` `pub(super)` would widen the trusted surface to all of `consensus`. Private items of `consensus` (`fresh_read`,
`propose_response`, `id`, `raft`) are visible to its child, so `enroll` belongs in `issue.rs`.

**F2, test-contract conflict: the `ISSUE_PAUSE` key.** Node IDs 1–3 repeat in every `Cluster`. The required in-process run (`cargo test -p arxd --lib cluster::`)
runs I1–I9 concurrently in one process, so a pause that I5 arms for "node 1" can capture I1's enroll on a different cluster: a hang or a false result. A
`std::sync::mpsc` blocking `recv`, which is how `APPLY_PAUSE` releases, would also park a Tokio worker if used inside async `enroll`. `APPLY_PAUSE_OWNER` is private,
so issuance tests cannot honor the A0 rule that "any future issuance guard test" holds it.

**F3, Windows coverage gap.** I1–I9 add nine `Cluster` fixtures to the Windows pool. They use the same fixture and D1 helpers (`leader`/`write`/`converge`/`stop`) as
the quarantined eight, which undoes the resumption brief's "R1–R5 are the only three-node tests in the Windows pool" condition. None of them is in, or covered by, the
existing 8. The guard does not read the new files, so an `ignore`, `cfg(windows)` or `cfg_attr(windows, …)` in `issue_tests.rs` or `pki/tests.rs` would pass
silently. Gating them openly is the only honest option, as it was for R1–R5.

**F4, precision: cancellation ownership.** `MAX_ISSUING` bounds concurrent caller-side parsing, signing and live DER. It does not bound abandoned engine work: when
`ISSUE_BUDGET` cancels, the caller's future and its issue permit drop at once. Only its proposal unit (at most 64) and read units (at most 8) outlive it, each under its
existing owner and permit, and repeated timeouts show up as `Busy`, not as leaks. Two outcomes are unmapped: an elapsed budget, and `ProposeError`. CSR parsing and
signing are synchronous and run inline, so the budget can preempt them only at await points. A `spawn_blocking` would put an await between `pre_now` and the sign step.

**F5, precision: secret lifetime, `Send` and refusal mapping.** `RedeemInput` is not defined anywhere. `&dyn SignerClock` makes the `enroll` future `!Send` unless the
trait is `Sync`. Neither `check_grant`'s error type nor the mapping from `authorize_peer` refusals to `IssueError` is specified, but I5 relies on `Revoked`.

**F6, precision: I6 determinism.** "Advance the clock while guard 1 waits on `APPLY_PAUSE`" names no observable signal. A bug that samples time after the AfterCommit
release but before the advance would pass.

**F7, CI and file allowlist are stale.** The CI section still quotes 585/716 s, Clippy 116 s, nextest 307 s and a 199 s Playwright cache example. The measured state is
exact-main `a18e95c` CI43836099670: Windows job133192981010 took 433 s, with nextest 239 s and Playwright install 6 s after C1
(`docs/evidence/m3-windows-hosted-diagnostic-sample.md`). A's compile cost (ring, rcgen, x509-parser, asn1-rs, time; there is no cargo cache) and the I-test time
are unmeasured. `Cluster` is already `pub(super)`.

## Mandatory brief edits (exact text, for the coordinator to apply; I did not edit the brief)

- **E1 (F1).** Replace the `consensus.rs` part of "Changed files" with: "`consensus.rs` (`mod issue;` and a private `propose_response` that `propose` delegates to,
  with `propose`'s signature unchanged). `enroll`, `ISSUE_PAUSE`, `committed` and `IssueTicket` live in `consensus/issue.rs` as an `impl ConsensusNode` block. No item
  in `consensus.rs` constructs a ticket or calls `committed`. `issue_tests.rs` is a child module of `issue`, so I7 can call the private `committed`."
- **E2 (F2).** Append to the ISSUE_PAUSE text: "The test-only `ISSUE_PAUSE` is keyed by the node's store pointer (`Arc::as_ptr(&sm.0) as usize`, as `APPLY_PAUSE`
  is), not by node ID. It releases through a `tokio::sync::oneshot` and never blocks a runtime worker. It compiles out of non-test builds. `APPLY_PAUSE_OWNER` becomes
  `pub(super)`, and every test that arms `APPLY_PAUSE` or `ISSUE_PAUSE` holds it for the hook's whole lifetime. This is a test-correctness lock for shared static
  slots, not a scheduling change. Unit test: a pause armed for one node instance does not fire for a second instance with the same ID."
- **E3 (F3).** Add to engine tests: "I1–I9 are ordinary required tests on all three OS. No `ignore`, platform `cfg`, filter, test group, retry or timeout change is
  added, and none of them is covered by the existing quarantine of 8. Their module path stays outside `^cluster::consensus::three_node::`, so the sampler's exact-8
  inventory is unchanged. The quarantine guard gains one assertion: `issue_tests.rs` and `pki/tests.rs` contain no `ignore`, `cfg(target_os`, `cfg(windows)` or
  `cfg_attr(windows`. A mutation that inserts an `ignore` must fail the guard. A red follows the resumption brief's pre-registered red handling: no rerun, no
  quarantine, no deadline or permit change. A green gives no Windows stability credit."
- **E4 (F4).** Append to decision 6: "Order: absolute `deadline = now + ISSUE_BUDGET`, then `try_acquire` on the issue permit, then parse. The permit is held by
  the caller's future, and `enroll` spawns no task. Only its proposal unit and read units outlive a cancellation, under their existing owners and permits.
  `ShutdownReport` and its cap of 72 are unchanged. An elapsed budget gives `Unavailable`. `ProposeError::Busy` gives `Busy`, `NotLeader(l)` gives `NotLeader(l)`,
  and `Stopped`/`Unknown` give `Unavailable`. CSR parsing and signing run synchronously inline: at most 4096 B, one verify, one P-256 sign and one `getrandom` call.
  The budget is responsiveness at await points, not preemption. I4 also asserts that the issue permits are all free as soon as the old leader returns, and that its
  read units return within 2 s."
- **E5 (F5).** Append to decision 6: "`RedeemInput { cluster, request, code: CodeId, proof: RedeemProof }` derives no `Clone` and is consumed by value. The proof
  is dropped right after `Command::redeem`, before the first await. It is never captured by guard closures, pause hooks, errors or `IssuedLeaf`. Dropping is not
  erasure: this slice adds no zeroize. `trait SignerClock: Send + Sync`. A compile-time test asserts that the `enroll` future is `Send`. `check_grant` returns
  `Result<(), Reject>` using existing variants only (`UnknownCode`, `Consumed`, `KeyConflict`, `WrongCluster`). `authorize_peer`'s `NotMember`/`KeyMismatch` gives
  `Revoked`, and its `WrongCluster` gives `Rejected(WrongCluster)`."
- **E6 (F6, F7).** In I6: "Arm `APPLY_PAUSE` on the leader's store and write W. Release AfterCommit only after `entered` fires (W is committed). Advance the clock
  only once the leader's `read_permits.available_permits() == READ_PERMITS - 1`. Release the pause before guard 1's deadline." In the file list: "`three_node.rs`:
  only `APPLY_PAUSE_OWNER`, `code`, `mint` and `founders` become `pub(super)` (`Cluster` and its D1 helpers already are), plus a per-cluster extra-marker list,
  empty by default, that `finish` scans as raw bytes and hex with the same positive control. Existing markers, D1 attribution, helper deadlines and `Params` are
  unchanged." Replace the first CI sentence with: "Baseline: post-PR149 exact-main Windows `verify` time. Prior reference: `a18e95c` CI43836099670, 433 s, nextest
  239 s, Playwright install 6 s after C1. The earlier Playwright 199 s figure and its caching example are stale. A's added compile and test time stay unmeasured until
  its PR run. The 660 s rule is unchanged, and a ci-speed PR must name a measured component. This edit adds no new optimization, pin or version."

## Status and next action

- Not claimed: no built or run check, no hosted result, no dependency re-verification. The quarantine of 8 is unchanged. M2 stays open (A7 FINAL NO-GO, A12 with no
  retry), M3 stays unchecked, and every hardware path stays **unverified on real hardware**. Advisory 1 stays open.
- **Next:** after PR149 merges and exact-main CI on its merge SHA is green, the coordinator applies E1–E6 to the brief and the root Sol check reviews that small
  delta. Then Sol high implements PR A within this allowlist (which adds the guard test file per E3), and Opus reviews it.
