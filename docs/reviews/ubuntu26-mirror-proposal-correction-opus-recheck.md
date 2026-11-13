# Ubuntu 26 mirror proposal correction: independent Opus recheck

Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-12. This is a
single-agent narrow recheck with no subagents. It covers the Standards and Spec
axes, kept separate. **Verdict: changes required, two bounded textual
corrections and two minor ones. Not yet ready for technical acceptance.** No
redesign is needed. This report grants no owner permission, implementation,
hosted trial, label or merge.

## Target and identity

- Runtime HEAD `5fcac4f23aa7decd03f7f6f91c92e8f1b1e5529f`, clean at start. Git
  top level is the isolated T3 worktree, distinct from the canonical checkout.
- Reviewed target: Sol coordinator correction `5fcac4f` against parent
  `b3105d5331bc8867ad171539dd72d6728a96a0e6`. `git diff --stat` touches only
  `docs/PROGRESS.md` (1 line) and
  `docs/reviews/ubuntu26-mirror-compatibility-proposal.md` (+71/−34).
  `git diff --check` is clean.
- The original Opus proposal `2d4b18b` is byte-identical to retained `261f5d7`.
  The Sol review `c60c14e` is byte-identical to retained `b3105d5`
  ([review](ubuntu26-mirror-compatibility-sol-review.md)).
- Pinned helper blob `adea343` at `5fcac4f` has SHA-256
  `a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff`, which
  matches the proposal pin. Frozen A5 was read from Git object `d3bc3aa`
  (`docs/reviews/ci-performance-attempt2-proposal.md`).
- Standards: AGENTS.md, KICKOFF line 112 (one-concern PRs), the
  dytschgo-review smell baseline, and the helper contract
  `tools/ci/ubuntu-mirror.md`.
- Credit: independent review credit applies **only to the Sol correction**. The
  original architecture keeps its credit from the Sol review `c60c14e`.

## Resolution of the prior findings

| Finding | Status | Evidence |
| --- | --- | --- |
| S1 frozen A5 | **Partly resolved**, see R1 | Lines 303–323 now state that delivery is blocked pending an explicit prospective owner decision. They record the old A5 scope, ask for both the A5 amendment and the one-concern exception, keep A1–A4/A6, the thresholds, coverage, first-attempt measurement and attempt counts, and stop the route if either permission is declined. The new path list is not complete. |
| SP1 raw grammar | **Resolved in the rule**, but the test plan has a gap (R2) | Lines 147–160: raw lowercase HTTP(S) before `urlsplit`. Exact `/ubuntu[/]`. LDH labels with the 253 limit. Rejects controls, whitespace, non-ASCII, literal `?`/`#`, percent escapes, backslashes, userinfo, ports and `xn--` labels. Uppercase scheme is refused, not normalized. Lines 239–241 add the matching fence cases. |
| SP2 unknown tail | **Resolved** | Test 9 covers a foreign, malformed or Kernel entry in the tail after allowed + omittable entries, refusing the whole plan with byte-identical targets, including a direct source that would otherwise change. Also covered: Kernel anywhere, duplicate Kernel, malformed tag on an omittable entry, retained order and exact bytes, no dropped targets, and the early-success, unvalidated-tail and omitted-as-retained mutants. Test 4 covers multiple omissions and the 1+15 / 1+16 / 16+1 / all-omitted bounds. Test 6 keeps existing ties and untagged entries with no new tie rejection. Zero retained still refuses. |
| SP3 evidence wording | **Resolved** | Lines 143–145 describe a parsed predicate class plus an unproven raw grammar. Line 89 marks the conclusion as inference only. Lines 200–202 no longer say "exactly". The checkpoint (lines 362–367) says the exact host and tail are UNKNOWN, the fixtures are representative only, and the inspected objects are not exhaustive. |

## Standards

**CHANGES REQUIRED: 1 blocking, 1 minor.**

**R1 (blocking for the owner request): the "exactly these" amendment omits the
helper contract.** Proposal lines 308–313 and PROGRESS line 5 ask the owner to
permit exactly `tools/ci/ubuntu_mirror.py` and `tools/ci/test_ubuntu_mirror.py`,
plus docs "under `docs/`". The helper's contract is
`tools/ci/ubuntu-mirror.md`. It sits outside `docs/` and outside frozen A5
(`d3bc3aa` lines 212–216). After the change it would be false:

- lines 19–21 say every list entry is validated as allowed and that a foreign
  entry refuses;
- lines 24–26 say every original Ubuntu entry is kept;
- the new `ubuntu-mirror-omitted:<n>` token is not documented anywhere. Line 75
  records only historical `ubuntu-mirror-updated:1` output.

Proposal line 199 relies on this file for allowlist origin. If the amendment is
approved as written, it forces one of two outcomes: a second owner round, or a
knowingly stale contract. Correction:

- add `tools/ci/ubuntu-mirror.md` to the requested path list, or explicitly
  justify leaving it unchanged;
- name the docs by exact path, not by category, so the A6 verifier can check
  the diff mechanically;
- if the coordinator treats "refuse unknown" → "drop this shape" as an
  architectural deviation (AGENTS: `docs/adr/`), name that ADR path as well.

This also means the claim "no additional source path" (line 314) must be
restated once the contract path is added.

**R3 (minor): PROGRESS is internally stale.** New line 5 says a fresh recheck
is next. Unchanged line 8 still says "Independent Sol high design review is
next", which line 5 contradicts. Line 5 also does not record the Sol review
outcome (changes required) or the correction's provider/effort (Sol medium,
recorded only in the proposal checkpoint). AGENTS requires per-task
provider/effort/outcome. Supersede or update line 8 when this recheck is
recorded. This report does not edit PROGRESS.

Smell baseline: no actionable smell in a documentation diff. One possible
**Duplicated Code** at design level: the raw grammar (rule 1) restates rules
2–4 and 6. That is acceptable as defense in depth, but it is the source of R2.

## Spec

**CHANGES REQUIRED: 1 bounded, 1 minor.**

**R2: the mutation list was not updated for the SP1 raw layer, and it is now
partly unsatisfiable.** Lines 270–276 require each listed mutant to fail at
least one test, including "make each of scheme, netloc, path, query and
fragment non-binding, one at a time". Rule 1 is an anchored grammar
(`https?://<LDH host>/ubuntu/?`). Any token that passes it already satisfies
parsed rules 2, 3 (netloc), 6 (path, no query, no fragment). Those five mutants
are therefore **equivalent** and cannot fail any test.

A pure local check confirmed this. Python 3.12.10 encoded the stated raw
grammar and parsed rules over a 25-token corpus. The corpus covers the test 1
and test 3 fence cases plus an empty port. Disabling each parsed predicate changed no outcome
(5/5 unchanged). Meanwhile, the list has no mutant for the new raw layer.

Correction:

- add raw-layer mutants: remove the pre-`urlsplit` grammar, allow `xn--`,
  accept an uppercase scheme, use a non-anchored match instead of `fullmatch`,
  and allow `%` or `\`;
- either mutate the parsed predicates together with the matching raw clause,
  or record them as documented equivalent mutants;
- say `re.fullmatch` explicitly;
- optionally add an empty port (`host:/ubuntu`) to test 3.

**R4 (minor): the root rewrite of the sequencing section dropped one ledger
rule.** The original P0 text and the Sol review classified any later,
separately authorized precheck as fetch-path invocation 3 / correction trial 1,
never a performance attempt, with any failure retained in that ledger. Lines
325–330 now only say the precheck should be conserved. They also leave implicit
that a fetch-path failure on attempt 3 would count both as performance
attempt 3 and as failed fix 1 of 3. PROGRESS line 7 already uses that dual
counting for attempt 2. Restore one sentence for each.

Everything else from the original sequencing section is retained:

- attempt 3 is the final candidate's first `pull_request` run;
- a red budget blocks merge, and a standalone fix cannot carry `ci-speed`;
- no empty push, false label, status override, rerun or dispatch baseline
  substitution, and no threshold change;
- speed gains are independently measured on the final head;
- no new stack;
- no cache-isolation claim (correctly removed per the Sol review).

**Not findings (optional):**

- Facts 1 and 3 (lines 35–40) describe `urlsplit`-parsed scheme and path.
  Raw scheme case, leading controls and empty delimiters are unestablished.
  The rule preamble now covers this.
- Line 201 says "strict label grammar" where it means raw-token grammar.
- The header (lines 3–6) and retained boundaries (lines 355–356, "No PROGRESS
  ... edit") describe the original Opus task, not this correction, which did
  edit PROGRESS.
- Out of scope, existing behavior: the HOSTS path still accepts and writes back
  a leading C0 or an uppercase-scheme allowlisted entry. Calling the pinned
  `rewrite_mirror_list` directly confirmed both. The host remains allowlisted,
  so this is not a regression and not a trust expansion.

## Recommendation

**Not yet ready for technical acceptance.** Fix R1 and R2. These are textual
corrections confined to the proposal, with no design change. Fold in R3 and R4.
A narrow confirmation is enough after that. R1 must be fixed **before** the
owner question is presented, so the owner sees an exact and sufficient path
list. Once corrected, the design is technically sound subject to the owner's
two prospective decisions. Neither decision is granted or implied here.

Implementation, source review with complete-plan tests and mutations, and any
hosted execution all remain unauthorized pending that decision.

## Verification actually run

- Git identity, diff, stat and `--check` comparisons as listed above.
- One `python -I -B` process in the session scratchpad. It encoded the
  proposed grammar for the equivalence check and loaded the pinned helper from
  Git object bytes in memory. It made two pure `rewrite_mirror_list` calls. It
  did not use the helper CLI, `apply`, `_apply`, native guards, the network or
  any owner path.
- No source, test or PROGRESS edit. No build, test suite, CI run, push, GitHub
  write, label or rerun.

## Retained boundaries

- Ledgers are unchanged. Performance: 2 of 3 failed, attempt 3 unused.
  Fetch path: 2 failed invocations, 0 failed fixes. Main baseline: 906 s, and no
  legitimate fresh-main shortcut exists.
- E1 and E5 are held locally, with no new stack.
- M2 is incomplete. Native, installed and dedicated paths remain **unverified
  on real hardware**.
- No account, service, cgroup, VM, experiment, private state or owner
  operation was touched. The old Lab Manager and the babysitter were not
  touched.

Standards: 2 findings. The worst is R1, the incomplete A5 amendment path list
(missing `tools/ci/ubuntu-mirror.md`). Spec: 2 findings. The worst is R2, the
equivalent parsed-layer mutants and missing raw-grammar mutants.
