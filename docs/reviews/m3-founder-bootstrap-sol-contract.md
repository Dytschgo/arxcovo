# Founder bootstrap contract: Sol review

Target: Opus brief `3b4919f`, based on `b52cd9c`. Reviewer: root Sol,
medium, 2026-11-16. Static source/API review only; no build or test run.

Verdict: changes requested before implementation. The founder-key dependency
is real and the selected single-voter slice advances M3. No source is authorized
by this review. Standards and Security have no separate blocking finding;
Spec has the two API/invariant blockers below.

## B1: sealing module is inaccessible from the proposed trait location

The brief defines `LeafSubject` in `pki/ca.rs`, with `sealed::Sealed` private
to sibling `consensus/issue.rs`. Rust module privacy prevents the CA module
from naming that private module. Make the visibility and ownership compile
without opening ticket creation to other cluster modules. One suitable design
puts the public-to-cluster trait in `issue.rs` alongside its private sealing
module, and imports the trait in `ca.rs`. Only issue and its founder child
implement the seal. Existing IssueTicket accessors already use this visibility.
Pin the intended inability of an unrelated module to implement the trait.

## B2: founder allocation invariant is only a convenience, not enforced

`ConsensusNode::start` accepts arbitrary `Init`. Providing `founder_init()`
does not force callers to use it. `Found` requires every founder ID to be less
than the configured next free ID, and leaves that next ID untouched. Therefore
node 1 started with first_free=4 can successfully found but its first joiner
will be node 4, contradicting this brief's invariant and F5's node-2 result.
A first_free=1 node instead initializes Raft and then rejects Found, leaving
an avoidable unusable initialized node.

Enforce the initial allocation contract before initialize, using a restricted
constructor or checked immutable startup metadata with an honest relationship
to persisted state. Keep the generic existing fixture start API compatible.
Add wrong-first-free tests for both 1 and 4: refusal before initialize, no vote
or log, with an explicit error. Do not rely on a caller convention. Include any
required accessor or field in the exact allowlist and explain restart behavior.

## Test/wording corrections

- F3's node-id-2 case cannot subsequently succeed on the same node, since id is
  immutable. Check it remains uninitialized; the corrected follow-up belongs
  only to the bad-CSR/CA cases on node 1.
- Specify the actual bounded OpenRaft leader-wait API and exact metrics predicate;
  do not leave an unverified API choice to the builder under a no-rerun rule.
- Pin the absolute-budget outer wrapper and final elapsed check, matching enroll.
  Inline crypto is not preemptible; an elapsed call cannot release a certificate.
- State how F7 signals each guard boundary before changing the fake clock so a
  test cannot pass by racing or reading time before an await.
- Clarify that restart proof covers registry and Raft state, not recoverable CA
  or founder key custody. Failed initialization recovery must never delete or
  overwrite the old directory as an automatic action.

## Evidence and limits

Read ca.rs sign_leaf/validity arithmetic, issue.rs ticket/guard/enroll ownership,
ConsensusNode::start and registry.rs Found validation. No source was edited.
The existing ephemeral CA subtracts 120 seconds from its input time, while the
leaf starts 60 seconds early; the proposed 30-day-plus-60-second boundary is
consistent with that arithmetic. No cryptographic or hosted result is claimed.

PR161 must merge and exact-main CI must be green before founder source starts.
PR159's holds remain independent. M2 open, M3 unchecked, eight Windows
quarantines unchanged; hardware paths are **unverified on real hardware**.
