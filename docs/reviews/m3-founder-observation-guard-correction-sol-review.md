# Founder observation guard correction: independent Sol review

Reviewer: OpenAI / Sol 6.1 / high, 2026-11-18; no children.
Independent of root builder; narrow static correction review only.
Published PR177 base: 3c2d04c3c9349e4acf5538a470a704c95413442f.
Reviewed head: 323d20ed92749198e386bc6763ce9562ca2a2f74 (initially clean).
One commit: test(cluster): keep founder probe within issuance guard.
Actual base..head diff: only found.rs and docs/PROGRESS.md.
Read AGENTS, KICKOFF, CONTRIBUTING, review skill, probe plan/patch,
prior safety/hosted reviews, local observation result and actual source/callees.
Owner Sol routing supersedes CONTRIBUTING's historical Opus requirement.

## Standards

PASS: zero blocking findings; no substantive smell finding.
One macro definition each replaces the split test/non-test definitions.
Only observer statements acquire cfg(test); the observer module and existing
found_tests binding retain cfg(test). No tests, bodies or assertions are hidden.
Policy, guard, quarantine, workflow, budgets, retries, clocks, dependencies and
all other source files are unchanged against the published head.
PROGRESS correctly qualifies the new production binding and pending review.
No architectural deviation, attempt reset or unauthorized source fix is present.

Actually ran: base..head diff/scope and git diff --check (PASS);
in-memory macro/body and unchanged-suffix comparisons (PASS);
retained patch reconstruction with exact context and recorded compiled-source
SHA256 checks (PASS); formatting token comparison preserving literals (PASS).
The first token-comparison helper rejected lifetime syntax; its corrected
lexer passed. This was a review-helper failure, not Rust execution evidence.
Reconstructed compiled found.rs matches published source except formatting;
found_tests.rs matches the recorded compiled probe byte for byte.
Removing the four statement attributes restores the published test macro
bodies exactly apart from whitespace; observer and all call sites are unchanged.
Static guard ran exactly once:
python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py
PASS: 7 tests. Guard and mutation coverage were not edited.
No Rust, fmt, build, compilation, test engine, F6, network or public write ran.
Root's fmt/diff passes are retained evidence, not a new fmt run by this review.

## Spec

PASS: zero blocking findings for the seven actual call sites.
Production expansion is now { let result = WORK; result }, NOT literal WORK.
That transformation is not universally lifetime/coercion/drop equivalent for
arbitrary Rust expressions. Acceptance here depends on these concrete uses.
Macro hygiene prevents result capture; no closure, async block or new await is
introduced. Each WORK evaluates once. All concrete Result types are fixed by
callee signatures/mappings; none relies on expected-type coercion or returns
a borrow into an initializer temporary. FoundError is Copy with no destructor.

Call-site accounting (all in found.rs::ConsensusNode::found):
- preflight: Result<Pre, FoundError>, Pre is owned Copy data. Its future/receiver
  cleanup stays within the existing await; the result moves to the same outer ?.
- initialize: Result<(), FoundError> after map_err(init_refusal). The BTreeMap
  and addr still move into initialize in the same order, before the same await.
  Future/argument temporaries cannot escape through the owned unit/error result.
- leaderwait: Result<(), FoundError> after the same metrics await/error mapping.
  The wait handle/future can end at the inner let semicolon instead of the outer
  ? statement boundary. Between those boundaries only moves and Copy error
  propagation occur: no user callback, clock, await or resource release is
  inserted. Cleanup remains before the next phase or enclosing work locals.
- proposal: owned Result<ClientWriteResponse<Arx>, FoundError>. Command/member
  construction, t0 use, await and mapping retain order. FoundMembers' inner ?
  still exits the SAME work async block, skipping exit on failure in test mode;
  it does not return from the macro block. The outer ? is unchanged. Response
  ownership moves through result into resp without cloning or early destruction.
- guard1 and guard2: each Result<(), FoundError>, borrowing the existing ticket
  only during its original await. Neither result retains that borrow or a guard.
  The two calls still bracket signing and the same SignerClock reads.
- final: owned Result<IssuedLeaf, FoundError> from the identical timeout match.
  timeout_at still owns work; its await and final Instant predicate stay ordered.
  Match scrutinee cleanup and discarded late certificate remain inside that
  initializer, before the outer issuance permit is dropped. Successful leaf
  ownership moves out; it is not destroyed by the added result binding.

The six phase initializer temporary scopes may end earlier within their original
statements. No returned borrow, custom conversion, intervening await or externally
observable operation makes that boundary change material at these sites.
Rust 2024 tail temporaries in the final match also stay before enclosing permit
cleanup. Cancellation can occur at the same existing awaits; no new suspension
or owner-worker cancellation is added. The result binding is initialized only
after its WORK completes. Owners/read/issuance permit lifetimes are preserved.
This is source-level semantic reasoning, not identical machine code/timing or
compiler verification. Test observer overhead remains as previously qualified.
No material production semantic risk found; no identity alternative is required.

Advisory documentation clarification: the prior plan/reviews and earlier
PROGRESS identity statements describe split-macro history only. For this head,
use this call-site-qualified binding analysis; do not reuse literal-token
identity as current evidence. The latest PROGRESS qualification is accurate.

## Security

PASS: zero blocking findings. Observer code and F6-only capture are unchanged.
Fixed-cap scalar/static allowlist, payload suppression and post-return emission
remain intact. No policy/quarantine weakening, new log content, unsafe operation,
secret exposure, owner administration, old Lab Manager or VM operation occurred.

## Readiness

READY for root disposition of this source correction and a new normal PR push
only after that disposition. This review performs no push and grants no CI/F6
rerun, dispatch, merge, fix/stability credit or additional repair attempt.
Per supplied CI44203071298 evidence, every OS rejected cfg(not(test)) in the
unchanged assert_issuance_required before Rust compile/tests/F6; no hosted phase
observation occurred. Workflow ordering and guard source support that account;
no hosted log was fetched here. No Rust/build/test/engine ran after correction.
Main09134085 remains RED; PR176 held; Sol3/3 and historical Opus0/2 unchanged.
M2 open, M3 unchecked; hardware paths **unverified on real hardware**.
Coordinator alone handles publication and required exact-head checks/review.
Only this <=120-line ASCII UTF-8 LF review is committed; PROGRESS is not edited.
Standards: 0 blockers; Spec: 0 blockers; Security: 0 blockers.
