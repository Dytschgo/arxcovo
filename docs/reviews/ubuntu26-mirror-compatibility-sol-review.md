# Ubuntu 26 mirror compatibility: independent Sol review

OpenAI / gpt-6.1-sol / high, 2026-11-12. Single-agent Standards/Spec/security
review of the Opus design. **Changes required before implementation.** The
drop-only direction is acceptable in principle; this report grants no hosted
execution, acceptance, label or merge authorization. No subagents.

## Target and evidence identity

- Prepared runtime HEAD: `311d4cc8ed8bba681312ef17590ee872954a5715`, clean at
  start. Git top level is the supplied isolated T3 worktree, distinct from
  canonical `D:/Code/Arxcovo`; no canonical writes or checkout changes.
- Reviewed proposal: retained `261f5d7117337188d905b224994329a180880892`,
  original `2d4b18b946b50364f927beecde4856ba40e85988`. Proposal bytes are
  identical between them and the prepared HEAD. Against proposal base
  `8c7ad0ebc6892b1d108d920e91cd5a2f2ca12df4`, the triple-dot diff adds only
  this proposal; the log contains only its retained commit.
- Actual helper read from Git object `bd707ae8ca3ddcc500b5c4395ab23d546f480938`:
  SHA-256 `a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff`.
  Helper/tests are identical at #54 head
  `344c406663ad08b5f0497ecee9eb67a4cbd7e896` and prepared HEAD.
- Frozen acceptance read from Git objects, not substituted checkout files:
  original `2067e23`, corrected/accepted `d3bc3aa`, evidence `ed03f6b`,
  independent patch review `d1507d6`, retained #54 head `344c406`. Also read
  its independent Sol design review and the complete accepted A1–A6.
- Read AGENTS, KICKOFF, CONTRIBUTING, ADR0002, mirror contract, diagnostic
  result/JSON, observer vocabulary/source at `b1c7a4d` and `bd707ae`, and
  policy/evaluator source. Applied dytschgo-review and dytschgo-debug phases
  1–2 only. This is proposal review, not a historical root-cause investigation;
  phases 3–6 and implementation are outside scope. No GLOSSARY exists here.

## Standards

**CHANGES REQUIRED — 1 blocking finding.**

**S1 — P1 violates frozen A5, not merely a discretionary one-concern judgment.**
Proposal lines 269 and 284–288 say “A1–A6 stay frozen” while adding mirror
helper/tests to #54. Accepted `d3bc3aa` A5 permits exactly
`.github/workflows/ci.yml`, `scripts/ci-workflow.test.mjs`, and
`crates/arx-guest/tests/cli.rs` plus named documentation against `48d4916`.
`d1507d6` independently confirms that `tools/` is unchanged. A separate commit
does not put additional implementation paths inside that scope. AGENTS also
requires one-concern PRs; coordinator judgment alone cannot silently amend the
owner-specific frozen acceptance.

Correction: describe P1 as blocked pending an explicit prospective owner
decision permitting **both** the A5 scope amendment for
`tools/ci/ubuntu_mirror.py` and `tools/ci/test_ubuntu_mirror.py` (plus necessary
named evidence/docs) and their inclusion in existing #54 as its compatibility
prerequisite. Preserve numerical/performance gates, failed attempts and all
other exclusions. Record old/new scope and independent review before any
trial. If either permission is declined, stop this delivery route and report
the budget deadlock; do not create another stack or invent a waiver. This
report does not grant that decision.

Fowler smell baseline considered: no separate actionable smell in this design
diff. The counted pure seam is proportionate. Report-only scope overrides the
general PROGRESS update instruction for this assignment.

## Spec

**CHANGES REQUIRED — 3 bounded findings.**

**SP1 — Strict raw URL shape is not enforced by the proposed predicates
(lines 145–156).** `urlsplit` normalizes a leading C0 control and uppercase
scheme, and empty `?`/`#` delimiters yield false query/fragment booleans. Thus
parsed checks alone do not establish the stated strict raw ASCII URL shape.
The LDH regex also accepts ASCII `xn--` A-labels despite “IDNA ... do not
match.” Specify and test an exact raw-token grammar before parsing: lowercase
HTTP(S), ASCII LDH labels/lengths, exact path, no controls, delimiters,
userinfo or ports; explicitly exclude A-labels to honor the stated IDNA
exclusion. Retain all current rejection paths outside omission. These are
input-fence gaps, not evidence of contact with an omitted host.

**SP2 — Required tests miss the unknown tail and inherited priority cases
(lines 216–260).** Add an allowed entry, then an omittable entry, then each
foreign/malformed/Kernel entry: the entire plan must refuse with no publication
and byte-identical targets, including a direct source that would otherwise
change. Test Kernel anywhere, duplicate Kernel, malformed omitted-entry tags,
multiple omissions, 1 retained + 15 omitted success and 1 + 16 refusal. Check
retained order and exact bytes, never writing dropped targets. Explicitly
document existing duplicate priorities: the pinned parser accepts ties and
untagged entries; `apt_mirror_order` rejects ties only in tests. Do not promise
universally unique priorities or silently add tie rejection while claiming
unchanged semantics. Add mutations for early success after first omission,
unvalidated tail, and counting omitted entries as retained. Keep empty fallback,
second-run refusal, bounds and exact public output tests.

**SP3 — Separate observation from additional grammar assumptions (lines
142–143, 192–194).** The eight booleans and 152-byte size establish parsed
predicate class, not strict DNS/ASCII grammar or raw URL spelling. Replace
“exactly” with “observed predicate class, further restricted by the proposed
raw grammar.” Exact hostname and unchecked tail remain **UNKNOWN**. Likewise,
the public-source section establishes no explanation found in the inspected
objects; “runtime or unpublished provisioning” is an inference, not an
exhaustive causal finding. Keep synthetic fixtures explicitly representative;
they do not reproduce historical #54 bytes or native publication.

## Security disposition and required implementation evidence

The acceptable change removes narrowly classified unallowlisted entries only
from the indirect mirror list. It must never preserve, resolve or contact
them. Retain at least one validated HOSTS entry; count retained plus dropped
active entries toward 16 and retain 4 KiB input/output bounds. Keep HOSTS,
Kernel endpoint, source replacement behavior, keyring, signature checking,
trust options and Valid-Until unchanged. Existing unrelated third-party source
preservation remains current behavior, distinct from rejecting every unknown
mirror-list entry outside the exact omission grammar.

Keep `rewrite_mirror_list` returning bytes, carry the bounded count separately,
and emit only fixed updated/omitted count lines on success. Refusal must not
emit success/count/host/index/error data. Existing invalid-argument behavior
remains its fixed arguments token. Direct mode should report omitted 0.
No partial parser plan may be published. This does not invent transaction-wide
rollback: existing publication can partially complete on later I/O failure,
as the current mirror contract explicitly states.

Future tests must assert exact expected bytes and absent dropped targets, not
just success/count. All unsupported input must refuse even after an omission.
Owned-temp complete-plan tests and restored assertion-killing mutations are
required before actual implementation review; this design review does not
replace that different-provider source review or green required checks.

## Delivery blockers and precheck recommendation

Keep three ledgers separate, without resets:

- #54 performance: **2 of 3 failed; final attempt 3 UNUSED**. Run43248592153
  failed its install criterion; run43270363385 failed Ubuntu fetch setup.
- Fetch-path hosted invocations: **2 failed**, #54 job131364954122 and
  diagnostic job131461334918. Neither was a correction: **0 failed fixes**.
- This review: **one local Python process**, five existing parser tests,
  three representative refusal fixtures, canonical-output assertions and
  diagnostic/URL-edge assertions. No hosted invocation or correction trial.

Source policy and frozen acceptance are different constraints. `policy.mjs`
allows a successful ci-speed candidate strictly faster than the latest main
measurement; it does not encode A3, A4 or A5. `ci-budget.mjs` selects latest
completed main **push**, then retrieves attempt 1; candidate selection requires
`pull_request`, matching head SHA and head repository, then attempt 1. These
source files match the examined #54 objects. The retained main measurement is
906 seconds. Frozen A1 additionally requires <900; A3 requires Windows compile
<108 seconds and unrounded compile/Clippy <1.12; A2/A4 coverage and A6 independent
measured review remain required. Added Python tests do not excuse any existing
coverage/count requirement.

A standalone mirror fix cannot honestly be called a measured speed change.
Manual dispatch and reruns cannot replace the first-main-push baseline, and
an old candidate measurement does not qualify a new head. The legitimate
exception still needs a genuine speed candidate's own successful first PR
attempt and independent evidence. No empty push, false label, status override,
threshold relaxation, new stack beyond the existing product #53/#55/#56, or
silent A5 amendment is available here.

**Recommend conserving the extra fetch trial: P0 is not necessary by policy
or by the current evidence.** Fix the bounded design gaps and complete pure
source verification first; do not spend hosted capacity while S1 is unresolved.
A synthetic loop cannot guarantee a future runner's unseen tail, and an
optional manual run on different capacity cannot guarantee recurrence either.
If a separately authorized precheck is later justified by a specific remaining
native-only risk, record it as fetch invocation 3/correction trial 1, never a
performance attempt; a new failure remains in that ledger. Dispatch cannot
be selected by the current PR event filter. However, branch naming alone is
not evidence of timing-neutral caches: the workflow uses setup-node npm
caching, and this review has not verified the pinned action/service cache
visibility. Do not assert cache isolation or zero timing effect. No dispatch
or action-source network check is needed for this report's recommendation.

If frozen A5/one-concern rules prevent bundling, the concrete owner question is
whether to authorize that explicit limited scope amendment on existing #54
while retaining its real speed gates and remaining-attempt ledger. Otherwise
record blocked delivery until an owner-authorized legitimate route exists;
neither reviewer nor coordinator may manufacture an exception. No host
execution is authorized by either outcome.

## Verification actually run

One `python -I -B -` process loaded helper/tests directly from Git object bytes
into memory; it did not invoke helper CLI, `apply`, `_apply`, native filesystem
guards, network, or owner paths. Five existing parser tests passed: mirror
comments/untagged/second-run behavior, exact source allowlist, deb822 byte
preservation, insecure trust refusal, and malformed-input refusal.

Three synthetic cases (replace first, replace second without slash, append
untagged) each refused at line169 with exactly
`(false,true,false,true,true,true,false,false)`. Canonical 144-byte fixture
produced exact 195-byte `FALLBACK_MIRRORS`. Observer vocabulary was decoded from
AST literals without executing the observer; retained JSON confirms that tuple,
line169 and 152-byte mirror list. Four synthetic raw-URL normalization
assertions passed (C0 prefix, uppercase scheme, empty query, empty fragment).
No proposed classifier implementation was executed or written. These checks
are a feedback loop for the captured predicate class only.

Git identity/diff checks passed: original/retained proposal equal, helper/tests
equal at the three named heads, policy/evaluator equal at examined heads, and
proposal diff whitespace clean. No full Rust build/suite, mutations, hosted
checks or public-primary-source fetch was performed. Upstream provisioning
claims remain attributed to the proposal's retained public pins; none is
needed to infer or request actual runner input values. No actual hostname or
private runner bytes were requested, read or disclosed.

M2 remains incomplete. Native, installed and dedicated paths remain
**unverified on real hardware**. E_PROTECTION and ExecutionFailed remain
separate unresolved unknowns, with no retry. Local E1/E5 remain held. No
proposal, PROGRESS, source, CI or policy edits; no push, GitHub writes, labels,
reruns, account/service/cgroup/VM/experiment/private-state or owner operation;
no old Lab Manager or babysitter changes. Only this report is delivered.

Standards: 1 finding, worst blocking frozen-A5 scope conflict. Spec: 3 findings,
worst strict input-fence and unknown-tail coverage gaps. Changes required.
