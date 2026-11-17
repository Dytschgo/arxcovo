# Atomic join-anchor contract: independent Sol recheck

Sol 6.1 high, 2026-11-17; single independent reviewer, no children.
Reviewed prepared ffab93c875ac9fd77a8ab54c15da2c8e9e9d6eda, parent
5fb5740464db5e43b12a6b6609310d10b2f210c7. Source baseline
380c0215d662f75c083015658a9f6f261808f2ea; local diff confirms no source
changes from it. Proposal 2462bf33 retained as 02f3a525 is historical;
the prepared brief plus normative root corrections are the reviewed contract.

## Standards

Blocking: none. Read AGENTS.md, KICKOFF.md, CONTRIBUTING.md and the
dytschgo-review skill/profile. Current owner routing supersedes the historical
cross-provider wording in CONTRIBUTING. This review grants no new attempts,
source authority or merge permission.

The bounded integrated slice fits milestone 3. Shared membership-v1 remains
unchanged/std-only. Root's explicit inventory-guard exception supersedes the
brief's general guard freeze only for the required single-voter class and its
negative controls. Add the mandated ADR0005 amendment before implementation;
record the schema break and effective capacity55 there. No migration/reset,
production ingress or unrelated genesis repair belongs in this slice.

## Spec

Blocking: none after applying root R1/R2/R3 as normative.

- R1 preserves existing semantics: enrollment.rs currently assigns effective
  operation time to Grant.applied_ms and check_issuable uses that field first.
  Separate retained original_applied_ms is feasible. At first1000/replay60000,
  signer20000 gives 50000 < 60000: require SignerClockBehind BEFORE asserting
  the fresh field. Choose an otherwise live code and valid binding/proof;
  original1000 must survive replay. The fourth pure mutant assigning origin
  time to fresh applied_ms then misses this exact refusal, rather than merely
  failing a new provenance-field assertion. Keep the original<=fresh<=committed
  check; it must not require fresh replay time to equal original time.
- R2 is feasible through tools/ci/test_windows_cluster_quarantine.py's existing
  assert_fixture_quarantine(required=). ISSUANCE_REQUIRED must contain exactly
  anchored_single_voter_receipt_replay_and_revocation; ISSUANCE11/reasons stay
  intact. Require omission/rename/ignore/Unix-gating and inventory/old-ungating
  negative controls. Inspect body AND helpers for one actual start, founder_init,
  found, enroll and no three-node Cluster dependency; do not ban ClusterCa by
  substring. Existing Params, MemRouter, CSR and AfterCommit hook suffice.
  Keep fixture28/sample8/founder13/TLS14 classifications unchanged.
- The entry deadline includes startup and a 5s cleanup reserve within40s;
  existing operation/service caps stay5s. Resolve/drop caller futures, release
  hooks, remove router engine handles, and require clean shutdown before pass.
  ConsensusNode::start awaits spawn_blocking: timeout cannot stop that work.
  Pending startup must fail with cleanup unresolved, possibly exceeding the
  cooperative deadline during runtime teardown, as root explicitly states.
- R3 can use genuine grants and fabricated ClientWriteResponse values at the
  private committed adapter in its child test module. Preserve own-response
  leader identity and Enrolled shape so the new check is reached. Independently
  test origin index greater than L, origin term greater with index less than L,
  and equal-index full-position mismatch; retain a valid older-origin control.
  Map malformed origins to Rejected(UnknownCode). Keep L for issuer guards;
  identity_commit is original redeem E, or FounderTicket's committed position
  reconstructed from its existing leader/index for found(). No signing bypass
  or new constructor is needed.

Advisory A1: 4096 is the membership envelope bound, not reachable canonical
membership size. The shared codec maximum is3605. Keep exact/over4096 envelope
and4163 row-bound checks distinct from canonical success cases. Malformed4096
membership must still refuse. Routine test construction resolves this.

Advisory A2: preserve old semantic assertions through explicit setup/offset
adaptation, including replay-before-full, expired-but-retained replay, privacy,
SQL unknown outcomes and revocation. Apply the55 reservation only to NEW Mint
after pruning and dedup; boundary Mint replay must succeed. A56th pending record
still fits raw bytes, so removing the reservation has a reachable StateFull
counterexample without relying on a storage failure.

## Security

Blocking: none in the internal kernel contract. code.rs already reduces proof
before proposal; v2 proof adds anchor to that derivation. Repairing a changed
anchor's checksum cannot preserve its verifier. EnrollmentState can hash stored
voter digests using shared Membership accessors without modifying that codec.
StateStore clones Image and persists before returning Outcome; its adapter
already supplies full committed LogPos under the store lock. Decode E-1
membership there and compare before enrollment time/pruning/consumption mutation.
AnchorChanged may advance applied E only. Direct trusted proposals need the
same fence. Exact retained replay authenticates original fields and bypasses
new-consumption fencing after membership drift; binding removal still revokes.

Image v3, new SQLite version2, metadata v2, ARXLE2 and tag6 are intentional
refusal boundaries, with explicit Unsupported for legacy versions/tags. Existing
transactional open paths support refusal before application schema/row/version
changes. Preserve files; journal/lock mechanics are not byte-identity promises.
Snapshot decode must enforce full origin bounds/equality, original-time bounds,
registry consistency and the reservation invariant; legacy install/log replay
need direct refusal evidence. Storage errors remain unknown/poisoned outcomes.

Scratch math confirms fixed4866 +55*209 =16361, +56*209 =16570;
canonical55 image15870, code270 characters, maximum hash input679 bytes.
The79-byte synthetic vector independently hashes to
71fd16b1bafc577123cfd694b1d196cd280c4f42b8f36e2d8e99f3590f708b01.
All four designated mutations are reachable within allowed pure/durable source
seams: A/B first-consumption fence, full-E replay persistence, NEW Mint reserve,
and fresh-time rollback refusal. Each needs its named targeted assertion,
once-only run and exact baseline restoration; compilation failure, survival or
unrelated red earns no kill credit. This is feasibility, not executed evidence.

## Evidence and disposition

Actual: local pinned-ref/status/diff checks; reads of the contract and applicable
instructions, enrollment/code/codec/registry APIs, state/log persistence,
membership codec, consensus adapter/issuance/founder and relevant tests/guard;
synthetic .NET SHA256 and integer arithmetic. Report encoding/whitespace/scope
checked locally. No Cargo/build/test suite, engine, network, remote status, PR,
admin, VM or cache deletion ran. No runtime or mutation pass is claimed.

Reused root evidence only: exact-main CI44097456394 attempt1 green at source
380c0215, wall556s, Windows551s, all11 shared-codec cases on all OS. This does
not test the proposed anchor behavior. M2 open/M3 unchecked; Sol3/3 historical
Opus0/2 unchanged; hardware **unverified on real hardware**.

Ready for root source-authorization consideration under the corrected contract;
authorization remains root's action. Required exact-head source review, hosted
checks, Windows<=660s and fresh exact-main gates remain future requirements.
Standards: 0 blocking; Spec: 0 blocking, 2 advisory; Security: 0 blocking.
