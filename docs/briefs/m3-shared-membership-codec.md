# M3 L2-A: extract the canonical membership-v1 codec

Design only; Sol 6.1 / high, 2026-11-17; no children. Coordinator review precedes implementation.
Inspected HEAD/base: `3792bd89398e312e4ec85dbab06c1ce99fc787a7` in the assigned isolated worktree.
Coordinator supplies tree equivalence to PR171 merge `fa7e3c019484bf2335bd26fbbafd2d65b207f480`;
this task did not independently compare that tree or query CI. Exact-main CI `44090513191` is pending per assignment.
No source build may start before coordinator verifies that exact main green; refresh the implementation base then.
Read [ADR0005](../adr/0005-raft-membership-and-enrollment-spike.md),
[production L2](m3-production-learner-admission.md), its [root review](../reviews/m3-production-learner-contract-root-review.md),
current consensus codec/helpers/tests, StateStore/Blob, cluster module and cached OpenRaft 0.10.0 source.
This is the next bounded prerequisite for L2 B3, not authority to implement the whole proposal.

## Scope and unchanged contract

Extract existing structural membership representation/encoding/decoding into `cluster/membership.rs`.
Keep membership-v1 bytes, limits, canonicality and consensus `StoreError::Membership` normalization identical.
No new wire/schema/code version, migration, DB change, StateStore validation/acceptance change or enrollment command.
Do not activate apply-time decoding, anchors, arxj2, CA/receipt/transport/controller policy or owner-control behavior.
ADR0005 owner decisions remain accepted: fingerprint voters, bounded admission/progress, host2 learner until host3.
Their implementation remains separate. No new architectural deviation is proposed and no ADR edit is needed.
StateStore currently accepts opaque nonempty <=4096-byte membership and apply clones it; leave that behavior intact.
The shared decoder is merely made available for a separately reviewed atomic apply-time fence follow-on.

## Concrete API and dependency direction

Add `pub(crate) mod membership;` to `crates/arxd/src/cluster.rs`; no public external API.
In that sibling module use only std, with `pub(crate)` representation `Membership` and unit `MembershipError`.
Private fields: `Vec<BTreeSet<u64>>` configs and `BTreeMap<u64, String>` nodes; derive Clone/Debug/PartialEq/Eq.
`Membership::new(configs, nodes) -> Result<Self, MembershipError>` validates all invariants before returning ownership.
Expose immutable `configs() -> &[BTreeSet<u64>]`, `nodes() -> &BTreeMap<u64,String>` and consuming
`into_parts() -> (Vec<BTreeSet<u64>>, BTreeMap<u64,String>)`; no mutable fields/access or unchecked constructor.
`encode(&self) -> Result<Vec<u8>, MembershipError>` and `decode(&[u8]) -> Result<Self, MembershipError>` are bounded.
Decode checks input nonempty/<=4096 BEFORE parsing/allocation, bounds each count/length BEFORE collecting/copying,
uses constructor validation, complete consumption and encode-back byte equality. No truncation, repair or normalization.
Use a tiny PRIVATE slice reader with checked take/u64 BE and local MembershipError; never index unchecked input.
Do not reuse enrollment::codec::Reader: it imports StateStore and reports Corrupt on short input.
Pure module must import neither enrollment, StateStore/Blob/StoreError, OpenRaft, async/runtime, DB nor network.
Dependency direction: consensus -> membership; future StateStore -> membership; membership -> std only. No cycle.

Retain `type Members = Membership<u64, BasicNode>` and existing consensus `encode`/`decode` signatures and visibility.
Use an alias for the pure Membership to avoid the OpenRaft name collision. Encode copies joint configs and ALL node
addresses into pure parts, then pure encode -> MembershipBlob::new. Check engine counts/address sizes before cloning
so an invalid engine object does not cause unbounded temporary copies. Map EVERY failure to StoreError::Membership.
Decode calls pure decode, consumes parts and maps String -> BasicNode::new, then existing OpenRaft Membership::new.
Keep wrapper encode-back equality if needed for direct equivalence; do not introduce different failure categories.
OpenRaft conversion stays in consensus, never in the pure module. to_stored/from_stored and all callers stay unchanged.
Remove only the codec's Reader import; preserve collection names inherited through `super::*` by frozen tests,
using cfg(test) imports if production no longer needs them. This avoids a predictable test-only import failure.

## Exact accepted and refused domain

Wire layout: version:u8=1 | configs:u8 | (voters:u8 | node:u64 BE*)* | nodes:u8 |
(node:u64 BE | addr_len:u8 | raw address bytes)*. No log position, checksum, cluster ID or engine metadata.
Exactly 1..2 ordered configs, each 1..16 voters; exactly 1..16 total nodes including every learner.
IDs are nonzero u64, including u64::MAX; each serialized voter set and node table strictly ascending and unique.
Every voter occurs in nodes. Learners are nodes outside the UNION of voters; retain every learner and address.
Configs themselves are ordered, NOT sorted/deduplicated: [{1},{1}] is ACCEPTED and stays two configs;
[{2},{1}] is accepted if both nodes exist. Overlap and identical sets are valid; never flatten or union configs.
Cached openraft-0.10.0/src/membership/membership.rs Membership::new stores the supplied config Vec unchanged
and only ensures voter metadata exists. It permits empty configs, but our prior codec explicitly refuses them.
BTreeSet/BTreeMap constructor inputs already have unique keys; duplicate-ID rejection applies to serialized input.
Address is 1..200 BYTES, ASCII alphanumeric or one of `._:[]-`; case and spelling remain exact.
Accept `mem:1`, `localhost` (BasicNode default), `a`, bracket/colon/dash forms, even syntactically odd endpoints.
Do not add numeric SocketAddr, nonzero-port, DNS, IP-class or address-ownership validation in this extraction.
Reject empty/201-byte/non-ASCII/invalid UTF-8/space/slash/other punctuation addresses; do not trim or lowercase.
Reject wrong version; zero/3 configs; zero/17 voters or nodes; zero/duplicate/descending IDs; missing voter metadata;
every truncation, inconsistent lengths and any trailing byte. No panic or partially valid return.
Blob envelope is nonempty..4096. Existing maximal valid encoding is 3605 bytes (two 16-voter sets, 16x200 addresses);
3606..4096 bytes cannot form a valid membership under these limits. Do not lower the envelope to 3605.
Pure failures use MembershipError; consensus maps them, short-read/UTF-8/OpenRaft and Blob failures uniformly to
StoreError::Membership. MembershipBlob::new itself still returns Corrupt for empty/>4096 input; do not change it.

## Independent vectors and meaningful tests

New tests live in membership.rs under cfg(test), named under `cluster::membership::tests`; plain synchronous std tests.
No engine/store/runtime/temp directory/fixture creation or OpenRaft imports in these new tests; required on ALL OS.
Pin these literal hex vectors (whitespace-free concatenation of the displayed groups); never generate expected bytes
with the new encoder or a helper sharing its algorithm. Assert decoded parts AND exact encoding in both directions.
U: voters [{1}], nodes {1:"a",2:"b"}; 2 is learner; 32 bytes:
`010101 0000000000000001 02 0000000000000001 0161 0000000000000002 0162`
J: voters [{1},{1,2}], nodes {1:"a",2:"b",3:"c"}; 3 is learner; 59 bytes:
`010201 0000000000000001 02 0000000000000001 0000000000000002 03`
`0000000000000001 0161 0000000000000002 0162 0000000000000003 0163`
D: voters [{1},{1}], nodes {1:"a"}; duplicate-config acceptance, 31 bytes:
`010201 0000000000000001 01 0000000000000001 01 0000000000000001 0161`
These are independently specified v1 vectors from the frozen layout, not claimed executed prior-codec outputs.
Pin reverse config order, max ID, default-address spelling, overlap and 200-byte boundary with fixed semantic expectations.
Pin the known 3605-byte maximal case and every prefix including empty; all proper prefixes must refuse.
Adversarial table uses fixed edits/offsets in U/J/D: all refused-domain classes above, including node-table order,
invalid UTF-8, length 0/201, a well-formed missing voter, valid prefix+trailing byte, 4096 and 4097-byte inputs.
For malformed-length cases ensure refusal is not accidentally only truncation; supply the declared payload as appropriate.
Constructor tests independently cover 0/3 configs, 0/17 voters/nodes, ID0, missing node and invalid addresses.
Freeze prior consensus/tests.rs bodies, especially maximal_codec_is_canonical_and_every_truncation_fails and
codec_rejects_noncanonical_and_out_of_bounds_memberships; they remain compatibility coverage in normal CI.
Use their existing offsets/expected StoreError::Membership as prior behavior evidence, not a rewritten oracle.
Add only small synchronous wrapper compatibility tests there if needed: literal U/J/D and short-read normalization.
No new fixture or broad differential harness; fixed independent vectors plus preserved prior tests suffice.

## Bounded verification and mutants for the later builder

After root acceptance and exact-main green: one natural round of fmt check, locked/offline workspace all-target Clippy,
new pure-focused run, existing Python quarantine guards and nextest LISTING only for protected engine inventories.
Suggested commands: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`;
`cargo test -p arxd --lib --locked --offline cluster::membership::tests`; and
`python -I -m unittest discover -s tools/ci -p 'test_windows_cluster_quarantine.py'`.
Use `cargo nextest list -p arxd --locked --offline` for inventory, never local engine execution or --ignored.
Ordinary unchanged hosted CI runs old compatibility and all new pure tests on Ubuntu/macOS/Windows.
Three bounded mutation probes ONLY against new pure tests, each compiled/run ONCE, restoring and checking baseline
file hash after EACH: (1) encoder drops learner metadata -> U literal equality fails; (2) constructor flattens identical
configs -> D decoded-parts assertion fails; (3) shared address validator admits `!` -> valid-length bad-address refusal
assertion fails. No engine case or natural cluster loop. Each probe must actually change source and compile; require
the designated semantic assertion to fail, not any failure. Target each named test alone, avoiding redundant-fence mutants.
Record exact test/result and restored hash; compile failure/survival/unrelated red gives no killed-mutant credit: stop.
No mutation of old tests, wrappers, dependencies or workflows; no mutant push. Do not repeat restored natural suite.
Routine compile-only import corrections may be recommended to root; ROOT sets any correction/recheck allowance.
Natural behavior red stops immediately: preserve evidence, no rerun/push-to-green or speculative repair.
Three-fix rule and exhausted shared Sol3/3, Opus0/2 remain; changing reviewer does not renew attempts.

## Implementation allowlist, gates and readiness

Later source allowlist ONLY: new `crates/arxd/src/cluster/membership.rs`; one module binding in cluster.rs;
codec wrappers/imports in consensus.rs; additive synchronous wrapper tests in consensus/tests.rs if required above.
Coordinator alone updates docs/PROGRESS.md. Freeze all existing caller/test bodies wherever practical;
if extraction needs another file or behavior change, stop and return the concrete conflict before proceeding.
No Cargo/lock, filters, retries, timeouts, nextest config, workflows, adapter, StateStore, log store or fixture edits.
Windows quarantine28/sample8, founder13/TLS14 and joint ignored classification stay unchanged; no recovery claim.
Feature head must meet Windows<=660s, normal five required gates (three OS verify, ci-time-budget, agent-review),
fresh exact-head independent Sol source review and fresh green main; coordinator alone publishes/merges.
Assignment reports PR171 actual seven-scenario joint proof PASS Ubuntu3.040s/macOS4.983s, Windows582s with new case
ignored. This is coordinator-supplied evidence, not queried/re-executed here; Windows recovery remains unproven.
Readiness: source-supported API design, compile/runtime UNVERIFIED; root review/main-green gate still pending here.
Material implementation blocker would be any newly discovered mismatch with frozen prior-codec semantics; stop,
do not narrow acceptance to simplify the module. No further owner interview needed for the routine choices above.
Separate follow-on proposal only: shared decode at StateStore boundaries and atomic join-anchor fence, with its own
schema/command/receipt contract and authorization. L2-A grants none of that work; production L2 blockers remain.
Proposed PROGRESS entry (coordinator-owned, not edited): “Sol6.1 high L2-A design at the delivered brief commit:
static codec/API/edge-case inspection; implementation/review/main-green pending; no source/build/runtime/network.
M2 open, M3 unchecked, hardware **unverified on real hardware**; Sol3/3 Opus0/2 unchanged; last merge PR171 per root.”
Actual task verification: documentation line count, whitespace/diff/scope and relative links only; no compile/test run.
Only this brief is committed locally; no PR/push/status/watch/link/merge/admin/VM/install/cache deletion authorized.
