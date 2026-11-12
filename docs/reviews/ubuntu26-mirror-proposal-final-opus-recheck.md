# Ubuntu 26 mirror proposal: final narrow Opus confirmation

Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-12. This is a
single-agent narrow confirmation with no subagents. It covers the Standards and
Spec axes, kept separate, for R1–R4 of the
[correction recheck](ubuntu26-mirror-proposal-correction-opus-recheck.md) and
for concrete regressions only. **Verdict: not passed yet. R1, R3 and R4 are
resolved. R2 is only partly resolved: the new raw-layer mutant list is
partly unsatisfiable (R2a).** One stale count (R1a) is minor. Both are
text-only fixes in the proposal, with no redesign. This report grants no
owner permission, implementation, hosted trial, label or merge.

## Target and identity

- Runtime HEAD `260798c49430f89e46cd1c19419301028d26f4e0`, clean at start.
  Git top level is the isolated T3 worktree, distinct from the canonical
  checkout.
- Reviewed target: Sol coordinator text fix `260798c` against parent
  `242faef8f176dafb37a7b87b93da0005e1d30206`. It touches `docs/PROGRESS.md`
  (1 line edited in each of 2 entries), the proposal (+45/−9) and the new
  `docs/adr/0021-ubuntu-mirror-list-omission.md` (+39). `git diff --check`
  is clean. All three files are ASCII with LF line endings and no CR.
- Prior Opus recheck: blob `09d58f3` is identical at `3e569f9`, `242faef`
  and `260798c`. Sol review: blob `2c075b4` is identical at `c60c14e` and
  `260798c`.
- Credit: independent review credit applies **only to the `260798c` text
  changes and the new ADR 0021**. The original Opus architecture keeps its
  independent credit from Sol review `c60c14e`.

## Resolution of R1–R4

| Finding | Status | Evidence |
| --- | --- | --- |
| R1 A5 path list | **Resolved**, plus minor R1a | Proposal lines 316–330 list each added path exactly: the helper, its tests, the helper contract `tools/ci/ubuntu-mirror.md`, ADR 0021, the evidence file, the five review files (including this file's exact path) and `docs/PROGRESS.md`. Already permitted A5 paths stay permitted. Lines 332–334 keep the separate PR54 one-concern exception. PROGRESS line 5 names both owner decisions, marks them pending, and records that the earlier incomplete question is superseded with no approval inferred. |
| R2 raw layer | **Partly resolved**, see R2a | Line 147 now requires `re.fullmatch` on the whole raw token, with no search, prefix match or normalized copy. Lines 277–284 add raw-layer mutants and drop the impossible parsed-check kills. |
| R3 PROGRESS | **Resolved** | Line 8 records Sol high `c60c14e` (corrections requested), Sol medium correction `5fcac4f`, and Opus high recheck `3e569f9` (clarifications required). Line 5's "final narrow independent confirmation is next" no longer contradicts it. The `260798c` provider and effort (Sol medium) appear in the proposal checkpoint and the ADR author line. |
| R4 ledger | **Resolved** | Lines 352–355 say a separately approved precheck is fetch-path hosted invocation 3 and correction trial 1, never a performance attempt. A fetch failure during performance attempt 3 counts in both the performance and fetch ledgers. |

ADR 0021 says `Status: proposed`, with the owner's A5-scope and PR54 bundling
decisions pending. It records the drop-only behavior, the bounds, the
count-only output, the availability tradeoff and the same owner fences. It
says no implementation is accepted and no source, system, workflow, policy,
account, service, VM or experiment changes. It authorizes no implementation
and no hosted trial. Its summary matches proposal lines 142–186.

## Standards

**1 minor finding.**

**R1a (minor): proposal line 338 still says "those two paths".** The list
above it now adds three non-`docs/` paths: the helper, its tests and the
helper contract. Line 335's "No additional source path" is still accurate,
because the contract is documentation. Change line 338 to point to the exact
list, so the owner does not read a count that conflicts with it.

Smell baseline: nothing actionable in this docs diff. The ADR repeats the
proposal on purpose, as an ADR summary that links to the proposal. That is
not Duplicated Code.

## Spec

**1 bounded finding.**

**R2a: six of the nine new raw-only mutants cannot fail any test.** Lines
281–282 say "Each must be caught by its named shape-fence case." Rules 3, 4
and 6 still apply after the raw grammar, as the proposal requires, and they
reject the same inputs:

- a port or userinfo, through `netloc == hostname`;
- another path, through the path check;
- label or host length, through the rule 4 regex and the 253 limit;
- an `xn--` label or a non-ASCII label, through rule 4.

So loosening only the raw clause changes no outcome. This is the mirror image
of the original R2. Lines 282–284 exempt only the opposite case: removing a
parsed check while the raw grammar still rejects. As written, the implementer
must either fail the mutation gate or delete the defense-in-depth parsed
checks to make these mutants killable.

A pure local model checked this. One `python -I -B` process (3.12.10) ran
in the session scratchpad and encoded rules 1–6 as specified. Its corpus
covered the valid cases, uppercase scheme, a C0 prefix, empty `?` and `#`,
port, empty port, userinfo, `/other`, `/ubuntu-ports`, a 64-character label,
a 254-character host, `xn--` and a non-ASCII label. Results:

- killed: uppercase scheme, C0 prefix, and prefix match through empty `?`/`#`.
  `urlsplit` normalizes these, so only the raw layer catches them;
- equivalent (no outcome change): port, userinfo, path, length bounds,
  `xn--` and non-ASCII.

The script is not retained in the repo (SHA-256 `39b60b6a…7fa18`).

Correction: keep the three raw-only mutants. For the other six, mutate the
raw clause **together with** the parsed predicate it duplicates, as one paired
mutant, killed by the named fence case. Alternatively, list those six as
documented equivalent mutants. Then make the equivalence sentence cover both
directions. This changes no path, permission or design.

No other regression found. The rule text, bounds, tests 1–10, the
trust-preservation argument and the retained sequencing are unchanged apart
from the reviewed hunks.

## Recommendation

**Not passed yet.** Fix R2a and R1a in the proposal. Both are text-only edits
to a path that is already in the requested list, and neither changes what the
owner is asked to decide. A narrow confirmation of those two hunks is enough
after that. Once they are fixed, the design is technically sound **subject to
the owner's two prospective decisions** (the A5 path amendment and PR54
bundling). Neither decision is granted or implied here. The coordinator
decides when to ask the owner. This report sends no owner question.

## Verification actually run

- Git identity, diff, stat, `--check`, blob-identity and line-ending checks,
  as listed above.
- One pure Python model process in the scratchpad. It did not load or run the
  helper, its CLI or `_apply`, and it touched no network, native path or owner
  path.
- No source, proposal, PROGRESS or ADR edit. No build, test suite, CI run,
  push, GitHub write, label or rerun.

## Retained boundaries

- Ledgers are unchanged. Performance: 2 of 3 failed, attempt 3 unused. Fetch
  path: 2 failed invocations, 0 failed fixes. Main baseline: 906 s, with no
  override. All numerical, performance and coverage gates are unchanged.
- M2 is incomplete. Native, installed and dedicated paths remain
  **unverified on real hardware**.
- No account, service, cgroup, VM, experiment, private state or owner
  operation was touched. The old Lab Manager and the babysitter were not
  touched.

Standards: 1 finding. The worst is R1a, the stale "two paths" count. Spec: 1
finding. The worst is R2a, six raw-only mutants that cannot be killed because
parsed rules 3, 4 and 6 still reject.

## Confirmation of R2a and R1a fix, 2026-11-12

Anthropic / claude-opus-5-5 / high, review sub-agent, single agent, no
subagents. Narrow two-finding confirmation only. All text above is the prior
report, kept verbatim. **Verdict: R1a is closed. R2a is closed in mechanism,
with one minor residual (R2b): the plan requires a non-ASCII paired mutant to
be caught by "the named shape-fence case", but test 3 names no non-ASCII
label case.** This is a one-clause text fix to a path that is already
requested. It changes no path, permission, design or ledger, and nothing the
owner is asked to decide. This section grants no owner permission,
implementation, hosted trial, label or merge.

### Target and identity

- Runtime HEAD `d4d9c48ee196f938581a86c24743ab5d346cf71c`, clean at start.
  Git top level is the isolated T3 worktree, distinct from the canonical
  checkout.
- Reviewed target: Sol coordinator text fix `d4d9c48` against parent
  `e59e417e1b4ca7301a583d2520e3e46267ec4db8`. It touches
  `docs/PROGRESS.md` (+1, the new "Final text checkpoint" entry, line 8) and
  the proposal (+14/-9: the mutation section, lines 277-289, and the A5
  sentence, line 343). `git diff --check` is clean. Both files are valid
  UTF-8 with LF line endings, no CR and a final newline. Every added line is
  ASCII.
- Prior report: `447c72d` and `e59e417` here carry the same report blobs as
  `202d89e` and `ac8c98d` on `t3/ubuntu-mirror-review` (`ce38161` and
  `2d3c507`). `d4d9c48` does not touch this report.
- Scope check: the diff changes no rule text, bound, required-test text,
  requested path, permission, sequencing or ledger row.
- Credit: independent review credit applies **only to the `d4d9c48` text
  delta**. The original Opus architecture keeps its independent credit from
  Sol review `c60c14e`.

### Standards

**0 findings.** R1a is **closed**. Line 343 now says "those three non-docs
paths and named docs". The list at lines 324-334 adds exactly three paths
outside `docs/`: the helper, its tests and `tools/ci/ubuntu-mirror.md`. The
rest are named docs. Line 340 ("No additional source path") stays consistent
under the prior review's reading. The PROGRESS line 8 checkpoint is accurate.
It credits `ac8c98d` with closing R1, R3 and R4, names coordinator Sol medium
as the corrector, and keeps both owner decisions pending. It infers no
approval.

### Spec

**1 minor finding (R2b), residual to R2a.**

R2a is **closed in mechanism**:

- Lines 277-279 keep exactly the three raw-only mutants: uppercase scheme,
  C0/DEL prefix, and prefix match through an empty `?` or `#`. The prior
  pure model showed that only the raw layer catches these, because
  `urlsplit` normalizes them. A DEL-only prefix would also fail parsed
  rule 2, but the C0 case still kills the combined mutant.
- Lines 280-283 pair each of the other six behaviors: the raw clause is
  loosened together with every matching later guard. Port and userinfo pair
  with `netloc == hostname`. Another path pairs with rule 6. Label and host
  length pair with rule 4's `{0,61}` and the 253 limit. `xn--` and non-ASCII
  pair with rule 4. Once both layers are loosened, these inputs reach
  omission instead of refusal, so the outcome changes. The only earlier gate
  is the line-166 token grammar, which does not reject any of them.
- Lines 284-289 make the equivalence exemption symmetric, with no kill
  credit. They keep both production layers and forbid deleting a guard to
  manufacture a kill. This resolves the mirror-image problem without
  touching the design.

**R2b (minor): the non-ASCII paired mutant has no named case to kill it.**
Test 3 (lines 236-243) names port, userinfo, `/other`, `/ubuntu-ports`, a
64-character label, a 254-character host and `xn--` labels. It names no
non-ASCII label. Rule 4 already refuses non-ASCII names, but no test
exercises that refusal. One `python -I -B` 3.12.10 process in the session
scratchpad checked the path for `http://` plus a `U+00E9` label under
`.ubuntu.com` and `/ubuntu`:

- It is one token for the line-166 grammar.
- `urlsplit` succeeds, `netloc == hostname`, the path is `/ubuntu` and the
  length is within 253.
- Only the rule 4 regex (and so the raw grammar) rejects it.

So a paired mutant that widens both character classes to accept non-ASCII
would omit the entry, and an implementer who follows test 3 as written can
see it survive. The underscore and uppercase-host cases do not reliably
catch it. Either the "must" in line 285 fails, or the implementer has to
invent the case. This gap existed before `d4d9c48`. The previous section
missed it: its model corpus included a non-ASCII label, but it did not check
that test 3 names one. The pairing made the gap visible; it did not create
it.

Correction: add "a non-ASCII (UTF-8) label" to the test 3 list. This edits
only the proposal, which is already a requested path.

Observation, not a finding: "label/host length" covers two behaviors. Test 3
names a case for each, so both are killable whether the implementer runs one
combined paired mutant or two.

### Recommendation

R1a is closed. R2a is closed apart from the R2b clause. With that clause
added, the proposal is **technically sound subject to the owner's two
prospective decisions**: the A5 path amendment and PR54 bundling. Neither
decision is granted or implied here. R2b does not change what the owner is
asked, so it does not block the coordinator from asking. It must be in the
proposal before implementation. The coordinator can confirm it with a
one-line diff check, or the independent implementation review can verify the
test exists and the mutant is killed. This section sends no owner question.
The earlier incomplete scope question stays superseded.

### Verification actually run

- Git identity, diff, numstat, `--check`, blob identity, UTF-8 validity,
  line-ending, final-newline and added-line ASCII checks, as listed above.
- A read-only `grep` of `tools/ci/ubuntu_mirror.py` to confirm the earlier
  gates: UTF-8 decode, the line-166 token grammar and `urlsplit`. It was not
  imported, executed or edited.
- One pure Python process in the scratchpad, using only stdlib `urlsplit`
  and `re`. It did not load the helper, its CLI or `_apply`, and it touched
  no network, native path or owner path.
- No proposal, PROGRESS, ADR or source edit. No build, test suite, CI run,
  push, GitHub write, label or rerun.

### Retained boundaries

- Ledgers are unchanged. Performance: 2 of 3 failed, attempt 3 unused. Fetch
  path: 2 failed invocations, 0 failed fixes. Main baseline: 906 s, with no
  override. All numerical, performance and coverage gates are unchanged.
- M2 is incomplete. Native, installed and dedicated paths remain
  **unverified on real hardware**.
- No account, service, cgroup, VM, experiment, private state or owner
  operation was touched. The old Lab Manager and the babysitter were not
  touched.

Standards: 0 findings, and R1a is closed. Spec: 1 minor finding. The worst is
R2b: no named non-ASCII shape-fence case exists for the required non-ASCII
paired mutant.
