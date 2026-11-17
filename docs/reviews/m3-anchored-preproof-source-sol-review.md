# Anchored pre-proof: independent source review

Sol 6.1 high, 2026-11-17; separate reviewer from the builders, no children.
Same-provider independence is valid under the owner routing; no cross-provider
review is claimed. Assigned isolated worktree only; initially clean.

- Reviewed head: b98b3d93640133474fecce5fea3411c0a98c11ab.
- Actual main base: 59a665c9cda602d0b2939613656f2c98399f51f1.
- Feature source: 9b6c7e060e54d9ec8b44fc12e0715ab92412a3bb.
- Sole subsequent source change: test-driver correction
  f0c13d23d975d2d2d6e539d3752f210a7f7d8422. Later changes are docs only.

Read AGENTS, KICKOFF, CONTRIBUTING, dytschgo-review and active profile, the full
anchored brief, normative root R1/R2, independent contract recheck, ADR0005
including its internal-gate amendment, and chronological local evidence.
Reviewed the full base-to-head diff, commit history and surrounding membership,
enrollment/apply, StateStore, founder/issuance, shutdown and peer-verifier APIs.
This report reviews that exact prepared head; it does not review its own future
commit or a later publication head.

## Standards

Blocking findings: none. The ten non-doc paths exactly match the allowlist.
Reviewer byte checks confirm UTF-8/LF and equality to original source plus the
two-line driver correction. Existing enrollment/PKI/TLS tests, issuance,
adapter, Cargo.lock, CI workflow and sampler remain byte-identical to main.
The diff adds no Cargo, timeout, retry, permit or quarantine-policy change.
The owner routing supersedes CONTRIBUTING's historical Opus-review sentence.
The smell baseline yielded no actionable finding for this bounded concern.

The additive guard pins the required new test and parent binding, rejects
omission/rename/ignore/Unix hiding and additional starts or Cluster helpers.
Current test and helpers have one real start; old fixture28/sample8/founder13/
TLS14/atomic-issuance1 classifications remain intact. These lexical guards
support source inspection; they are not a sandbox against arbitrary Rust edits.

## Spec

Blocking findings: none. BoundVoters checks count/length before allocation,
caps at16 pairs/641 bytes and refuses suffixes, truncation, zero/nonincreasing
IDs and duplicate keys. Observation uses canonical BTreeSet order, refuses
joint/unbound membership and excludes addresses/learners. Hash domain, cluster,
count and pair bytes match the original derivation. Delegation changes neither
the existing E-1 apply fence nor replay, receipts, signer guards or schemas;
duplicate-key refusal agrees with the registry invariant.

The new all-OS test has one start/found/mint, observes actual state and decoded
membership under one store guard in joined blocking work, and drops its store
Arc before TLS. A genuine founder leaf/key serves the in-memory TLS endpoint;
the joiner has no client certificate or registry. Received proof is compared
before the trusted bridge invokes actual enroll with a real CSR. Replay checks
the original identity_commit. This is a probe, not CSR transport or durable
receipt custody. The guard makes the new case required on all OSes; it has NOT
been locally executed and needs first hosted execution.

The normal/deadline path resolves blocking handles, drops TLS and router
references, then requires shutdown Ok and weak store/log expiry before PASS.
Work uses35s plus5s cleanup from entry; existing operation budgets are frozen.
Unresolved startup explicitly fails with cleanup unknown, consistent with
non-abortable startup; no universal teardown promise follows.

Advisory P1: anchored_join_tests.rs uses unwrap/assert inside the work future
and unwrap on joined blocking results. A panic bypasses the explicit cleanup
tail. Normal completion and timeout cleanup are covered structurally, but
panic-path cleanup is not established. Do not describe this harness as proving
cleanup after every failure; any future hardening should preserve the original
failure while attempting cleanup. This does not produce false PASS evidence.

## Security

Blocking findings: none within this internal contract. The private TLS client
pins bounded root DER before trust installation, bounds leaf DER, rejects
intermediates and shares the canonical URI/cluster/SPKI parser at pki-parent
visibility only. Reviewer normalized-byte comparison confirms existing peer
verifier bodies unchanged. TLS1.3 signature checking delegates to real rustls
verification; TLS1.2, SNI, resumption and early data are disabled. Chain trust
does not yield AuthenticatedPeer, registry authority or owner authority.

The private connection/config has no arbitrary writer or raw-connection escape.
Only metadata survives calls. Completed THIS-connection TLS plus matching
anchor digest and endpoint node/key precede one proof32 release; query32 is
fixed and separately consumed. Gate/TLS/quota errors become Fatal; repeated
operations cannot restore authority. Released permits bounded drain only.
Partial proof-write failure is already Fatal and queued bytes cannot drain.

Incoming full offered lengths are checked with checked_add before read_tls;
only consumed bytes are charged and callers retain/resubmit the suffix. Output
copies only into the bounded caller slice, charges actual copied bytes and
cannot exceed remaining quota. Queue preflight after feed, cumulative128KiB
per direction, no phase reset, and terminal error handling cover partial queues.
The4096 application buffer is not claimed as a handshake cap. read_anchor is
bounded untrusted plaintext access in Queried only; acceptance independently
checks canonical bytes and session identity. No unsafe bypass or unbounded
retained input path was found. JoinCode::proof remains a trusted internal API,
so the wrapper's guarantees are scoped to its reviewed path, not crate isolation.

## Evidence checked, not re-executed

Reviewer ran only read-only git/source inspection and a Python byte/hash audit.
No Rust/Cargo/build/engine/mutant/guard/sampler rerun, network or public action.
Verified all10 original manifest artifacts, all5 first-continuation artifacts,
all16 completed-continuation artifacts, root scope hash and targeted-driver log.
Inspected commands, logs, mutation patches, runner stop/restore logic and source
reachability rather than relying on summarized kill flags.

- Original natural round: anchor2 and literal regression PASS; partial-write
  case failed in prepared query setup, actual0 versus32 before its intended
  assertion. Original red is retained; no unchanged rerun is claimed.
- f0 changes only quiescence to require both directional writes zero. Same32
  rounds, buffers, quotas and assertions. Corrected exact case PASS0.027s.
- Remaining22 PASS once. Thus26 cases have pass evidence across rounds, NOT
  one fresh26-case run. Guard7/sampler60/saved listing are reused.
- M1/M2/M3 each compiled once and failed precisely actual received proof32
  versus0 at preproof_tests.rs:228. exchange consumes query first and requires
  an actual32-byte server proof read before success; this is not marker search
  or callback-only evidence. M1's attacker list contains its real endpoint key;
  M2 retains the genuine digest/list; M3 pairs a valid anchored leaf with the
  other real signing key and changes only the new signature delegate. Intact
  M3 reaches the signature callback; mutated M3 delivers proof.
- Final fmt and locked/offline workspace all-target Clippy passed in retained
  logs. These are builder/root execution evidence, not reviewer executions.
- Initial generated-JSON classifier failure was closed statically without
  source repair; cp1252 display failure followed the saved natural red. One
  original mechanical compile batch and one separate root driver correction
  are disclosed. Artifacts support the recorded sequence, not an audit of
  unrecorded machine activity; no hidden rerun is inferred or authorized.

Verified SHA256 pins:

- Original artifact manifest:
  11bfeae6bf5407097eb3505cfa62ba64b9f6055b4d8e9de755a27929b1d477d2.
- First-red manifest:
  3405260e771d0c11f7cfa0d24b99bdf5f61ab7a7fa26ec7316e9ccf351f6628a.
- Root scope/inventory:
  9e15b5249775f42b0f08a93613037eca40d3b490df6608f86ccff2ea761fa8f3.
- Corrected-driver targeted log:
  a55511ec4b7afc9261ff791ffa209afcda122097a77708eb5773568b5fa2c701.
- Completed22/mutant manifest:
  b7e8fd126f6639851cda4d2abfc129e9e0ea8e84d9e3f9eeb3d21046d28cde6b.
- Current preproof.rs, saved baseline and each recorded exact restore:
  cda6d9b06610794f15bf377b22dd726eec7b530acea55fff99f61ba8b944e67e.

Raw artifact locations and individual patch/log hashes remain in
../evidence/m3-anchored-preproof.md; this review adds no execution credit.

## Publication disposition

PASS for coordinator publication preparation of this internal slice, with the
advisory and evidence limits above. No merge permission or status is granted.
Separate final exact-head review, five green normal gates, new all-OS hosted
founder proof, feature Windows<=660s and fresh exact-main green remain required.
Main CI44114182696 green609s/Windows604s is root evidence only, not a fresh
reviewer query or feature acceptance. The change is reversible private source
with no deployment/data migration; no public listener/query/discovery, custody,
admission/controller, owner/control or production-clock acceptance is implied.
M2 open / M3 unchecked; Sol3/3 and historical Opus0/2 unchanged;
hardware paths are **unverified on real hardware**.

Findings: Standards0 blocking/0 advisory; Spec0 blocking/1 advisory (panic-path
cleanup limits); Security0 blocking/0 advisory. No concrete source blocker found.
