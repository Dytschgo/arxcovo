# Atomic join-anchor source round: stopped on natural failure

Sol 6.1 high, 2026-11-17, implementation child; assigned isolated worktree only,
no children. **Incomplete WIP; not accepted for publication.** PROGRESS is root-owned
and unchanged. M2 open / M3 unchecked; hardware **unverified on real hardware**.
Historical shared Sol3/3 and Opus0/2 counters unchanged.

## Pins and authorization

Prepared local `c9616b11116c4a8f77ce2ab936258429050a69c1`, source main
`380c0215d662f75c083015658a9f6f261808f2ea`. Source WIP commit `8a7b96293bc0ee43743227c90bcda63e167a110b`.
Read AGENTS, KICKOFF routing, CONTRIBUTING (owner routing supersedes historical
Opus requirement), engineering skill/safeguards, full atomic brief, normative root
review, independent contract recheck609fdd3f retained e92a897a, ADR0005 amendment.
Root corrections control over original2462bf33. No external API lookup was needed.
Coordinator-supplied evidence reused only: main CI44097456394 attempt1 green,
556s wall / Windows551s, all11 codec cases on every OS. That does not verify this source.

## Result and mandatory stop

First natural suite failed `registry_capacity_rejection_keeps_code_and_counter`:
expected `Rejected(MembersFull)`, actual `Rejected(AnchorChanged)`, at the shared
reject assertion in enrollment/tests.rs. The capacity fixture founds nodes1..16
with key(n), whereas the shared code fixture binds node1/key250. The new first
redemption fence therefore rejects before the old capacity assertion. The old
assertion remains intact. This is evidence of a missed fixture adaptation, not
authority to change the assertion or production fence.

Nextest fail-fast: **18/58 selected tests ran:17 PASS,1 FAIL;40 not run;599 skipped**.
Already-running tests completed during cancellation. No fix or rerun followed.
Fresh signer regression, literal anchor/proof binding, retained capacity/replay,
privacy and prior clock/race tests are among the17 actual passes (full log retained).
No durable test pass is claimed: failure stopped the run before those started.

One mechanical correction batch used out of at most two, before first natural
behavior check: wrong anchor receiver c->z in a three-node bad-proof setup;
missing LogPos/membership arguments in the existing crash-child setup; collapse
three nested-if Clippy diagnostics into one equivalent let-chain. Initial Clippy
exit101 retained, corrected Clippy exit0. No algorithm/limit correction followed
a test failure. A preparatory Python adaptation script had an IndexError while
locating a final call argument; it ran no build/test, and its completed prefix and
remaining edits were retained in the source patch. One exploratory rg command
used unsupported PowerShell brace syntax; it changed no files and was corrected.
Neither is a behavioral attempt or a claimed passing check.

## Actual verification

All Cargo builds used locked/offline flags and task-specific target
`D:/Temp/arxcovo-anchor-c9616b-target`; cache and logs retained, no deletion.
Inventory is a real nextest list of workspace all-target binaries, not a mock.
No test executable was invoked by inventory listing.

| Exact command | Exit | Duration | Result file stem |
| --- | --- | --- | --- |
| `cargo fmt --all -- --check` | 0 | 0.828 s | `fmt` |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 101 | 49.157 s | `clippy` |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | 11.438 s | `clippy-correction1` |
| `cargo nextest list --workspace --all-targets --locked --offline --message-format json` | 0 | 87.172 s | `inventory-result` |
| `cargo nextest run --workspace --all-targets --locked --offline -E (test(cluster::enrollment::tests::) | test(cluster::state_store::tests::) | test(cluster::log_store::tests::) | test(cluster::membership::tests::)) & !test(crash_child)` | 100 | 1.312 s | `focused-result` |

`cargo fmt --all` was applied before the first check and after the correction.
The single fmt --check pass precedes the correction; no final-head recheck is
claimed. All-target Clippy and nextest inventory are after the correction.
No local engine tests (including the new one-voter case), broad engine suite,
network/API/status/PR/push/watch/merge, installs/admin/native/VM work ran.
All task-owned subprocesses finished before evidence/commits.

Guard/sampler, four designated mutants and their restoration checks **UNRUN**:
mandatory natural-suite stop takes precedence. No mutant patch, compilation,
assertion failure, kill credit or restored-baseline claim exists. Required hosted
all-OS behavior, feature Windows<=660s, independent exact-head Sol high source
review, all five normal gates and fresh exact-main evidence remain outstanding.

## Source and old-test adaptations

Production edits stay in the exact allowlist: enrollment/code/codec, StateStore,
VoteLogStore, issuance, and founder response reconstruction. Shared membership,
consensus wrappers/adapter runtime, signing/verifier bodies, transport, Cargo,
workflows, timeouts/retries/permits/schedules and PROGRESS are unchanged.
No ingress/discovery/custody/controller/clockpolicy/genesis repair was added.

- enrollment/tests: all old apply sites mechanically call FixtureApply.apply_test,
  which supplies explicit synthetic one-voter canonical membership and LogPos;
  code generation uses a synthetic founded observation. All old semantic assertions
  remain, including the failing MembersFull assertion. Helpers changed: code,
  state, redeem, reject. Call sites changed: secret_markers_never_escape_debug_even_after_retry,
  identity_replay_and_node_ids_survive_pruning,
  first_applier_wins_and_proof_precedes_consumed_or_expired_status,
  skew_boundaries_ratchet_and_invalid_times_do_not_mutate,
  mint_order_pending_cap_and_replay_never_extend_expiry,
  retained_cap_only_pruning_frees_records_and_replay_precedes_full,
  node_exhaustion_never_issues_max_and_leaves_pending_intact,
  delayed_sole_proposal_grants_by_proposal_time_then_issuance_guard_refuses,
  retained_grant_check_matrix, replies_are_generic_and_replicas_deterministic,
  pending_precedes_total_cap_and_rejects_still_prune,
  wrong_proof_on_expired_pending_is_bad_proof,
  registry_validation_replay_gates_and_authorization,
  registry_capacity_rejection_keeps_code_and_counter,
  registry_commands_preserve_common_time_and_prune_prefix,
  founder_count_cap_is_independent_of_allocation_and_other_structure.
  text_is_bounded_checked_and_round_trips changes206->270 and checksum offset205->269,
  accepts arxj2 and explicitly refuses arxj1. transfer_hashes_match_independent_synthetic_vectors
  replaces proof/verifier/checksum literals using Python hashlib over literal domains/bytes.
  Retained-cap tests change64->55 and next-node74->65; pending-precedence setup
  consumes47 records leaving8 pending, preserving TooManyPending-before-StateFull.
- state_store/tests: code/redeem/base now use canonical membership and anchor;
  rich and all users inherit this setup without shifting original entry indices.
  canonical_roundtrip_and_semantic_decoder_rejections computes section start from
  canonical membership length, consumes209 rather than145 bytes per record,
  preserves node offset137, changes version-corruption byte3->4.
  membership_size_boundary_reaches_constructor_and_decoder keeps4096/4097 envelope
  bounds and changes opaque4096 success to Membership refusal.
  open_and_order_fail_closed uses DB1 as legacy and restores DB2 for corruption.
  install_and_replica_determinism supplies an empty valid state for the lower-term
  snapshot so origin corruption cannot mask the original stale-snapshot check.
  crash_child receives explicit pos/membership only. maximal_state_is_bounded_and_unsafe_sidecars_are_refused
  adds membership before mint (local index0->1), uses55 records and40 removals to
  retain16 members; canonical max membership replaces opaque4096 bytes.
  maximum_v2_image_and_registry_semantics_with_recomputed_checksums becomes its
  v3 equivalent:55 consumed, original full positions/time, next65, members39..54,
  15870 actual bytes,209 stride, duplicate-key record40 instead of49, invalid-next49
  instead of58. Existing registry corruption assertions remain.
- log_store/tests: payloads uses canonical membership; canonical_codecs_and_full_payload_semantic_rejections
  uses canonical1/16-node success and unknown tag7 rather than now-valid6;
  append_preconditions_and_maximal_capacity_are_atomic uses canonical3605-byte
  membership/3672-byte row and pins envelope MAX_ROW4163 separately.
  registry_rows_match_independent_raw_format_and_bounds uses ARXLE2 literal vectors.
  reopen_rejects_row_meta_inconsistency_versions_clusters_locks_sidecars uses DB1
  as legacy. two_store_ordering_replay_and_snapshot_install_ahead_of_tail and the
  invalid-time codec control receive explicit original pos/canonical membership.
- consensus/tests command_mapping_and_display_never_emit_command_contents receives
  an explicit synthetic anchor. three_node code/redeem and t1 bad-proof setup bind
  known fixture founder digests; no production shortcut. issue.rs test_ticket
  supplies canonical node1 registry context and explicit positions.
- issue_tests input helper and i4/i7/retained-record direct command calls add anchor;
  i7 adds own-response origin index/term/equal-index checks and older-origin control.
  New required anchored_single_voter_receipt_replay_and_revocation uses one actual
  start, founder_init/found/enroll, real CSR, origin receipt replay, AfterCommit
  removal and clean shutdown requirement. This case compiled but was not executed.
- found_tests f5_joiner_two and TLS enroll/founder_joiner_mutual_tls_real_identities/
  learner_catch_up_gives_joiner_own_registry observe applied membership/registry
  and carry anchor fields. Existing identity/signing assertions stay intact.
- inventory guard adds ISSUANCE_REQUIRED exactly the new case, required= wiring,
  omission/rename/ignore/Unix-gate controls and exact-word Cluster check. Original11
  list/reasons unchanged. Guard changes are unexecuted and need independent review.

Contract coverage is incomplete. In particular, remaining durable/legacy/corruption,
receipt and capacity assertions need disposition and actual execution after root
re-authorizes work; no complete contract-conformance claim follows from compilation.
The stop is not permission to resume the historical exhausted fix loops.

## Retained evidence

Local evidence directory `D:/Temp/arxcovo-anchor-c9616b-evidence` contains complete
stdout/stderr logs, command result JSON, real inventory JSON, source.patch,
changed-test-functions.json and per-file SHA256 source-hashes.json. These are
synthetic-test artifacts; no secrets/private owner data were read or collected.
SHA256 below pins each artifact. There were no mutant restorations to hash.

- `changed-test-functions.json`: `17a398520f20554f8f7a5dad84ae0132a3e95ddff349277e9b248df674fb2eff`
- `clippy-correction1.json`: `152523f9bab5787b37753537209be636c697734d6197b245322f297dccca3b57`
- `clippy-correction1.log`: `b6ff97a55baf5a0feb85541beabfea418197da4f0d83d4c74f3754fc9ea1673e`
- `clippy.json`: `c7afd484ba49cc36a715be889c062125e2dcf475ad5d8957ae907e630828ea7e`
- `clippy.log`: `c9fc1d759216f5c9b53d29fabc4b700a41b46e3c25b73ac99690fefb19b42494`
- `fmt.json`: `951f6dac0808ae16d4f23a1cba8389eccd2caeed93c39ee91bf943745a1854bc`
- `fmt.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `focused-result.json`: `6fa13128f6e013244e389451c8d24fc3ec1cb40aa9dd3ccff94f6a38f0dcb558`
- `focused.log`: `f0e4a8300282dc54832c7a5e4fcba1439225287a63978cc508bc759a71b3ce35`
- `inventory-result.json`: `9e38c1fc391640e66f75fa8b58288dc76342058a7878296ce17021f8b5516ce6`
- `inventory.json`: `30a8c1550fe1213b84fc0e59131a9ec88e927494a6c4bdc4fcc3b8e2d395ecd2`
- `inventory.stderr.log`: `b46c98b107cec3edb10ffbf53101715adbc34278db5d27bb387c269f9c9b643d`
- `source-hashes.json`: `fdc733200283cdf2823f2e7680309f7bc381f50bb2a3bb4c97fa3666c6cc4d19`
- `source.patch`: `48274097d13decd6fe20cb3ef3542ca25a5bf46c2aa44f9b2a2fa4ace9d9f75a`

## Proposed coordinator PROGRESS entry (not applied)

M3 atomic join-anchor internal kernel — Sol6.1 high builder, WIP 8a7b96293bc0ee43743227c90bcda63e167a110b;
mandatory natural-test stop (17 pass/1 fail/40 not run), capacity fixture anchor
mismatch; fmt initial pass, Clippy pass after1 mechanical correction batch, real
nextest inventory pass. No guard/sampler/mutants/engine/hardware acceptance.
Root disposition and independent review required before further source/testing;
no publication. M2open/M3unchecked, hardware unverified on real hardware;
historical Sol3/3 Opus0/2 unchanged.


## Authorized continuation at a28b752 (2026-11-17)

Sol6.1 high implementation child, assigned isolated worktree only, no children.
Prepared `a28b752ba25051b5ad4251da7a76eaee99fe834b`; root fixture correction
`2e6b64eb262a73a54fab18b32ebd50c34138b672`, original source/evidence pins above.
Read full brief, normative root review, independent recheck and new fixture
disposition. No production changes beyond prepared baseline. Added six tests
in the approved enrollment/state_store/log_store test files, source commit
`98b853058e022200cff9e9c71bfb32c791527702`. PROGRESS is root-owned and unchanged.

**Stopped at a new custom inventory-classifier static failure; no retry/fix.**
The final actual nextest listing succeeded (663 cases, exit0). A subsequent
ad hoc Python inventory classifier incorrectly selected only `tls_engine_tests::`
for a TLS14 assertion: founder13, selected TLS7, assertion `(13,7)` failed.
This is not a Cargo failure or evidence of inventory deletion. The classifier
never produced protected-inventories.json. Its earlier checks found all fixture28
ignored and sample8 names intact. Complete TLS14 classification is NOT claimed;
root disposition is needed before correcting/re-running this static check.
The guard's own six tests passed. No repository guard/filter changed here.

### Coverage audit before behavior execution

- Actual StateStore A/B refusal and counterfactual: existing
  authoritative_anchor_change_is_inert_before_consume compares entire enrollment
  state and applied E, with joint identical configs; control installs pre-B
  snapshot and enrolls. New anchor_boundary_variants_and_durable_inertness adds
  absent membership, absent voter binding, changed voter key, durable reopen
  equality and learner/address-only unchanged-anchor success. Whole-state equality
  covers records, counter, registry, committed time and pruning state.
- Existing maximum_v3_image_and_registry_semantics_with_recomputed_checksums
  independently constructs 55 consumed records/max canonical membership/members:
  actual15870, canonical3605. New new_mint_reservation_56th_pending_bytes_fit
  independently pins16361/16570 reservation arithmetic; proves a 56th pending
  record fits current bytes, asserts StateFull and exact unchanged state, exact
  Mint replay and pruning. Existing retained-cap test's prior pass is reused,
  not rerun. New replay_drift_authentication_and_pending_reservation_decode
  rejects56 expired retained pending records even though serialized bytes fit.
- New malformed_membership_refuses_all_state_boundaries_and_v2 tests malformed
  canonical payloads at apply/peek/install/open, before persisted mutation;
  explicit v2 snapshot refusal complements existing v1/DB-version refusals.
  New anchor_legacy_rows_and_malformed_membership_refuse_replay covers tag2,
  ARXLE1, metadata-v1 and malformed4096 membership at row replay/open/append,
  preserving durable rows/meta on append refusal. Canonical maximum3605,
  membership envelope4096 and row MAX_ROW4163 remain distinct; exact malformed
  envelope row4163 refuses Membership, old4164 CHECK corruption remains.
- New origin_time_corruption_refuses_peek_install_and_reopen reaches full origin
  index/term/equal-index leader mismatch, index above i64::MAX, original time
  after committed time and invalid original-time lifetime; repaired checksums
  reach semantics. Good snapshot install/reopen control retained. Existing
  replay_origin_survives_drift_snapshot_and_reopen verifies full E, original1000,
  expiry and fresh60000 after joint drift, snapshot install and reopen.
- Existing literal_anchor_and_proof_binding and replay_fresh_signer_clock_fence
  are prior17-pass evidence only. New replay_drift_authentication_and_pending_reservation_decode
  tests exact replay after joint drift, wrong proof/request/key/anchor and
  removal refusal, comparing entire state and original E/time. Prior privacy,
  proof/hash vectors, marker-positive controls and durable SQL/crash assertions
  are preserved. No semantic assertion removed. Only old-test diff is rustfmt's
  line wrap of root-corrected capacity assertion.
- R3 source audit: i7_adapter_rejects_forged_shapes_and_mismatches uses genuine
  grant data and own current response leader, then tests origin index>L,
  origin term>L at lower origin index, equal-index full-position mismatch,
  and valid older-origin control. Existing WrongNode path is separate.
  This quarantined engine fixture is compiled/listed, NOT run locally.
- Required anchored_single_voter_receipt_replay_and_revocation inspected with
  helpers: one actual start, founder_init/found/enroll, MemRouter, real CSR,
  no three-node Cluster helper, original E receipt and replay L>E, AfterCommit
  removal causing Revoked rather than another allocation/leaf, router clear and
  shutdown before pass. 35s work/40s entry including5s cleanup; startup blocking
  caveat remains. Required all-OS class is compiled/listed, NOT executed here.
  Founder13 was counted from real listing; full protected TLS14 classification
  stopped as explained above. No local engine test or engine mutant ran.

### Actual checks, reused evidence and source qualifications

Before additions, actual nextest list with root resume-selection filter selected
exactly40 original unrun names; compared original focused.log's17 PASS names
with original_passes and excluded those plus root corrected target. Retained
remaining-names.json is that exact list. After additions, executed an exact-name
union of those40 once (not an expanded filter that silently added new cases),
then a separate union of six new tests once. **40/40 and6/6 PASS.** No broad
passing-suite replay. Root corrected capacity PASS and original17 PASS are reused
only, never rerun naturally. Mutant4 intentionally executes its designated
previously-passing signer regression once under mutation, as authorized.

One mechanical pre-behavior correction for new tests: cargo fmt parser caught
one misplaced assert_eq closing parenthesis; fixed once, fmt and nextest listing
compiled. No compile/import/lint correction followed. Later diff inspection found
Python's default Windows read encoding had mangled the unchanged non-ASCII
malformed-code literal. Restored all three old test prefixes directly from git
UTF-8 bytes, retaining only additive tests, then formatted. This restoration
preserves old assertions, not a semantic repair. The40-case result therefore
used a malformed non-ASCII variant in text_is_bounded_checked_and_round_trips;
its original literal is restored on final source without a forbidden rerun.
Do not describe that individual test as an exact-final-bytes behavior pass.
Final fmt/Clippy and real listing ran after restoration. New six test bodies
were unchanged between their natural passes and final source.

All Cargo uses task target D:/Temp/arxcovo-anchor-c9616b-target, locked/offline;
no concurrent user of that target. No installs/network/cache deletion. Commands
and full argument vectors (including exact40 and6 filters), exit/time/log hashes
are in corresponding result JSON under D:/Temp/arxcovo-anchor-resume-a28b752.
Initial inventory/list compile also passed; no test executable ran during listing.
Final static checks were justified by new tests and UTF-8 prefix restoration.

| Result stem | Exit | Seconds including build |
| --- | --- | --- |
| `remaining40` | 0 | 3.453 |
| `new6` | 0 | 1.375 |
| `guard` | 0 | 0.329 |
| `sampler` | 0 | 0.407 |
| `fmt` | 0 | 0.812 |
| `clippy` | 0 | 11.640 |
| `mutant1` | 100 | 20.859 |
| `mutant2` | 100 | 19.531 |
| `mutant3` | 100 | 17.469 |
| `mutant4` | 100 | 17.297 |
| `final-fmt` | 0 | 0.796 |
| `final-clippy` | 0 | 5.172 |
| `final-inventory-result` | 0 | 16.672 |

`guard`: python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py
(6 PASS, including omission/rename/ignore/Unix gating and old inventory controls).
`sampler`: same unittest command with test_windows_diag_sample.py (60 PASS).
`final-fmt`: cargo fmt --all -- --check.
`final-clippy`: cargo clippy --workspace --all-targets --locked --offline -- -D warnings.
`final-inventory-result`: cargo nextest list --workspace --all-targets --locked --offline --message-format json.
Natural runs use cargo nextest run --workspace --all-targets --locked --offline -E
with exact filters in remaining40.json/new6.json. Pre-behavior compile used the
same inventory command (compile.json/log). No unchanged suite replay for final checks.

### Four designated mutants (once each, restored after each)

All four compiled successfully, exit100 at their designated assertion; none
survived, failed compilation, or failed at an unrelated assertion. No retries.
Each touched enrollment.rs only temporarily; baseline and every restored SHA256:
`955b728624272640dcad8483e7fcdf728c8dfc527f9be6236ecc23ce5066f4f2`.
Each mutant patch hash and restored hash is retained in mutantN-restore.json;
logs/results include the exact filter and build/passbuild evidence.

1. Replace first-consumption voter-anchor comparison with false: actual A/B
   StateStore assertion line831 returned Enrolled instead of AnchorChanged.
2. Replace retained consumption origin with current pos on replay: snapshot/
   reopened replay assertion line895 returned full origin(2,3,5), expected(2,3,3).
3. Remove only NEW Mint reservation clause: new test line938 returned
   Minted{replay:false,expires_ms:900000} instead of StateFull. Its earlier pending
   byte-fit assertion passed, so this is the required reachable capacity mutant.
4. Set fresh grant applied_ms to retained original time: regression line869
   returned Ok instead of SignerClockBehind at signer20000, first1000/replay60000,
   before any fresh-field equality assertion.

Retained source hashes and continuation.patch pin final additions. All artifacts
are listed with SHA256 in artifact-hashes.json, itself SHA256 `2909772dddf6a0b24c82785ff138cae5888fef9ede06c67cc073661f501a9474`.
Logs/caches retained; owned Cargo/Python processes finished. No push, PR, remote
status/comment/watch/link/merge, admin/native/VM or old Lab Manager operations.

### Remaining coordinator work and proposed PROGRESS entry (not applied)

Root disposition of custom classifier static failure; independent full Sol high
source and exact-head reviews; hosted five gates/all-OS new tests and required
one-voter engine; feature Windows<=660s and fresh exact-main evidence remain.
Main380c0215/CI44097456394 green is historical root evidence, not feature proof.
Existing exhausted repairs and Sol3/3 historical Opus0/2 remain unchanged.
M2open/M3unchecked; hardware **unverified on real hardware**.

Proposed entry: Sol6.1 high atomic-anchor continuation 98b853058e022200cff9e9c71bfb32c791527702: six additive
boundary tests; remaining40 and new6 passed once, original17/root target reused;
guard6/sampler60/final fmt/Clippy passed; all4 designated mutants compiled/killed
at exact assertions and restored. Actual nextest listing passed, custom TLS14
classifier failed(13,7); stopped without correction/rerun. Independent source/
exact-head review and hosted gates pending; no publication/hardware acceptance.
