# M3 durable member registry: implementation evidence

Sol high, 2026-11-15. Parent `4750ae1` on the enrollment/auth boundary; main reference
`0ab5038d89e86e5f726fbed7e5ef6d76e64fa306` (PR146). Source/test commit `a8118e96d5920fde6fa35e660f6265650fd2ebe2`.
The settled brief includes retained `2640629` and the coordinator's 16-byte ClusterId correction;
R1/R2 design review is closed. Independent Opus source review and R1 recheck now pass; exact-head publication review and hosted gates remain required.
No PR, push, hosted check, merge, watcher, delegation or admin operation was performed.

## Implemented boundary

- Found validates structure before current-pair replay, including after founding. It never allocates IDs.
  Empty, unordered, duplicate-node/key, out-of-range and 17..64-pair Found commands commit BadFounders.
  A current subset, or a subsequently enrolled pair, can replay; this does not attest the original bootstrap list.
- Mint/Redeem/Remove require Found after the unchanged common time/prune prefix. Fresh Redeem checks
  key uniqueness and the 16-member capacity before consumption/allocation, then inserts the binding atomically.
  Remove is idempotent, refuses unknown IDs and the last member, and preserves the monotonic allocator.
  A removed consumed-record replay returns Revoked without a grant. Record pruning does not prune bindings.
- FoundMembers owns a private Vec, exposes only as_slice, retains at most64 pairs and reads at most65
  iterator items (including a million-item counting-iterator test). Registry module is94 formatted lines.
- Tags4/5 leave tags0..3 and ClusterId unchanged. Independent full-row construction and recomputed SHA-256
  verify Found90+40n bytes (730 at16;2650 at64), Remove97, and sizeof ClusterId16. Counts0/1/16/17/64
  round-trip. Count65, missing pairs, truncations, trailing bytes and tag6 refuse as Corrupt.
- State image version2 appends founded/members after records. Semantic tests recompute digests and exercise
  empty-founded, unfounded records/members, duplicate keys/nodes, unsorted nodes, out-of-range IDs and
  consumed-current-member key disagreement. Independently built64-record/16-member/maximal actual membership
  image is13655 bytes, below16384. All truncations reject; install/reopen round-trips the complete bytes.
- A checksum-valid, independently built old v1 image gives Unsupported in peek/install/open; failed open
  leaves the SQLite file byte-identical. There is no migration or production v1 state. Mixed versions are
  unsupported. SQLite schema user_version remains1; no schema or dependencies changed.
- Pure authorize_peer reads local applied state only. Consensus local_authorize and registry_covers are
  test-only. An isolated node can accept a removed binding indefinitely until it applies Remove; there is
  no global freshness or physical-expiry guarantee. This helper is insufficient for isolated production
  execution/control. Registry removal does not change Raft membership or its quorum denominator.

## Commands and observed results

All cargo build/test/Clippy processes used these environment values:

```powershell
$env:TEMP='D:/Temp/arxcovo-registry-target-sol4750/temp'
$env:TMP=$env:TEMP
$env:CARGO_TARGET_DIR='D:/Temp/arxcovo-registry-target-sol4750/target'
```

No installations, dependency fetches or cache cleaning. Existing toolchain1.95.0,
OpenRaft0.10.0, rusqlite0.37.0 and tokio1.48.0 remain pinned by the unchanged lockfile.

| Command | Observed result |
| --- | --- |
| `cargo test -p arxd --lib cluster::enrollment --locked --offline` | Initial focused existing pure tests:14 passed. New pure tests also pass in the complete cluster runs. |
| `cargo test -p arxd --lib cluster:: --locked --offline -- --nocapture` | Historical builder result:107 passed,0 failed,2 ignored child entry points;8.77s test execution. Superseded final R1/main-integration result:109 passed (below). Both parent process-kill tests invoke their ignored owned child entry points. |
| `cargo test -p arxd --lib cluster::state_store::tests::maximum_v2_image_and_registry_semantics_with_recomputed_checksums --locked --offline` | After the equivalent lint correction:1 passed in1.42s. Complete cluster suite subsequently rerun green. |
| `cargo fmt --all -- --check` | Pass on final source. |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | Pass on final source;4.21s incremental. |
| `git diff --check` | Pass. |

Final cluster log: `D:/Temp/arxcovo-registry-target-sol4750/cluster-final.txt`.
Final Clippy log: `D:/Temp/arxcovo-registry-target-sol4750/clippy-final.txt`.
Full workspace tests were deliberately not repeated: the coordinator reported the existing unattributed
owner_windows8 AccessDenied failure. No admin remediation attempted. Hosted full CI and its15-minute budget
were not exercised here; local cluster test execution is below60s.

Earlier runs exposed routine fixture issues from adding Found (initial positions, shifted applied-index
corruption values, capacity/reused-key fixtures) and one same_item_push lint in the independent snapshot
builder. These were fixed; no test assertion or storage gate was disabled.

## Actual OpenRaft T4 proof and preserved fixtures

T4 initializes three real in-process OpenRaft voters with in-memory RPC and private SQLite stores. Before
Found, semantic-invalid commands including17/64 pairs commit BadFounders with the EnrollmentState unchanged.
After Found[1,2,3], empty/17/64 Found rejects, a subset replays and a conflicting key gives AlreadyFounded.
Mint/Redeem registers node4 only in the registry (no Raft voter/learner addition); all three authorize it.
The test stops a follower, commits Remove4 plus40 UnknownCode entries, waits for purge beyond the stopped
position, restarts from the same directory, and proves full_snapshot RPC delivery to that follower.
The caught-up follower refuses4 and accepts all founders before advancing time past pruning. All nodes
then have zero records, matching persistent registry/state bytes and registry_covers for the initial voters.
All three stop/reopen and converge again, exercising decode of committed semantic-invalid Found entries.

Final measured T4: follower2, stopped applied15, leader purged55, snapshots to follower1,
elapsed3.8551662s; RPC mismatch0, max append6, max snapshot465 bytes.
T3 independently remains green: follower2, stopped applied3, leader purged39, snapshots1,
final applied44, elapsed2.832061s. T1/T2 and U6 remain green (U6 restored64/64 permits).

Existing state-store fixtures insert Found at index0 and shift later positions; the malformed membership
index test still exceeds applied, and the pending-cap decoder test now has a valid eight-record control.
Log fixtures preserve the original membership, Mint/Redeem and pending-record sequence after prepending
Found; crash split/truncate/resume positions shift together. Both original four-kill groups retain hot
journal header checks, exact pre/post recovery, replay and next-index assertions. Reusable retained-cap
fixtures use distinct synthetic public keys and explicit removals; first-code replay coverage is retained.
Existing privacy scans cover the registry-bearing rows, debug, snapshots and private SQLite files with
positive marker controls. No owner data or real credentials are used.

## Limits and next gate

This is in-process registry/storage evidence, **unverified on real hardware**. No CA, private key,
signature, TLS, ingress, listener, production authorization, membership workflow or owner operation exists
in this slice. Certificate candidate pins in the brief are not dependencies. Existing shutdown-release
and ReadIndex apply-catch-up advisories remain assigned to later production/certificate wiring.
M2 remains open; A7 FINAL NO-GO and A12 consumed/cleanup-uncertain no-retry are unchanged. Full M3 acceptance
(three processes/hosts and an agent build from A on B) remains unmet. Coordinator owns PROGRESS, fresh
Opus review and any future PR/gates; this implementation does not waive them.

## R1 isolation and main integration

The original rejection-coverage statement was overstated: three checks were masked by other refusals. Test-only correction `7a37a68` adds31 lines across the two test files. Found count cap now has allocated, sorted, unique-keyed17/64-pair cases before and after founding, each requiring BadFounders and unchanged state. The maximum-image duplicate-member-key case also updates record49's consumed key; the last-member range case preserves ordering while setting that member to next_node_id74.

Independent Opus recheck `b1efd7b` at `5b451df` weakened each guard alone. Each mutant compiled and produced108 passed/1 assertion failure at its new case; the old masked cases still passed. After restoring exact hashes,109 cluster tests passed again (8.51s; fresh pre-mutation run8.86s), including T4/T3 and both crash groups. Fmt, locked/offline workspace Clippy with warnings denied and diff checks passed. The109 count is107 plus the isolated Found test and PR147's consensus regression; the state-store cases extend an existing test. The earlier unobserved root run is not credited.

Main integration preserves every registry production blob from `e617200`; PR147 supplies only the independent consensus append hotfix. This remains in-process evidence, **unverified on real hardware**, and gives no full M3 acceptance.

## Source/test blob pins

| Path | Git blob after R1 (other eight unchanged) |
| --- | --- |
| `crates/arxd/src/cluster/consensus.rs` | `d331442ec2d69433d0293bc3e97b48b66ea95ab6` |
| `crates/arxd/src/cluster/consensus/three_node.rs` | `9999af2fba30e3b5af9f31a65911e84d1287748b` |
| `crates/arxd/src/cluster/enrollment.rs` | `d26e28b45b277f1b6379bf3882e8de4f636f0c4b` |
| `crates/arxd/src/cluster/enrollment/codec.rs` | `ecaeb9ffa0e614bb99da015ea4ee485b621e674b` |
| `crates/arxd/src/cluster/enrollment/registry.rs` | `3764f4d5214fa90fb67d36ec5b02ec1b7b6b8727` |
| `crates/arxd/src/cluster/enrollment/tests.rs` | `762505ee071937758f1caabaf324a57185a941b7` |
| `crates/arxd/src/cluster/log_store.rs` | `02db51fc2409201f7f84c731707329d71398727c` |
| `crates/arxd/src/cluster/log_store/tests.rs` | `f0072dd177289361c6c1b85da2a1278d1f924b93` |
| `crates/arxd/src/cluster/state_store.rs` | `90474881230e40061648efcedd0a51dd66eab54e` |
| `crates/arxd/src/cluster/state_store/tests.rs` | `1bc25e51483fa1144b691b7da72b4e6746bf5591` |
