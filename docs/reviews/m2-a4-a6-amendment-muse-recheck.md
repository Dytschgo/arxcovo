# M2 A4/A6 amendment recheck — Muse (narrow, docs-only)

2026-11-14; reviewer Muse (Muse Spark), independent narrow recheck of the
coordinator Sol amendment against the prior conditional contract review.
Document only; no source, ADR, PROGRESS, test, build, native, product CLI,
system service, account, owner, probe, network, GitHub, dispatch, or subagent
action. Read-only git CLI used for pin/diff verification only.

## Reviewed target (exact pins)

- Head under recheck: `b4ffb437f0d08e37206421e310d299970c24c628` —
  `docs(adr): define bounded A4 and explicit injected A6 acceptance`.
- Parent (prior review record): `383d49215c5e54e373201a9a3ebeb09d48fc395a` —
  `docs(review): contract review for practical A4/A6 acceptance`.
- Accepted-only conditional proposal:
  `bd27b90e213ee35437fd92e63ba33fd730e90640` — `docs(plan): propose bounded
  M2 A4 A6 acceptance` (retained as `b861dba7b4420227d9818e49f14b41abf4eb175b`;
  prepared HEAD `18435a20be6db37096bbd8b427b82bd0dcab2e56` adds only the
  2-line PROGRESS queue entry).
- Prior contract review:
  [m2-a4-a6-practical-contract-muse-review.md](/abs/path/docs/reviews/m2-a4-a6-practical-contract-muse-review.md)
  — verdict "Request changes — narrow, then proceed", conditional pass only
  for the exact quoted D9/brief amendment plus five binding corrections.
- Proposal underpinning it:
  [m2-a4-a6-practical-acceptance-proposal.md](/abs/path/docs/reviews/m2-a4-a6-practical-acceptance-proposal.md).
- Delta verified by `git diff 383d492..b4ffb43 --name-only`: exactly two files,
  no other tree change:
  - [0020-linux-execution-hierarchy-and-janitor.md](/abs/path/docs/adr/0020-linux-execution-hierarchy-and-janitor.md)
    (+63 lines: new §D9a).
  - [0020-linux-hierarchy-implementation.md](/abs/path/docs/briefs/0020-linux-hierarchy-implementation.md)
    (+17/−5 lines: A4/A6 bullets rewritten).
- No production, package, bounds, or mapping change in the delta (see Spec).

## Quoted review bound under check

Per the prior review verdict, the reduction is acceptable **only** as the
exact quoted amendment: A4 = native fixture-preseeded bounded populated-leaf
startup cleanup under unchanged Standard package, mechanism evidence without
prior-MainPID SIGKILL provenance or crash-lifecycle/directory-removal
conclusions, A9/U9 retained; A6 = deterministic injected-port
LeafRemoval/KillTimeout/VerifyFailed through actual setup → readiness
latch → quarantined persisted refusal, non-native failure evidence with no
native-nonremovability claim; mappings `LeafStructure`/`KillInterface` →
`mechanism_unavailable`, `LeafRemoval`/`KillTimeout`/`VerifyFailed` →
`quarantined` unchanged.

## Standards — PASS (docs-only amendment)

- One-concern conventional commit (`docs(adr)`), two-file scope, exact
  rationale links to proposal and prior review, explicit "effective after
  independent review, required checks and coordinator merge" gate, explicit
  "grants no native run, retry, activation or milestone acceptance" boundary,
  and explicit **unverified on real hardware** labelling satisfy
  [AGENTS.md](/abs/path/AGENTS.md).
- No product-approval flow, no public-publishing default change, no Lab
  Manager/VM contact, no secret or owner data, no CI-budget or review bypass.
- No Standards findings. This axis grants no implementation or run permission.

## Spec — PASS (exact coverage, all five corrections bound)

- **A4 native present-residue without prior SIGKILL/crash claim; A9/U9
  retained.** ADR D9a: "Prior-MainPID SIGKILL provenance is excluded; crash
  lifecycle and directory removal are not established by A4. A9/U9 remains
  independently required. No historical crash-survivor credit is implied."
  Brief A4: "native present-residue mechanism evidence, not prior-MainPID
  SIGKILL provenance or crash-lifecycle evidence … A9/U9 stays independently
  required." Matches the bound exactly.
- **A6 actual-setup injected LeafRemoval/KillTimeout/VerifyFailed →
  readiness quarantine / executor zero calls / committed refusal / no
  domain recovery / shutdown Stopping; explicit non-native, no native
  induction.** ADR D9a + brief A6 jointly require: latch quarantined, refuse
  execution with zero runner calls, persist the refusal, refuse same-domain
  recovery, preserve shutdown-Stopping; "non-native failure evidence; native
  nonremovable-leaf induction is not claimed". Matches the bound exactly.
- **Binding correction 1 (helper provenance/passivity, 64 MiB / 0 swap /
  1 pid before placement, 15 s lifetime).** D9a §1: fixed test-only
  executable re-entry, one waiting behavior, never shell/caller-selected;
  no commands, no forks, no network/storage, no probes; fixture exclusively
  creates the leaf and writes `memory.max=64 MiB`, `memory.swap.max=0`,
  `pids.max=1` before placing the helper; preparatory writes labelled an
  explicit fixture exception; blocked-handshake start; 15-second absolute
  lifetime. Incorporated.
- **Binding correction 2 (sole S7 kill accept path; expiry fails; failure
  cleanup distinct).** D9a §2: only S7's `runs/cgroup.kill` write may kill on
  the acceptance path; seeder never writes `cgroup.kill`; failure cleanup may
  kill only after acceptance already failed with distinct attribution; early
  death/expiry fails; already-empty leaf earns no credit; omitted-kill mutant
  must fail native acceptance assertions. Incorporated.
- **Binding correction 3 (parent identity, waitpid SIGKILL, >6 s margin,
  5 s deadline).** D9a §3: spawning parent survives S7, holds child identity,
  reaps with waitpid after setup requiring SIGKILL death before the helper
  deadline; pre-single-setup confirmation of live membership, populated=1,
  empty runs procs, sole MainPID in supervisor, with more than six seconds
  of helper lifetime remaining; production five-second kill bound preserved.
  Incorporated.
- **Binding correction 4 (Created-only trust, nofollow literal, no adoption,
  exact evidence, S9).** D9a §4: reuse A5 trusted no-follow descriptor pattern;
  `mkdir_runs` returns Created, never adopt; exclusively create literal
  `arx-0-0`; reject links, foreign/replaced objects, nested/unknown leaves;
  keep 4096-byte reads, 512-record/64-child lists, 1 MiB capture; require
  exactly one removal, `runs_created=false`, `unit_written=true`,
  `runs_written=true`, literal leaf absence, fresh S9 before READY;
  historical A1/A2/A5 assertions unchanged. Incorporated (`runs_created=false`
  is the runs-reuse encoding; `unit_written`+`runs_written` are the two
  controller writes).
- **Binding correction 5 (joined A6 + mutants).** D9a §5: A6 joins actual
  production setup over the injected port to readiness latch, executor refusal
  with zero runner calls, committed quarantined activity, same-domain
  subsequent-request refusal; Busy at validated leaf rmdir, population beyond
  five seconds, fresh verification residue as separate cases; shutdown
  Stopping preserved; component tests alone insufficient; mutants must expose
  swallowed removal failure, skipped verification, early Ready. Incorporated.
- **No production/package/bounds/mapping change.** Delta touches no source,
  packaging, or bounds table. D9a restates unchanged: package, production
  cleanup, deadlines, readiness, restart-only recovery; mappings
  `LeafStructure`/`KillInterface` → `Setup/mechanism-unavailable` and the
  three janitor failures → quarantine retained; installed systemd teardown
  "remains unobserved here". Verified by two-file `--name-only` above.
- **Authority gating correct.** D9a: effective only after independent review,
  required checks, coordinator merge; grants no run/retry/activation/milestone
  acceptance; source review + required checks mandatory; native A4 execution
  needs a separate bounded disposition and acceptance review. Source contract
  is not run permission. M2, remaining Stage F, Stage G, real-Runner PA6 stay
  open; installed/dedicated paths **unverified on real hardware**.
- No Spec findings. Proportionate technical fences only; no optional
  framework, no product approval flow.

## Security — ACCEPTABLE (no new findings)

- The amendment adds no executable surface, no selector API, no harness, no
  second-setup substitute, no manual pre-removal, and explicitly excludes
  chmod/mount/migration-race/uninterruptible-workload tricks and any general
  restart harness — the prior review's non-goals are restated verbatim in D9a.
- Helper limits (64 MiB / swap 0 / pids 1 / 15 s / 5 s kill bound / >6 s
  pre-setup margin) and single-kill attribution are bound in contract text
  before any source slice, as required.
- No security findings beyond the five corrections, all incorporated above.

## Actionable findings

- None. No changes requested on this docs-only amendment within the quoted
  bound.

## Verdict

- **Pass — narrow recheck.** Amendment `b4ffb43` covers the prior review's
  exact quoted bound with all five binding corrections incorporated, no
  production/package/bounds/mapping change, and correct authority gating
  (implementation only after amendment review gates + merge; native execution
  separate; source contract is not run permission).
- Permitted next scope (unchanged from prior review): coordinator merge after
  green required checks, then small one-concern source slices with the §D9a
  requirements bound in code plus the stated mutation checks, under normal
  Sol-builds / different-provider-review routing, then a separately authorized
  bounded disposable-host run. No run permission is granted here.
- M2, remaining Stage F, Stage G, real-Runner PA6 stay open; hardware paths
  **unverified on real hardware**.
