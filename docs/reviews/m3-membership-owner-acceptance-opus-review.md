# Review: M3 membership owner acceptance record (Opus 5.5 high)

Reviewer: Opus 5.5 high, single agent, no children, 2026-11-17.
Target: `50100a023880f8fa271bae40ef0146669dd7222e` (root Sol docs commit),
parent `9d0b80e75593ed1c4e77ea2126942c3058d12b97`. Local `D:/Code/Arxcovo`
HEAD and this worktree both resolve to `50100a0`.
Scope: the three-document delta only: ADR 0005 amendment, the membership
owner decisions brief, and PROGRESS. The earlier root cross-provider design
recheck (`m3-membership-decisions-root-review.md`) is retained, not redone.
PR168 (`b851d14`) files were not reviewed or touched.

Verdict: **PASS, no blocking findings.** The delta accurately records the
three owner selections as product/security requirements without claiming
production L2 readiness. Five Low/Info notes below may be folded into the
next docs change; none needs a fix before publication preparation.

## Owner answers versus recorded text

| Owner answer | ADR 0005 amendment | Match |
| --- | --- | --- |
| D1 fingerprint current voters | code fingerprints voting hosts and keys; joiner checks anchor possession before proof | yes |
| D2 5 s budget, then progress | 5-second response budget, progress exposed separately | yes |
| D2 Stalled after 60 s no progress | "No progress for 60 seconds reports Stalled" | yes |
| D2 no automatic removal | "Nothing is removed automatically" | yes |
| D2 timeout is Unknown | resolve by check/replay of same operation, no second entry | yes |
| D3 host 2 learner until host 3 caught up | then leader automatically promotes both via guarded change | yes |
| Accepted host-1 dependence and disk-loss risk | tradeoff stated and accepted | yes |

The superseded host-2 paragraph is explicitly marked historical; the voter
cap stays unadopted. No new approval step is introduced (AGENTS fence rule).

## Standards findings

- S1 (Low) PROGRESS log entry "Owner membership choices recorded" names
  root Sol and the outcome but not the effort level; AGENTS asks for
  per-task provider/effort/outcome.
- S2 (Low) Brief status line keeps the "(Opus 5.5 high, 2026-11-17)"
  attribution although root Sol changed it in `50100a0`, and its "not closed
  until root re-review, an owner ADR 0005 amendment, ..." clause is now
  partly satisfied. Still true as a conjunction; wording is stale.
- S3 (Info) ADR 0005 header Status stays "proposed". This matches the
  2026-11-15 amendment precedent; acceptable.
- Format: ASCII, LF, valid UTF-8, `git diff --check` clean, three files only.

## Spec findings

- P1 (Low) D3 says "With one or two hosts, writes depend on host 1." Host 1
  is also the only voter with three hosts present until host 3 catches up
  and the three-voter change commits, and indefinitely if host 3 stalls,
  since nothing is removed automatically. Suggest "until the three-voter
  change commits".
- P2 (Low) "No timeout, retry, capacity ... changes are authorized" could be
  read against the adopted 5 s and 60 s numbers. Intended meaning appears to
  be: these are requirements for a future contract and no existing budget or
  CI timing changes. A one-clause clarification would remove ambiguity.
- P3 (Low) PROGRESS current-state blockers list B1-B4 only; B5
  (deny-by-default allow-list) and the root read-ownership/cancellation
  requirement appear in the ADR but not in that line.
- P4 (Info) Owner text said a code is "rejected unused if voting hosts
  change first"; the ADR's "can require a fresh code ... no unconditional
  unspent guarantee" is the conservative reading consistent with D1-A7 and
  root Finding 1. Correct choice.
- P5 (Info) Encoding sizes are labelled "proposed". Arithmetic checks:
  `arxj1-` 6 + 2*100 = 206; `arxj2-` 6 + 2*132 = 270; +32 bytes = +64 hex.

## Security findings

- No overclaim: ADR and brief both state production joining/promotion are
  not implemented or accepted; L2 remains blocked.
- B1 address source, B2 removal authorization, B3 apply-time anchor fencing,
  B4 interrupted joint recovery, and B5 pre-proof deny-by-default boundary
  plus read ownership under cancellation, durable receipt, restart behavior
  and negative controls are all retained as open in the ADR.
- Speculative schema/API detail is explicitly not adopted by the
  selections (ADR closing paragraph; brief checkpoint).
- The D1 requirement that the joiner verifies anchored-key possession before
  sending its redeem proof preserves the bearer-proof protection and the
  "stolen CA key alone" property.
- Durability tradeoff is recorded as accepted, not hidden.
- No secrets, private owner data, gate exception, workflow, quarantine,
  timing or production source change in the delta.

## Checks

Actual: `git rev-parse` of HEAD/parent and local `D:/Code/Arxcovo`;
`git diff HEAD^ HEAD` read in full; `git diff --check` (clean); CR count 0
and `iconv` UTF-8 validation on all three files; added-line non-ASCII scan
(none); reads of ADR 0005, the brief (owner text, B1-B5) and the root review
read-ownership paragraph.
Reused: root cross-provider design recheck and its findings.
Not run: builds, tests, runtime probes, hosted actions, network. M2 open,
M3 unchecked; transport and hardware paths **unverified on real hardware**.

## Outcome

Exact commit reviewed: `50100a0`. Safe to prepare eventual documentation
publication. This review does not grant merge; normal gates and coordinator
merge rules still apply.
