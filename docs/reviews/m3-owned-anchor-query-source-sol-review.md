# Owned anchor query: independent Sol source review

Sol 6.1 high, 2026-11-17. Independent reviewer separate from builder; no
children. PASS for the bounded local source scope. No blocking Standards,
Spec or Security finding. This same-provider review is valid under current
owner routing; it makes no cross-provider claim or publishing authorization.

## Identity and scope

- Prepared review HEAD: 8b5fcd2ac1dc06e1b730e9e3f74152ab6f8b9bfc.
- Feature source: f9d0a413d02737214af62601aa345c90634a4e26.
- Builder evidence: 15610c3335a0411ca45f5e9c7de6c8aa71dcbadd.
- Prepared contract: 2bee2043c0f53ac2f173d166f220ae59ec1b0f4f.
- Source main: 3022d601f09f24b0e5f092a33f2e9343b500471d.
- Independent contract recheck: d56a4a2268fb9c9af6266e0b46d865a4f2f9a745,
  retained as 3b45bb07.

Read AGENTS, KICKOFF, CONTRIBUTING, review skill/profile, full owned-query brief,
normative root R1/R2, independent contract recheck and latest ADR0005 amendment.
Reviewed the complete five-source-file change against source main and prepared
contract, plus surrounding admission, adapter, store, membership, redemption,
pre-proof and shutdown APIs. Worked only from the assigned isolated worktree;
read external saved artifacts at the supplied paths. Only this report is edited.

## Standards

Blockers: none. The five allowed source paths are exactly consensus.rs, new
consensus/anchor_query.rs, new consensus/anchor_query_tests.rs,
enrollment/anchor.rs and tools/ci/test_windows_cluster_quarantine.py.
Programmatic comparison proves consensus.rs contains only the module binding,
Arc<Semaphore> field and Arc::new around the same MAX_ISSUING initializer.
Existing issue/found borrowed try_acquire semantics and capacity4 remain intact.
The old anchor implementation/tests are byte-identical after removing only the
now-unfulfilled encode dead-code expectation. Old guard content is preserved
verbatim around the additive query block. Other source, Cargo, workflows,
StateStore, adapter, fresh reads, shutdown, issuance, TLS/pre-proof, old tests,
timeouts, retries and capacities are unchanged. No baseline smell warrants an
unrelated refactor. Local removal of this additive slice is reversible; no
schema, persistent-state migration or public endpoint is introduced.

## Spec

Blockers: none under R1/R2. Query length32 precedes copying, SignerClock,
admission and allocation. The entry READ_BUDGET deadline precedes the one
trusted synchronous clock sample. Issue then read owned admission is immediate;
failed read admission releases issue. Owners closed-check, reap, store clone
and owner registration share its mutex without an await. Before submission,
cancellation/deadline permits cleanup without blocking work. After submission,
the actual blocking closure owns store Arc and BOTH units, including while
waiting for the mutex. Computation's inner scope releases its guard on errors;
explicit Arc/read/issue release precedes completion. Only the caller oneshot
wait is timed. Owner abort cannot reclaim submitted resources early. There is
no arbitrary callback escape or store ownership cycle.

One guarded StateStore image supplies state and decoded canonical membership.
Pure pending_voters checks founded/cluster/time range, rollback skew, checked
effective-time safety margin, exact record, consumption and expiry; missing,
joint or unbound membership refuses. Existing observe/encode bounds output to
641 bytes. No scan, mutation, pruning, applied-time advance or new authority.

Three pure tables cover literal canonical output, maximum16, learner/address
invariance, time boundaries, eligibility and state immutability. Four durable
async tests use actual StateStore.apply outcomes and real Owners, permits,
Weak/Released handles with raft=None; absent log release and EngineEnd::Absent
are honest. Actual closure-entry counts follow a confirmed held store lock.
Four dropped callers retain issue0/read4 and four extra store references;
read saturation returns issue immediately. Real timeout/shutdown tests check
Orphaned and wait for actual AbortHandle.is_finished before read7/issue3 checks.
Saved assertions propagate after gate/thread joining and resource/store release.
The test-only registry holds AbortHandles, no stores or permits; entries are
removed on passing paths and current workloads bound their accumulation.
Bounds, canceled/expired no-submission, closed Owners and poisoning paths are
covered. No successful path reports PASS with gated work unresolved.

M2 receives query32 through TLS, invokes the real query on those bytes and sends
ANY returned list through normal accept_anchor/release_proof before measuring
actual server proof plaintext. Synthetic leaf SPKI is the actual committed
founder voter; no fake ticket, always-OK registry or replica seeding.
The separately inventoried hosted case has ONE start, actual founder/Mint/query/
TLS proof/enroll/replay, retained blocking work, router release and shutdown
checks within cooperative35s work plus5s cleanup. It remains UNRUN locally.

Advisory: startup cancellation or an assertion/API panic can bypass the engine
case's cleanup tail. This is an acknowledged unresolved-failure limitation,
not a universal startup/panic cleanup guarantee; do not promote it to one.

## Security

Blockers: none for this internal advisory hint. Static errors reveal no input,
eligibility reason, nested store error, secret or list in Display/Debug.
Eligibility maps to Refused, admission to Busy, mutex/decoder/owner/store absence
and timed reply failure to Unavailable. No listener, dispatch, logging, signing
or membership authority is added. Applied-state coherence on followers proves
no quorum freshness, unspent code or real-time expiry. Existing code anchor,
same-session endpoint-key proof, E-1 first-redemption fence and both signing
guards remain authoritative and unchanged. Internal query success is not join
acceptance. Hardware paths remain **unverified on real hardware**.

## Evidence: actual inspection versus reused execution

Actual reviewer work: full diff/API inspection; exact five-path/frozen-remainder
comparisons; recomputed all nine retained artifact hashes against the committed
evidence; both restored source hashes matched current files and feature blobs.
Parsed the bounded saved inventory's first JSON object after its Cargo stderr
prefix, with no trailing data. Listing succeeded:681 tests, eight suites.
All eight additions are present and unignored:3 pure,4 local durable,1 engine.
Existing Windows fixture28/sample8 remain ignored as specified; founder13,
required TLS14 (10 TLS tests plus3 engine-required and1 directory pure), atomic1
and anchored-proof1 retain their classifications and unchanged source.
Reviewer ran the static quarantine guard once:8 tests PASS (runner1.090s),
and git diff --check. No Rust/build/engine/mutant rerun or remote action.

Reused builder execution, inspected raw logs/patches/metadata: initial fmt;
Clippy initial stale expectation then PASS after one mechanical correction;
26 selected tests PASS (7 new,19 compatibility), guard8, sampler60 and inventory.
M1 compiled and ran once, designated post-abort read assertion8 versus7; issue3
was checked first. M2 compiled and ran once, designated server proof32 versus0.
Both failures occur after the specified cleanup path, not a token/compile oracle.
Restoration after EACH is recorded with equal baseline/restored SHA256:

- Query: 350b634354c1d12de617dadbce3c828bf215efb77f7ed9ddaed99ba6ecef234a.
- Anchor: 0f0f185d2f4b604f879738dcb84b046a0f282af6ad81ccfd7bf0d1ca3a65195b.

Raw builder artifacts:
C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-6959c072-46da01a5cc9eee99/target/m3-owned-query-6959-evidence.
Root inspection.json/fmt.log under D:/Temp/arx-owned-query-root record final
exact-source fmt PASS1.282s after lint correction; empty-log hash recomputed.
These executions are reused, not reviewer runs. The retained cache last compiled
M2; source restoration establishes bytes, with NO post-restoration passing
behavior replay. An eventual hosted run must compile the restored exact source.

## Local disposition and remaining gates

Local source review is complete; feature MUST remain unpublished under the
active905s budget hold. Root's functionally green main attempt2 is reused root
proof only, not query hosted evidence or first-attempt budget recovery. PR163
remains priority NEXTMERGE: supplied exact-head PASS22a09eae, platform pending,
budget failing; no live status was queried here. No policy activation, public
comment/status/push/watch/link/merge or administrative action is authorized.
Independent published exact-head review, all five required gates, first hosted
new engine proof on every OS, Windows<=660s and fresh green main remain future
prerequisites after policy recovery. Only coordinator merges. M2 open, M3
unchecked, exhausted Sol3/3 and historical Opus0/2 counters remain unchanged.

Standards:0 blockers; Spec:0 blockers; Security:0 blockers. One cleanup advisory;
local source PASS does not grant publication, merge or hardware acceptance.
