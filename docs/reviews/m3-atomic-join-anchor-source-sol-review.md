# Atomic join-anchor: independent source review

Sol 6.1 high, 2026-11-17. Independent same-provider reviewer, separate from
builder and coordinator; no children. Assigned isolated worktree only.
Source-review PASS: no blocking Standards, Spec or Security findings. Ready for
coordinator publication review, subject to the outstanding gates below. This
report grants no publication, merge, production-security or hardware authority.

## Pins and scope

- Reviewed head: cd976bd80812a8b6c0ad296cbe83f9e4b2c2f69b.
- Feature source: 98b853058e022200cff9e9c71bfb32c791527702.
- Original WIP: 8a7b96293bc0ee43743227c90bcda63e167a110b.
- Base main: 380c0215d662f75c083015658a9f6f261808f2ea.
- Root fixture correction: 2e6b64eb262a73a54fab18b32ebd50c34138b672;
  continuation evidence edaf48fd; final classifier disposition cd976bd8.

Reviewed the complete base-to-head diff, all changed behavior/test files and
surrounding membership, registry, adapter, persistence and signing boundaries.
Feature-to-head crates/tools diff is empty. Read AGENTS, KICKOFF, CONTRIBUTING,
dytschgo-review/profile, full atomic brief, normative root corrections,
independent contract recheck, ADR0005 amendment, fixture disposition and evidence.
Current owner routing supersedes historical cross-provider wording. No attempt
counter or stop rule is reset by this review.

## Standards

Blockers: none. Production edits stay in the authorized kernel files; adapter,
shared membership codec, signer/verifier, transport, dependencies, workflows,
timeouts and limits are unchanged. The guard exception adds only the required
single-voter class and negative controls. ADR0005 documents the format break
and intentional effective capacity55. No migration, ingress or unrelated repair.

Advisory S1 (low, possible Duplicated Code/Primitive Obsession): enrollment.rs
Mint and enrollment/codec.rs decode repeat 4866/209/16384 reservation arithmetic.
A small shared named reservation predicate/constants would reduce future drift.
Current formulas agree and independently checked boundary tests pin them; this
is optional maintenance, not a prerequisite or authority for another source round.

Old assertion assessment: no semantic weakening found. The explicit apply-test
context, canonical membership setup, required anchor fields, shifted positions,
new code vectors/tags/versions and 209-byte strides are legitimate adaptations.
64-record success becomes the specified55-capacity equivalent; pending precedence
still leaves8 pending. Opaque membership acceptance becomes refusal plus canonical
success. The lower-term snapshot control removes incompatible origins so it still
reaches stale-snapshot validation. SQL/crash, dedup/races, revocation, clock,
privacy positive controls, CSR/leaf and founder assertions remain. Root's capacity
fix binds code to actual voter1/key1 instead of helper key250, preserving the
MembersFull, unchanged-state/counter and post-removal success assertions.

## Spec

Blockers: none. arxj2 is exactly270 characters with bounded parsing, checksum-v2
and anchor-bound proof-v2; verifier-v1 is unchanged. Redeem tag6 carries anchor
and reduced verifier, never caller-supplied origin. Sorted voter IDs and stored
key digests feed anchor-v1; learners/addresses are excluded; joint/absent/unbound
membership refuses. The independent literal SHA256 vector matches.

StateStore supplies full E and canonical E-1 membership under the existing lock.
Authenticated retained first Redeem compares the anchor before committed time,
pruning, allocation, consumption or binding mutation. AnchorChanged preserves
all enrollment bytes while applied E persists. Actual store A/B refusal has a
successful pre-B snapshot counterfactual; variants cover missing/key-changed
bindings, absence, identical joint configs and cosmetic membership changes.

Exact live retained replay bypasses new-consumption fencing after drift, preserving
full original E, original time and expiry. Proof/request/key/anchor mismatch and
Remove cannot allocate or resurrect membership. Tombstone eligibility uses fresh
effective time. Grant.applied_ms remains fresh; retained checks require original
<= fresh <= committed. The1000/60000/20000 regression checks SignerClockBehind
before fresh-field equality. Adapter origin index/term/equal-index checks use
otherwise valid own-node responses; current L still controls both issuer guards.
identity_commit reports E; founder position comes from its sealed existing ticket.

All state apply/open/peek/install and log append/decode paths use canonical
membership decoding. Legacy image1/2, database1, metadata1, ARXLE1 and tag2 refuse
Unsupported without application row/schema/version migration. Snapshot origin
bounds/full equality, original-time/lifetime, registry consistency and retained
reservation checks are present. Existing SQLite journal/locking mechanics are
not a byte-identity guarantee for refused opens.

Independent arithmetic: canonical membership3605 versus envelope4096; fixed
reservation4866 plus55*209=16361, versus56=16570; actual canonical max15870.
Pending89/consumed209, state16384 and row4163 remain consistent. NEW Mint reserves
after dedup/pruning; boundary replay succeeds, pruning reclaims room. Decoder
rejects over-reservation states even when pending bytes fit. Future consumption,
registry and membership growth fit the reservation; final persistence/SQL limits
remain. No reachable new oversize committed-apply trap found within these bounds.

The new required case uses one actual start, founder_init/found/enroll, MemRouter,
ephemeral CA and real CSR, with no three-node Cluster fixture through helpers.
It compares full E receipts at replay L>E, checks the CSR key and pauses after
commit before Remove; Revoked returns no leaf. Caller future resolution/drop,
hook release, router clearing, shutdown and store-release checks precede a pass.
Its35s work budget reserves5s cleanup within40s. Blocking startup timeout can
still fail with unresolved cleanup and later runtime teardown; no hard cleanup
guarantee is claimed. This test has NOT run locally.

## Security

Blockers: none in this internal kernel. Constant-time verifier comparison guards
first redemption and replay. Anchor/checksum equality concerns public data;
SHA checksums detect corruption, not authentication. Proof reduction precedes
await/proposal; command/state/log persistence carries verifiers, not bearer proof,
secret or private keys. Existing redaction and marker-positive privacy assertions
remain. No secret-bearing boundary or new signing authority was introduced.
Persistence precedes adapter responses; SQL failure remains unknown/poisoned,
never a known grant or unspent promise. Fresh retained/binding/leader checks,
CSR possession, CA fit, signer expiry/rollback and sealed subject checks survive.
Discovery, authenticated pre-proof ingress, receipt custody, transport/control
identity and trusted production clock policy remain separate unresolved work.

## Evidence actually checked and limitations

Actual reviewer work: local reads/diffs/status, one read-only Python static audit,
SHA256 reconstruction, literal-vector/arithmetic checks, and report validation.
No Cargo/Rust/build/engine/mutant execution, network/live status/comment/PR/push/
merge, install/cache deletion, admin/VM or old Lab Manager operation ran.

Reused execution evidence is attributed to builder/root, not reviewer runs:
original17 PASS and natural fixture1 FAIL/40 unrun; root setup-only correction
exact target1 PASS; authorized continuation remaining40 PASS/new6 PASS;
guard6/sampler60 PASS; final fmt/Clippy/list PASS after restoration. Initial
mechanical corrections, the continuation parser correction and root cp1252
wrapper display failure remain recorded, not hidden retries.

Recomputed all14 original artifact hashes printed in the evidence document,
all42 continuation manifest entries, original WIP source hashes, final four-file
manifest against checkout/feature blobs, and all four reconstructed mutant byte
hashes/restored baseline. Inspected mutant script/results/raw logs: all compiled,
exit100 at the designated assertion, respectively Enrolled vs AnchorChanged,
full origin(2,3,5) vs(2,3,3), Minted vs StateFull, and Ok vs SignerClockBehind.
No compiler/unrelated-failure kill credit. Baseline/restored enrollment.rs SHA256:
955b728624272640dcad8483e7fcdf728c8dfc527f9be6236ecc23ce5066f4f2.
Continuation manifest SHA256:
2909772dddf6a0b24c82785ff138cae5888fef9ede06c67cc073661f501a9474.

Saved inventory source98b85305 has663 cases. Independently checked named ignored/
required flags against saved listing: fixture28 ignored, founder13 required,
TLS10 pure +4 engine required, issuance1 required; sample8 names present.
Recomputed protected-inventory SHA256:
b4dfce89df3e73c6206458b1be9db08419aff2caa64f152b45b87f4b1bbde9e8.
The child's(13,7) wrong selector and root scratch namespace typo are static
classifier errors, not engine failures. Root saved classification closes them
without Cargo rerun. Sample runtime semantics remain attributed to guard/sampler.

Qualification Q1: the old malformed non-ASCII literal was mangled by default
Python decoding during remaining40, then restored to exact UTF-8 old prefix with
arxj2 format adaptation. Reviewer checked the bytes. That ONE test has no
exact-final-bytes runtime pass; no hidden rerun. Final fmt/Clippy/list compiled
the restored source; hosted ordinary CI must execute those bytes. No local
one-voter engine or quarantined adapter-engine execution is claimed.

Raw evidence: D:/Temp/arxcovo-anchor-c9616b-evidence,
D:/Temp/arxcovo-anchor-resume-a28b752, D:/Temp/arx-anchor-root-disposition.
Historical main CI44097456394 green/Windows551s is root-supplied prior-main
evidence only. All five hosted gates (three OS verify, ci-time-budget,
agent-review), required all-OS new tests/one-voter case, feature Windows<=660s,
fresh green exact main and final publication-head review remain coordinator work.
M2 open/M3 unchecked; Sol3/3 and historical Opus0/2 unchanged; hardware paths
remain **unverified on real hardware**. No source corrections are required by
this review; source-review PASS grants no merge/publication authority.

Standards: 0 blocking, 1 low advisory; Spec: 0 findings, Q1/runtime gates open;
Security: 0 blocking findings within the internal kernel scope.
