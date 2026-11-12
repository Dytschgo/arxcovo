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
