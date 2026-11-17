# M3 L2-B: atomic redemption anchor fence

Implemented local kernel; independent Sol high source review PASS c5ccb6c4. Hosted gates remain pending.
Original design inspection: Sol 6.1 high, 2026-11-17; no children; assigned isolated HEAD
`380c0215d662f75c083015658a9f6f261808f2ea`; tree equality to reviewed `27b412b0` is coordinator-supplied.
Root reports PR172 merged normally: eleven pure/wrapper/compat tests PASS all OS, Windows510s.
Root subsequently confirmed exact-main CI44097456394 attempt 1 green: 556 s wall, Windows 551 s; all eleven cases PASS on every OS. Source was subsequently authorized under ADR0005; no hosted behavior is claimed yet.
Root clarification review is in [the contract review](../reviews/m3-atomic-join-anchor-root-review.md); its corrections are normative.
Independent Sol contract recheck609fdd3f PASS; root authorizes the bounded local source round under ADR0005. No production security acceptance follows.
M2 open, M3 unchecked; hardware **unverified on real hardware**; shared Sol3/3, Opus0/2 unchanged.

## Decision and local grounding

Implement the smallest integrated kernel: existing trusted `ConsensusNode::enroll` -> anchored Command ->
adapter -> `StateStore::apply` -> enrollment transition -> durable grant -> existing guarded signing.
No L2 listener, AnchorQuery, discovery, receipt custody, admission controller or promotion is required for this kernel.
There is presently no actual enrollment ingress. The result proves an internal apply boundary, not safe public joining.
Read [ADR0005](../adr/0005-raft-membership-and-enrollment-spike.md),
[L2 proposal](m3-production-learner-admission.md), [root review](../reviews/m3-production-learner-contract-root-review.md),
[decision review](../reviews/m3-membership-decisions-root-review.md) and [shared codec](m3-shared-membership-codec.md).
D1 voter-key fingerprint before proof, D2 five-second response/progress and D3 learner-until-three remain adopted.
The 792-character variable endpoint-hint format is only a proposal. This slice chooses anchor-only 270-character codes;
discovery policy remains separate. No new owner question is needed for these routine implementation choices.

Actual source inspected under `crates/arxd/src/cluster/`: enrollment.rs, enrollment/{code,codec,registry,tests}.rs;
state_store.rs and tests; log_store.rs and tests; membership.rs; consensus.rs and consensus/{adapter,issue,found,
tests,three_node,issue_tests,found_tests}.rs; caller search also covers tls_engine_tests.rs. No external API facts needed.
`membership::Membership` is pub(crate), immutable configs/nodes with bounded canonical decode, std only.
EnrollmentState members/records are private; registry exposes authorize_peer but no key enumeration. Add one bounded
pub(in crate::cluster) `voter_anchor(&Membership) -> Result<JoinAnchor, Reject>` on EnrollmentState; hash inside enrollment.
`StateStore::apply(pos,&Entry)` clones private Image, calls state.apply, persists snapshot then returns Outcome.
Adapter Sm::apply already calls that API under the store lock; its response is sent only after persistence. Keep it unchanged.
`EnrollmentGrant` constructors are private; `IssueTicket` is cluster-visible with private grant/leader/index.
`issue::committed` binds current ClientWriteResponse leader/index; two fresh issue_guard calls check retained grant,
registry binding and leader identity. `found.rs` uses separate sealed FounderTicket/guards. Preserve their strength.

## Canonical anchor and one authoritative transition

Propose `JoinAnchor([u8;32])`, equality/accessor only; no default zero anchor or unchecked optional fallback.
SHA256(exact ASCII `arxcovo/join/anchor/v1` || cluster16 || voter_count:u8 || sorted(node:u64 BE || KeyDigest32)*).
Exactly ONE config, 1..16 voters, every voter bound in E-1 registry; missing/absent/joint => Reject::AnchorChanged.
Two identical configs are still joint and refuse. Exclude learners, addresses, membership position and registry nonvoters.
Hash existing stored KeyDigest bytes, not SPKI again; no CA digest substitution. Maximum hash input is 22+16+1+640=679 bytes.
Shared codec stays std only and byte-identical; sha2 remains in enrollment, with existing dependencies.

Replace public `EnrollmentState::apply(&Command)` with cluster-visible
`apply(&Command, LogPos, Option<&membership::Membership>) -> Outcome`; no retained unanchored production overload.
StateStore decodes its E-1 membership before calling it with current full E. Membership-entry apply, Image decode
(open/peek/install), and log membership-row append/decode use the same canonical decoder; malformed => StoreError::Membership,
no persistence/signing. Valid absent/joint/unbound membership is state, not corruption; it refuses first Redeem only.
MembershipBlob's nonempty/4096 envelope stays unchanged; no narrowing of its constructor or shared codec semantics.
All first Redeem paths use the fence, including direct trusted proposals; issuer prechecks cannot replace it.
After existing wrong-cluster/time-range/clock validation, inspect retained code/proof/request/key WITHOUT mutation.
For an unconsumed valid code, compare command anchor to E-1 voter_anchor BEFORE committed_ms, prune, allocation or consume.
AnchorChanged preserves enrollment state byte-for-byte, including timestamps/tombstones; applied E still advances durably.
Bad-proof/unknown-code retain generic ingress reply and existing validation order; do not disclose anchor details publicly.
Other refusals keep existing common timestamp/pruning semantics; document/test those rather than promise all refusals are inert.
Evaluate replay eligibility against tombstone lifetime at effective time; never return a record that normal pruning would remove.
Once fence passes, existing expiry/key uniqueness/member/node-counter checks remain. Persist consumption, registry binding,
counter, original E and original effective applied_ms together; SQL failure remains unknown/poisoned, never a returned grant.

Consumption adds anchor32, origin:LogPos24 and original_applied_ms:u64. Grant adds anchor/origin/original-time getters.
EXISTING applied_ms remains the CURRENT operation effective time, including replay, and check_issuable continues using it.
check_grant matches request/key/node/anchor/origin/ORIGINAL time/expiry to the record; original_time <= fresh applied_ms <= committed_ms.
Exact live proof+request+key+anchor replay bypasses NEW-consumption voter fencing after voter drift, including joint state,
only while original record is retained and node/key still bound. Wrong request => Consumed; wrong key retains KeyConflict;
wrong anchor refuses without grant; Remove => Revoked. Never reinsert a removed node. Replay preserves expiry/origin time;
fresh applied_ms advances for signer rollback protection, never as renewal of the original grant lifetime.
Before/at-fence rejection is unconsumed; after E, a lost response/guard failure can mean consumed. No unconditional unspent claim.
Expired retained replay may return its old grant as today, but check_issuable still refuses signing; pruned replay is UnknownCode.

## Command, code and receipt contract now

Add required anchor32 to Command::Redeem and Command::redeem; replace its old tag2 with NEW tag6.
Layout: 6 | cluster16 | request16 | code16 | verifier32 | key32 | proposed_ms:u64 | anchor32. BE integers, exact consumption.
No persisted origin in the command: StateStore supplies E, never a caller. Tags0/3/4/5 and Mint tag1 layouts stay unchanged.
Mint still stores verifier only; no second stored authoritative anchor is necessary: new proof binds the anchor cryptographically.
JoinCode::generate gains required anchor, accessor; no legacy generator. Emit/parse fixed `arxj2-` + hex of
cluster16 | code16 | secret32 | ca_fp32 | anchor32 | check4 =132 binary bytes, 6+264=270 text characters.
Checksum first4 SHA256(`arxcovo/join/check/v2` || preceding128 bytes). Reject arxj1; exact 264 hex payload, bound input
before decoding, preserve current outer ASCII whitespace/hex-case acceptance. Reject suffixes/truncation/extra fields.
Use proof SHA256(`arxcovo/join/proof/v2` || cluster16 || code16 || secret32 || anchor32); verifier domain v1 stays unchanged.
Thus altered code anchor, even with repaired checksum, changes proof/verifier; Mint replay's verifier comparison still binds it.
Only fresh generated in-memory code becomes Mint; decoded Mint is never re-proposed. No proof/secret persistence or forwarding.
Minting callers obtain anchor from an explicit applied membership+registry observation; stale observation may produce a code
that later refuses unconsumed. No new public mint endpoint or promise of a pre-proof authenticated observation in this slice.

RedeemInput gains anchor; enroll passes it into Command::redeem. IssueTicket retains CURRENT response L leader/index,
adds original full E via grant; committed validates origin.index<=L.index, origin.term<=L.term and full equality at equal index.
Malformed origin uses existing Rejected(UnknownCode); direct adapter tests must reach this check with otherwise valid own-response fields.
Do not use E's former leader for fresh issuer leadership checks. Keep both current-L issue_guard checks, CSR possession,
key match, signer expiry/rollback, CA-fit and sealed subject checks. IssuedLeaf gains `identity_commit: LogPos` from grant E.
found() already returns IssuedLeaf: reconstruct this field from committed FounderTicket leader/index, as its founding position.
FounderTicket/LeafSubject signing authority and every old founder assertion stay intact.
E survives snapshot/restart and replay at L>E, without renewing original grant lifetime. No durable joiner receipt/key custody yet.

## Versioning and byte budget (proposed source/snapshot break, not a migration)

Image magic stays ARXSM1, embedded version2 ->3; decode versions1/2 as Unsupported before enrollment parsing.
SQLite state user_version1 ->2 for NEW stores; existing nonzero1 refuses without rewriting rows or auto-migrating.
Log NEW stores use user_version2, metadata embedded version2, row magic ARXLE2; old DB/meta/ARXLE1 rows refuse Unsupported.
Tag2 refuses Unsupported, never translates to6. New codecs reject trailing bytes/re-encode mismatch; versioned refusal tests
must cover both log replay and snapshot installation. No mixed-version cluster or rolling upgrade compatibility is claimed.
Snapshot consumption validates origin.index<=i64::MAX and <=Image.applied.index, term<=applied.term, exact full equality
at equal index; original_applied_ms<=committed_ms, original_applied_ms+2*SKEW_MS<expires_ms and existing checked time bounds.
Keep sorted unique records/nodes, node<next, unique current keys and retained/current key consistency; revoked records may remain.
This is scoped to unreleased local M3 probe data, not an assertion that all owner clusters are disposable. Preserve files;
no delete/reset/migration/admin steps. If retained deployed data must remain operable, STOP for a separate migration contract.
Open refusal must not modify application rows/schema/version; existing SQLite locking/journal mechanics are not byte-identity claims.
Genesis first_free persistence is a separate known concern; do not smuggle a genesis format fix into this slice.

Current record: code16+actor16+request16+verifier32+expires8+flag1=89 pending; consumption16+32+8=56 =>145 consumed.
New consumption adds32+24+8=64 =>120, new consumed record209; pending stays89. Members max16*40=640; state fixed36.
Full Image overhead excluding membership payload/enrollment: magic8+version2+applied_flag1+pos24+membership_flag1+
membership_pos24+length2+checksum32=94. With 4096 membership:4190. Old maximum =4190+36+640+64*145=14146;
actual canonical membership max3605 =>13655 (existing literal test). New64-consumed maximum18242; actual max17751: too large.
Reserve future consumption/member/membership growth at Mint: `94+4096+36+640+(records_after_prune+1)*209 <=16384`.
55 records =>16361 (23 spare); 56=>16570 (186 over). MAX_RECORDS64/MAX_PENDING8/MAX_MEMBERS16 remain unchanged;
effective Mint capacity55, deterministic Reject::StateFull, no consumed/allocation effect; normal Mint time/pruning still applies.
Reject snapshots violating this reservation invariant even if current pending bytes fit; reserve is not an extra serialized field.
This conservative capacity refusal ensures Redeem and later membership/registry growth cannot strand committed apply at TooLarge.
Keep StateStore persist's final LIMIT check and SQL CHECK16384; storage errors are not StateFull/known-unspent results.
Log envelope: magic8+pos24+checksum32=64; membership tag3:u8+len2+4096 gives4163, actual3605 gives3672.
Old Redeem body121/new153 => row185/217. Mint105=>row169 unchanged; Found26+40*n =>row90+40*n,
max wire64 founders =>2650 (apply still max16). No limit increases: MAX_ROW4163, membership4096, state16384 stay exact.

## Later implementation allowlist and mechanical adaptation

Production allowlist: enrollment.rs; enrollment/{code,codec}.rs; registry.rs only voter_anchor access if needed;
state_store.rs; log_store.rs; consensus/issue.rs; consensus/found.rs only full founder position/new response field.
Prefer voter_anchor in enrollment.rs with private members access. Freeze membership.rs/consensus wrappers/adapter runtime,
pki signing/verifier bodies, transport, service, Cargo/lock, workflows, filters/quarantine, schedules/timeouts/retries/permits.
Test edits allowed only enrollment/tests.rs, state_store/tests.rs, log_store/tests.rs, consensus/{tests,three_node,
issue_tests,found_tests,tls_engine_tests}.rs and cfg(test) test_ticket in issue.rs; no assertion deletion or proof weakening.
Mechanical allowlist: required anchor arguments/fields; explicit LogPos/membership context at pure apply sites; synthetic
canonical membership/bound voters in setup; fixture entry indices shifted for added membership; new version/tag bytes,
checksum offsets and receipt field patterns. Three-node code helper uses known fixture founder digests, no production shortcut.
Old raw opaque membership acceptance, v2 image success, 64-record success and fixed code vectors must become explicit legacy
refusal coverage plus new-format equivalents. These are intentional contract changes, not claims old test bodies stayed unchanged.
Keep every old semantic assertion (dedup/races/revocation/clock/SQL/crash/privacy/CSR/leaf identity), adapting only its setup;
list every changed test/offset with reason in evidence. Any further behavior/caller/file need stops for root scope review.

## Meaningful tests and bounded verification proposal

Pure + durable tests required on Ubuntu/macOS/Windows; no engine execution locally on Windows. Golden hash uses synthetic
cluster/node/digests with independently fixed expected bytes, not expected values generated by the production hash helper.
Literal vector: cluster=16 bytes 0x11, one voter node1:u64 BE, stored digest=32 bytes 0x22. Hash input79 bytes;
expected SHA256 `71fd16b1bafc577123cfd694b1d196cd280c4f42b8f36e2d8e99f3590f708b01` (local .NET calculation here).
At ACTUAL StateStore boundary: Mint under A; observe/precheck A; apply membership B; Redeem(A) => AnchorChanged,
same records/counter/registry/time/pruning but applied E advances. Counterfactual omit B succeeds and persists origin E.
Also joint identical-config, absent membership, removed voter binding, voter key change, learner/address-only changes;
the latter preserve anchor. Bad malformed membership refuses before persist at apply/log/open/peek/install.
Success/replay tests compare full E (term/leader/index), original time and expiry across later L, membership drift, snapshot
install and close/reopen. Wrong proof/request/key/anchor and Remove replay produce no new binding/allocation/certificate.
Pure grant check tests forge each retained-field mismatch inside test visibility; actual durable replay is the primary proof.
Byte-budget tests independently build maximum55 consumed records/max membership/members, assert16361 envelope/15870 actual,
56th Mint StateFull and unchanged code count/counter, post-prune room reclaimed; exact/over4096 membership and4163 log bounds.
Test all code prefixes/suffixes, anchor/checksum substitution, legacy versions/tags, reduced verifier only persistence;
retain positive secret-marker controls. Durable SQL/crash pre/post commit tests retain unknown-outcome/poison/reopen assertions.
One new standalone ONE-voter issuance integration in issue_tests.rs uses existing ConsensusNode/Params/MemRouter/ephemeral CA
and ProvenCsr types (not Cluster::new three voters): enroll -> original E; replay -> L>E; released leaf retains E and real CSR key.
Reuse existing AfterCommit pause to Remove joining binding before release: Revoked, consumed remains, no leaf returned.
Member-change-before-consume counterfactual belongs in deterministic store tests, not a new multi-voter engine fixture.
Keep founder13/TLS14 and joint classification; new one-voter case is required all OS, outside founder inventory.
The inventory guard is additionally allowlisted ONLY to add the required single-voter class described in the root review. No Windows engine
run by builder; first engine evidence is ordinary hosted CI. Preserve quarantine28/sample8 and all old quarantined tests.

After root authorization/exact-main green, ONE source round: fmt --check; locked/offline workspace all-target Clippy -D warnings;
focused pure/codec/durable tests excluding crash-child helper and all engine fixtures; existing guard5/sampler60; nextest listing
only for protected inventories. Hosted unchanged ordinary CI supplies all OS required suites/one-voter integration and Unix engine.
Recommend root allow <=2 mechanical compile/import/lint corrections BEFORE first natural behavior check; no algorithm/limit fixes.
Natural semantic/static-suite red stops immediately: preserve evidence, no rerun/tuning. Existing exhausted counters stay exhausted.
Four designated new-source mutants, each compiled/run ONCE with exact baseline hash restoration after EACH:
(1) skip first-redemption anchor comparison -> actual store A/B test wrongly enrolls; (2) replay origin overwritten with L ->
snapshot/reopen replay full-E equality fails; (3) remove Mint reservation fence -> 56th Mint StateFull assertion fails while pending
image still fits. Target named tests only; compiler failure, survival or unrelated red stops, no kill credit/retry/natural-suite replay.
Fourth pure mutant: replay replaces fresh applied_ms with original_applied_ms; the fresh signer-clock refusal regression must fail.
Root review pins its exact 1000/60000/20000ms vector and guard changes; no extra engine fixture or old assertion weakening.
Independent Sol source reviewer verifies allowlist, changed old assertions, vectors, arithmetic, restored hashes and exact head.
Normal five gates: verify(ubuntu-latest), verify(macos-latest), verify(windows-latest), ci-time-budget, agent-review; green exact-head
source review, feature Windows<=660s and fresh green exact main before coordinator publication/merge; no protection exceptions.

## Readiness, blockers and coordinator entry

Kernel contract accepted for bounded local implementation under ADR0005 after independent recheck609fdd3f. Publication and runtime acceptance remain pending.
No full-flow blocker to this kernel. Public D1 remains blocked on same-connection pre-proof anchored-key possession/discovery/read
ownership; real transport, CA custody, authenticated control identity and trusted production clock/expiry policy remain separate.
Root-owned PROGRESS unchanged. Proposed entry: “Sol6.1 high atomic-anchor design: local API/schema/byte-budget inspection only;
independent contract recheck and source authorization pending; exact-main44097456394 green confirmed by root. PR172 last merged per root; no runtime claim.
M2 open/M3 unchecked; unverified on real hardware; Sol3/3 Opus0/2 unchanged.”
Actual task checks: local reads, synthetic SHA256 vector calculation, documentation line count/relative links/UTF-8/whitespace/diff scope; local commit only.
No build/test/network/PR/watch/link/merge/admin/VM/install/cache deletion, and no source or PROGRESS modification authorized here.
