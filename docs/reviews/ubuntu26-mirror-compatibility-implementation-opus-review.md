# Ubuntu 26 narrow mirror omission: independent Opus implementation review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent,
no subagents, 2026-11-12. Local only: no push, PR, label, GitHub write, CI,
rerun or owner/admin action. Source and proposal files are not edited.

Verdict: **CHANGES REQUIRED before push. One small blocker (B1).** The
omission grammar, bounds, whole-plan refusal, count plumbing and output tokens
are correct. Their tests are meaningful, and every check I ran passed. B1 is a
regression on the *retained* path. The new ASCII line splitter makes it
possible, and the fix is one guard plus tests.

## Target and independence

- Source `e9d63df9a2670a9fe52b31415f4d94152cb76395` (parent
  `1ac1aa167e03b1ba8d0a3284b512d6c71d15dcec`). It touches exactly
  `tools/ci/ubuntu_mirror.py`, `tools/ci/test_ubuntu_mirror.py` and
  `tools/ci/ubuntu-mirror.md`. Evidence `adf2c52a1dccbf0a6ff697e7ad1ece2839504c5b`
  (parent `e9d63df`) touches only
  `docs/evidence/ubuntu26-mirror-compatibility-fix.md`.
- The SHA-256 values of the three source blobs match the evidence manifest
  (`888fd3e7...`, `eb2f9e1b...`, `19c42387...`).
- Base identity: at `1ac1aa1`, `344c406` (PR54 pin), `origin/main` and #38's
  `441978d`, the helper, test and contract blobs are identical (helper blob
  `adea3432...`). So cherry-picking `e9d63df` onto PR54 applies to the same
  base bytes, with no unrelated product history.
- Runtime: an isolated T3 worktree at the coordinator progress head `18ea43e`,
  clean, with its top level different from canonical `D:/Code/Arxcovo`. I read
  the targets from Git objects (`git show`/`git archive`) and never from the
  checkout.
- **Cross-provider (independent):** the Sol implementation `e9d63df` and the
  Sol evidence `adf2c52`.
- **Not independent:** the omission design, raw grammar, required-test list
  and mutation list are Opus proposal work. Sol keeps credit for implementing
  them. No architecture independence is claimed.

## Read

AGENTS.md, the KICKOFF review rules, the current proposal (including the
owner disposition), ADR 0021 (including the owner disposition), the Sol
review, the final Opus recheck with its R2a/R2b sections, root correction
`6e7616f`, the full helper at `e9d63df`, the source diff, the test diff and the
evidence document.

## Verification actually run

Platform: Windows 11, Python 3.12.10, Node v24.18.0. All Python ran with
`-I -B` on `git archive` copies in the session scratchpad. There were no
actual `/etc/apt`, production CLI/`apply`, sudo/root, network or package
operations, and no Rust suite was run (this change is Python-only).

1. Suites: source `e9d63df` ran 31/31 OK. Base `1ac1aa1` ran 23/23 OK. On an
   `e9d63df` archive, the Node tests `ci-workflow`, `policy`, `evaluate-budget`
   and `enable-auto-merge` ran 19/19 with 0 failures and 0 skips.
2. `git diff --check 1ac1aa1 adf2c52` is clean. The workflow does not parse
   helper stdout (`ci.yml:65-66` only runs the suite and the helper), so the
   extra stdout line cannot break the step.
3. Independent omission oracle: a hand-written token validator that shares no
   regex with the helper, run against 296,578 structured mutations of grammar
   seeds. The mutations insert, delete or replace C0/DEL, NBSP, U+0085, U+2028,
   U+212A, U+FF0E, U+3000, `xn--`, `%2d`, ports, `@`, `//`, `?`, `#`,
   uppercase text and the Kernel host. The helper omitted 14,539 of them. Every
   omission matched the oracle except a single trailing CR, which is the
   intended CRLF terminator. No oracle-valid token was refused.
   - Targeted cases: a 253-character host and a 63-character label are omitted;
     254 refuses. `xn--` refuses as the first or an inner label, while `xn-a`
     is omitted. A trailing hyphen refuses. IPv6, `//ubuntu`, `/./ubuntu`,
     empty userinfo, an empty port and Kelvin/fullwidth look-alikes all refuse.
     The Kernel host refuses, and HOSTS entries are retained.
4. Differential base vs. source over 154 retained/comment variants. This found
   B1 and A3.
5. 17 independent in-memory mutants, each in its own owned temp copy and each
   running all 31 tests. After the run, the production copy hash was
   unchanged.

| Mutant (mine, not the builder's set) | Result |
| --- | --- |
| Line split back to `splitlines()` | killed (6) |
| Tokens back to `str.split()` | killed (8) |
| Drop trailing-empty `pop` | killed (9) |
| Drop `removesuffix("\r")` | **survives** (A2) |
| Raw 253 lookahead only | equivalent: the parsed length still rejects; no credit |
| Parsed `len<=253` only | equivalent: the raw lookahead still rejects; no credit |
| Paired scheme admits `ftp` | killed (1) |
| `_apply` returns omitted 0 | killed (1) |
| `main` drops the omitted line | killed (1) |
| Total cap 17 | killed (2) |
| Omission before tag grammar | killed (3) |
| HOSTS made omittable | killed (7 failures, 4 errors) |
| Omitted becomes a blank line | killed (6) |
| Paired empty `?`/`#` | killed (2) |
| Paired trailing-dot host | killed (1) |
| No 4 KiB output check | killed (2) |
| Direct mode reports omitted 1 | killed (1) |

I did not re-execute the builder's 21-mutant ledger. Its rows were read
against the proposal list and are complete. The symmetric equivalence records
for single-layer port edits are consistent with the equivalents I observed.
No production guard was weakened to manufacture a kill.

## Standards

- No documented-standard breach. The commits are conventional and
  one-concern, and the source manifest stays in the approved paths. The
  contract doc is updated, and the label "**unverified on real hardware**" is
  kept.
- Possible Primitive Obsession/Duplicated Code: the omission predicate is a
  12-term inline conjunction. Most of it intentionally repeats the raw grammar
  as backup guards. This is acceptable because the proposal requires both
  layers, so it is not a finding. A named helper (`_omittable(token, parts)`)
  would make later audits easier.
- Possible Mysterious Name: `count` now means "retained" next to `omitted`.
  Renaming it `retained` would make `1 <= retained <= retained + omitted <= 16`
  read directly. Judgement call only.

## Spec

Met:

- Raw lowercase-ASCII `fullmatch` before `urlsplit`.
- LDH labels of 1-63 characters, a host of at most 253, at least one
  subdomain label, and no `xn--` in any label.
- Exact `/ubuntu` or `/ubuntu/`, no port or userinfo, and no `?`/`#` even when
  empty.
- Every parsed backup guard is kept.
- Drop-only: the entry is never serialized, and `replacement()` is unchanged,
  so direct sources still refuse. At least one HOSTS fallback is required, and
  all active entries count toward 16. The 4 KiB input/output bounds are kept.
- Any invalid tail, a Kernel host anywhere, a duplicate Kernel host, a nested
  list or a malformed tag refuses the whole plan before any `os.replace`. This
  includes a direct source that would otherwise change.
- 144 to 195 identity is kept. Priority+1 and ties are unchanged. The second
  run refuses.
- Keyring, signature, trust, Valid-Until, file guards, recheck and publication
  are unchanged apart from the count plumbing.
- Success prints exactly `ubuntu-mirror-updated:N` and
  `ubuntu-mirror-omitted:M`. Refusal and argument errors print one fixed
  stderr token and no success data.
- Tests 1-10 and the full mutation list are present, including the R2b UTF-8
  label case and its killed paired mutant.

Minor gaps: see A2 and A4.

### B1 (blocking): interior CR in a retained HOSTS entry is serialized verbatim

Replacing `splitlines()` with `split("\n")` means an active line can now
contain an interior `\r`. The retained path still decides on the
`urlsplit`-normalized view, and `urlsplit` deletes `\t\r\n` anywhere. As a
result, a token like `https://archiv\re.ubuntu.com/ubuntu` (tagged or
untagged) passes `host in HOSTS` and `replacement()`, and its raw bytes are
written to the root APT list. The literal host there is not a HOSTS name.

The base refused all such lines. The same change turns `\x0b`, `\x0c` and
`\x1c-\x1e` prefixes on HOSTS entries from "split into a blank line plus a
clean entry" into "written verbatim with the control prefix".

Example (probe): the base list plus `http://arch\rive.ubuntu.com/ubuntu
priority:5` is refused by `1ac1aa1`. At `e9d63df` it yields
`...http://arch\rive.ubuntu.com/ubuntu\tpriority:6\n`.

This contradicts proposal "Every other entry follows today's code", the
contract text "must be either a retained HOSTS HTTP/HTTPS URI", and this
task's "never serialize an unsupported host". It is the same
normalization-differential class that the raw fence closes for omissions.

Impact: I expect APT, which splits on LF only, to see an invalid host or
scheme and not a resolvable foreign host. **That expectation is unverified
here.** Signatures still protect integrity. I found no omission bypass.

Fix (checked locally on a scratch copy only): before tokenizing an active
line, refuse it if it matches `[\r\x0b\x0c\x1c-\x1e\x85\u2028\u2029]`. This
restores the base's property that none of the old `splitlines` separators
reaches the output, and it only adds refusals. With that guard, the suite ran
31/31, the fuzz serialized no non-allowed token, and the canonical CRLF list
still produced 195 bytes.

Required tests:

- An interior-CR HOSTS entry refuses, tagged and untagged.
- A `\x0b`/`\x1c`-prefixed HOSTS entry refuses.
- A mutant that removes the guard is killed.

The fix delta needs an independent focused recheck before push.

### Material advisories (non-blocking)

- **A1, comment/blank classification:** `str.strip`/`lstrip` are Unicode-aware,
  so `\x1f#...`, `\xa0#...` and Unicode-whitespace-only lines are preserved
  verbatim as comments or blanks. APT likely strips only ASCII whitespace and
  may parse such a line as an entry. Because the junk always lands in the
  scheme, no resolvable host results. The NBSP form already existed in the
  base. Using ASCII `strip(" \t")` here would match the separator hardening.
  The B1 guard also covers the `\x0b`/`\x0c`/`\x1c-\x1e` comment forms if it is
  applied before this check.
- **A2, CRLF compatibility is untested.** The `removesuffix("\r")` mutant
  survives. The base accepted CRLF lists through `splitlines()`. Add a CRLF
  canonical test that expects the same 195 bytes. It also protects B1's fix.
- **A3, undocumented fail-closed changes on the retained path:** U+0085,
  U+00A0 and U+2028 prefixes and a trailing `\x0b` on HOSTS entries now
  refuse. The base accepted and sanitized them. This direction is safe, but
  `ubuntu-mirror.md` should state that active lines are ASCII-tokenized.
- **A4, test 7 is partial for omission:** the omission publication test does
  not assert two list reads or the absence of staging residue on success. The
  canonical publication test covers the shared path.
- Already in the base and out of scope: leading C0 `\x01-\x08` and `\x0e-\x1b`
  on HOSTS entries are still accepted verbatim, because `urlsplit` strips them.

## Ledgers and boundaries

- Performance: 2 of 3 failed; attempt 3 is UNUSED.
- Fetch path: 2 failed hosted invocations, and 0 failed hosted fixes before
  this local work.
- There is no reset, threshold change or optional precheck. This review adds
  no hosted invocation.
- The historical hostname and tail remain UNKNOWN. Synthetic fixtures are
  representative only, and no reproduction is claimed.
- Native, installed and dedicated paths remain **unverified on real
  hardware**. M2 is incomplete. E_PROTECTION and ExecutionFailed stay separate
  and unretried.
- No account, service, cgroup, VM, Lab Manager, experiment, private state or
  babysitter was touched.

Standards: 0 findings (2 judgement-call smells). Spec: 1 blocking (B1, retained
interior-CR serialization regression) and 4 advisories, the worst being A2
(CRLF untested). Push readiness: **NOT READY** until B1 is fixed, its tests
are added and the delta is independently rechecked.

## B1 recheck - 2026-11-12

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent,
no subagents. Local only: no push, PR, label, GitHub write, CI, rerun,
optional precheck or owner/admin action. No source, proposal or PROGRESS edit.
The prior report above is kept verbatim.

Verdict: **B1 CLOSED. PASS for the source delta. No new blocker.** The
correction is exactly the guard recommended above, and every B1-required test
is present. My own guard-removal mutant is killed.

### Target and independence

- Correction `4b9695257a57d187d55cdf7ca94d5551d6acdde3` (parent `ebe535b`)
  touches only `tools/ci/ubuntu_mirror.py` (+2) and
  `tools/ci/test_ubuntu_mirror.py` (+12). Evidence
  `8c2ea110505ba21a54750b533797742e9e0f3c59` touches only `docs/PROGRESS.md`
  and `docs/evidence/ubuntu26-mirror-compatibility-fix.md`. `git diff --check
  ebe535b 8c2ea11` is clean.
- Between `e9d63df` and `4b96952`, the only source paths that differ are the
  two helper files. The contract `tools/ci/ubuntu-mirror.md` blob is unchanged
  (`e800270f...`), and so is `ci.yml`. The helper and contract blobs at the
  base are still identical at `1ac1aa1` and the PR54 pin `344c406` (helper
  `adea3432...`).
- Helper blob `79b583e5...` (SHA-256 `2c861c8b...`), test blob `1521cb29...`.
- Runtime: an isolated T3 worktree, clean at `8c2ea11`, with its top level
  different from the canonical checkout. Targets were read via `git archive`
  into the session scratchpad.
- **Cross-provider (independent):** the coordinator Sol medium correction and
  evidence. Not independent: the guard text itself, which this report
  recommended. Sol keeps credit for implementing and testing it.

### Verification actually run

Windows 11, Python 3.12.10. All runs used `-I -B` on owned archive copies.
There were no actual APT, `/etc/apt`, production `apply`, sudo/root, native,
network or hosted operations.

1. Suites: `4b96952` ran 32/32 OK. `e9d63df` ran 31/31 OK, and `1ac1aa1` ran
   23/23 OK.
2. Red check: the `4b96952` tests against the `e9d63df` helper ran 32 tests
   with **14 failures**, matching the builder's claim.
3. Diff: the guard sits after `removesuffix("\r")` and before the
   blank/comment classification and tokenization. Its class
   `[\r\x0b\x0c\x1c-\x1e\x85\u2028\u2029]` is exactly
   the set of `str.splitlines` separators other than LF, so none of the old
   `splitlines` boundaries can reach the output again. `urlsplit` deletes only
   `\t\r\n` inside a token. Tab is a token separator, and LF ends the
   line. CR is now refused, so the interior normalization differential is
   closed.
4. Independent mutants (15, mine). Each ran in its own owned copy against all
   32 tests. The production copy hash was unchanged afterwards.

| Mutant | Result |
| --- | --- |
| Guard removed | killed (14) |
| `removesuffix("\r")` removed | killed (1); closes prior A2 |
| Guard moved before `removesuffix` | killed (1, CRLF case) |
| `re.search` -> `re.match` (line start only) | killed (2) |
| Class reduced to CR only | killed (10) |
| Drop CR / VT / FF from class | killed (4 / 2 / 2) |
| FS-RS narrowed to FS-GS or GS-RS | killed (2 each) |
| Drop NEL / LS / PS from class | survives: equivalent on active lines (see 5) |
| Guard moved after blank/comment check | survives: differs only on comment/blank lines |
| Guard applied to `tokens[0]` only | survives: differs only on comment/blank lines |

5. Differential probe: base, `e9d63df` and `4b96952`, plus a combined
   no-NEL/LS/PS mutant. It covered 37 characters (C0 except LF, DEL, NBSP,
   U+0085, U+2028, U+2029, U+3000) in prefix, inner-host, inner-path, suffix,
   comment-prefix and blank-line positions, tagged and untagged, on the
   canonical list.
   - The correction closes all 34 separator leaks that `e9d63df` had, on both
     active and comment/blank lines.
   - CRLF canonical input gives 195 bytes, byte-identical to the LF output.
     `\r\r\n` refuses.
   - On active lines, the no-NEL/LS/PS mutant behaves exactly like the
     correction. Those three characters never pass the existing HOSTS,
     `replacement()` or raw checks in any position. In the guard they only
     affect comment and blank lines.
   - Exactly one fix-vs-base difference remains: a `\x1f` prefix on a
     **tagged** HOSTS entry is serialized (A5 below). It is not a `splitlines`
     separator, so it is outside B1.

### Standards

- No documented-standard breach. The commits are conventional and
  one-concern, the change stays in the approved helper paths, and the tests
  are meaningful.
- Possible Mysterious Name (judgement call): the class literal has no name or
  comment tying it to `str.splitlines`. A short comment, or a constant such as
  `_SPLITLINES_NON_LF`, would make audits easier.
- No other smell in this delta.

### Spec

- B1 required tests are met. Tagged and untagged inner-CR HOSTS entries
  refuse. Prefixes of every separator, including `\x0b` and `\x1c`,
  refuse tagged and untagged. The guard-removal mutant is killed (builder's
  and mine). The fix only adds refusals, and every existing assertion still
  passes.
- The prior A2 gap (untested CRLF) is now covered and its mutant is killed.
- Nothing outside B1 was added. The production guards are intact and no
  check was weakened.
- Advisory **A1** (partly addressed, still open). Because the guard runs
  before the comment check, `\x0b`/`\x0c`/FS-RS/NEL/LS/PS comment and
  blank forms now refuse. No test pins this, which is why three mutants
  survive. A one-line case such as `"\x0b# note"` and `"\u2028"`
  expecting `Refusal` would kill them. The NBSP, U+3000 and `\x1f` comment
  forms are as before. Non-blocking.
- Advisory **A5** (new, non-blocking). A tagged HOSTS entry with a leading
  `\x1f` is written with that byte. Base dropped it, because `str.split()`
  treats `\x1f` as whitespace. Base already wrote the same untagged bytes
  (`\x1fhttps://archive.ubuntu.com/ubuntu`) verbatim, so this is not a new
  output class. It belongs to the existing leading-C0 class (`\x01-\x08`,
  `\x0e-\x1f`) noted above as present in base and out of scope. The junk
  stays in the scheme, so no foreign host results; that APT effect is
  **unverified here**. A later hardening could refuse
  `[\x00-\x08\x0a-\x1f\x7f]` on active lines.
- **A3** and **A4** remain advisories without a concrete regression. The
  contract doc still does not say that active lines are ASCII-tokenized and
  that line separators refuse.
- Residual risk: if a real hosted mirror list contained one of these
  separators, the step would now fail closed. Its real content remains
  UNKNOWN.

### Ledgers, boundaries and push readiness

- Performance: 2 of 3 failed; attempt 3 is UNUSED. Fetch path: 2 failed
  hosted invocations and 0 failed hosted fixes. This was one local
  correction, with no hosted attempt, reset, threshold change or optional
  precheck. This recheck adds no hosted invocation.
- Native, installed and dedicated paths remain **unverified on real
  hardware**. M2 is incomplete. E_PROTECTION and ExecutionFailed stay separate
  and unknown, with no retry.
- Push readiness: **READY from source review.** The coordinator may integrate
  `e9d63df` + `4b96952` (the approved three helper paths and named docs, as
  the separate #54 compatibility commit) once root integration passes. That
  integration still needs its own unchanged required checks and measured
  acceptance. This review does not grant merge permission.

Standards: 0 findings (1 judgement-call smell). Spec: B1 closed, 0 blockers.
The worst open advisory is A1 (comment-line separator refusal untested; three
equivalent-on-active-line mutants survive).
