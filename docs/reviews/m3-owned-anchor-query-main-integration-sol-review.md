# Owned anchor query: independent main integration review

Sol 6.1 high, 2026-11-18; independent review sub-agent, no children.
PASS for the assigned local source integration. No publication or merge authority.
Reviewed HEAD: 2c599ffd7eeebd161191a8e3127d907bfef475de.
Pinned main: 10aa891f27906e0c73e48396c5c812cab7d4523a.
Reviewed feature: f9d0a413d02737214af62601aa345c90634a4e26.
Source main: 3022d601f09f24b0e5f092a33f2e9343b500471d.
Prepared checkpoint: e5fca6bfe1268c3f115c3eb345ca8ed52d7c1963.
Builder evidence: 15610c3335a0411ca45f5e9c7de6c8aa71dcbadd.
Prior independent source review: 89efaaf5, retained at 0c9efc0a.

Read AGENTS, KICKOFF, CONTRIBUTING, SECURITY, dytschgo-review/profile,
owned-query brief, normative root R1/R2, contract recheck, ADR0005 amendment,
source review and both owned-query evidence documents. Reviewed the full feature
diff, query/test bodies and surrounding adapter/shutdown semantics. Current owner
Sol routing supersedes historical cross-provider text. Assigned worktree only.

## Standards

No blocking finding or actionable baseline smell. All five working files and
HEAD blobs equal reviewed f9d0a413 byte-for-byte:

- crates/arxd/src/cluster/consensus.rs
- crates/arxd/src/cluster/consensus/anchor_query.rs
- crates/arxd/src/cluster/consensus/anchor_query_tests.rs
- crates/arxd/src/cluster/enrollment/anchor.rs
- tools/ci/test_windows_cluster_quarantine.py

The complete binary five-path diff from source main to feature equals pinned
main to reviewed HEAD, SHA256:
6bd0c42ec03c2f4b49254f9592f7f6c5c30f4f1212e36601b33519dcd1ae5e72.
All remaining 260 non-document main paths equal pinned main. This includes:

- scripts/ci-budget.mjs and scripts/ci-budget.test.mjs
- scripts/ci-workflow.test.mjs
- scripts/evaluate-budget.mjs and scripts/evaluate-budget.test.mjs
- scripts/fixtures/ci-budget-buffer.mjs
- scripts/fixtures/ci-queue-credit-runs.mjs
- scripts/policy.mjs and scripts/policy.test.mjs

All workflows, protection/authors files, sampler, nextest configuration and
docs/ci-duration-history.json are frozen to pinned main. PROGRESS also matches
pinned main. History from source main to pinned main contains only PR163's
queue-policy scripts and documentation. Integration fdc5b327 has parents
e5fca6bf and exact main10aa891f; no policy rollback or feature workaround.
The main-to-HEAD diff has 14 paths: five feature files and nine documents.
Removal of this additive internal slice is reversible; no schema migration.

Provenance objection resolved independently: current query evidence equals
prepared e5fca6bf exactly; builder15610c33 evidence is its exact byte prefix.
The retained root appendix explains the earlier whole-document comparison red.
The source-review document also equals89efaaf5. This was a provenance assertion
failure, not a feature failure or a new authorized repair attempt.

## Spec

No blocking finding under R1/R2. Only the module binding, issue Arc field and
Arc-only wrapper around the unchanged initializer alter consensus.rs. Existing
issue/found borrowing, capacity, start/read/shutdown behavior remain intact.
Bounds precede clock/admission; one trusted clock sample feeds pure eligibility.
One store guard supplies state and canonical membership; output remains <=641.
Exact-code eligibility checks foundation, cluster, range/skew, checked safety
margin, consumption and expiry without mutation or a freshness promise.

Both issue/read permits and store Arc move into the actual blocking closure.
Owners closed-check/registration share the mutex without await. Pre-submission
cancellation/deadline releases resources; submitted work retains them through
store waiting despite caller timeout/drop or async-owner abort. Guard, store,
read and issue release precede completion. Existing Orphaned semantics remain.

Three pure tables, four durable non-engine cases and one separately inventoried
single-voter engine case remain. Durable fixtures use actual apply/Owners and
EngineEnd::Absent; M1 checks actual post-abort capacity. M2 uses received query32
and normal TLS anchor/proof release before measuring actual server plaintext.
The engine path uses one start and actual founder/Mint/query/enroll/replay.
Inherited advisory: startup cancellation or assertion/API panic can bypass its
cleanup tail. No universal startup/panic cleanup guarantee is established.

## Security

No blocking finding. Internal fixed-size advisory query adds no listener,
dispatch, signing or membership/owner authority. Static errors disclose no
eligibility reason, input or nested store error. Existing code-anchor matching,
same-session endpoint key possession, E-1 first-redemption fence and signing
guards remain authoritative. No identity/admission implementation credit.

## Verification and disposition

Actual reviewer checks: byte identity/full binary diff/frozen main comparisons,
history inspection, evidence prefix/exact-document checks, strict UTF8/no-BOM/LF
for all 14 changed target blobs, and git diff --check against pinned main: PASS.
Ran the static quarantine guard exactly once:
python -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py
8 tests PASS, runner0.656s. No Rust/build/engine/mutation or network action.

Reused integration evidence: fmt PASS, guard8 and sampler60 each ran once;
nine retained execution hashes and saved 681-test/eight-suite inventory verified
by the integration agent. These are not fresh reviewer artifact/inventory runs.
Reused source evidence: 26 selected Rust tests, Clippy after one mechanical lint
correction, designated once-only compiled M1/M2 failures and exact restoration.
Cache last compiled M2; restored bytes do not prove post-restoration execution.
No new engine proof locally. Hosted-first owned_anchor_query_single_voter remains
required on all OS, future Windows <=660s, fresh exact-main green and all five
gates: three platform verify checks, ci-time-budget and independent agent-review.
This report does not post a published exact-head status or satisfy those gates.
PR175 documentation checkpoint may merge first without changing source credit;
its pending status and main health are supplied facts, not live remote checks.
Historical budget holds are superseded by supplied PR163 recovery, not erased.
M2 open/M3 unchecked; Sol3/3 and historical Opus0/2 unchanged. Hardware paths
remain **unverified on real hardware**. Only coordinator merges.

Proposed PROGRESS entry (not applied): Sol6.1/high independent local integration
review at2c599ffd PASS; five reviewed feature blobs/full diff and frozen main
including nine policy files/workflows/history match; provenance objection closed;
static guard8 and diff/encoding checks PASS. Execution evidence reused; hosted
query proof/exact-head gates remain. M2/M3, counters and hardware gaps unchanged.

Standards: 0 blockers; Spec: 0 blockers; Security: 0 blockers. Inherited cleanup
advisory only; local integration PASS grants no publication or merge acceptance.
