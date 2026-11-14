# M2 F0b: independent Sol design review

2026-11-14. Reviewer: OpenAI / gpt-6.1-sol / high. Author reviewed:
Anthropic / claude-opus-5-5 / high. Single review sub-agent; no delegation.
This is different-provider design review, not source implementation review.

**Standards: CHANGES REQUIRED (one blocker). Spec/security: CHANGES REQUIRED
(two blockers).** The bounded transport direction is sound, but its completion
claim, reader-failure policy and regression migration need correction before
implementation. This review grants no implementation, execution, publication,
dispatch, retry or merge authority.

## Scope, isolation and exact pins

Cwd equalled the Git top level in the supplied isolated T3 worktree, branch
`t3/review-m2-f0b-transport`. Initial `git status --short` was empty. Prepared
HEAD was `45e5539f163d33a888f14c1483b60494d29cabe8`. Paths below are
repository-relative; no owner filesystem paths or private state are retained.

| Object | Exact identity |
| --- | --- |
| Original proposal commit | `d5686cd17566bb9d9f357c3bf1fb664b3a53a342` |
| Retained proposal commit | `5353e3b33117f26439a6a63f93a5f084cdbe1618` |
| Proposal blob, identical at both commits | `62dbc83e1eed6cb8b386eef26315ecc98ff835f8` |
| Original F0 review / retained review | `4220dd780d52a4008d077a3aab0bbc581c7bb504` / `9c64c6660a82956afff0cd1bff01055d227b7492` |
| F0 review blob, identical at both commits | `99df914aa99ef4663869053815ad6ded01469204` |
| Authoring base | `f0baed412b82abf5ade88e3016853e4bf08b7f66` |
| Merged F0 main pin supplied for review | `affaec91b83d3a3c06a67dc796578a98972c0c2b` |
| Complete tree, identical at those two bases | `eccb0ce5d2cdbba98cf5ddbca557149a1a0b96a1` |
| Prepared CI-only follow-up | `a6f76051e02a7714bce4b31a1952ab5fb5aec558` |
| Driver / original tests | `cf5a0ca64707ccf95036490a06c7dc3b4cb86573` / `0baeeb58f2e789bf9babd32322fd7ee7000e3093` |
| F0 blocker tests / mutation checker | `6545f5d7245c9bd5dc0eee311a0245968a8d0f0d` / `fe5b976bd1b946c29012d33879536d172406dc9f` |
| Tool README / F0 evidence | `aaa397530133e05a6aea5afaafa5e4e9f24b0094` / `6b2f2056f909a5fe80ef085aa5b157285c407038` |
| ADR 0019 / remaining-gates proposal | `cc36ce4e7e055aeeb36e2a061ccac380f36697d6` / `8fd8bdc011740cc51e970acb9cc685df572d82a8` |
| AGENTS / KICKOFF | `c2ae25947916672b13180778265354907b75e295` / `9dc49e726bce3af9144ac867b5f7d2c96f4bca77` |
| Manual experiment workflow / disposable Rust fixture | `e5f1a96dbfa522bda602db45bf24db000ebddd5c` / `586b30811b3af485f6b96f7a4a989b2ba64e3e70` |
| Prepared CI workflow | `fc065af9786e75c52cfb7668b726d0a89931cd5b` |

Read AGENTS, KICKOFF, CONTRIBUTING, SECURITY, dytschgo-review and its active
profile; the target proposal; the original F0 review including its F0b
contract; both F0 correction rechecks; ADR 0019 D7/C2; Stage F including F2;
current driver, replay/tests/mutation sources and README; relevant F0 evidence
and disposable fixture source excerpts. Review used Git objects rather than
substituting an unrelated checkout or live environment.

Git comparison confirms the retained proposal is unchanged. The prepared
CI change widens unittest discovery from `test_driver.py` to `test_*.py`;
it does not change the transport contract. The proposal's PR70/main/CI
statements describe its earlier authoring base. PR71 pending review/CI is
user-supplied context, not independently checked host status.

**Checks actually performed:** Git identity/status/log/tree/blob/diff reads
and skill reads only. No build, test, mutation execution, network, GitHub,
driver invocation, runtime stat observation, proc/sys/cgroup/account/service
operation, VM, installation, owner workflow or private-state access occurred.
Prior test counts are historical source evidence, not runs credited here.
Only this review file is authored by this task.

## Standards

**ST-B1 — regression migration is broader than “v2 expectations only”.**
Proposal lines 562–571 require all existing 32 tests and 23 mutants to keep
working, but permit `test_f0_blockers.py` changes only for v2 expectations.
The current `xf_success` mutant matches the entire old success-rule text,
which V5 replaces: its exact-once anchor cannot survive unchanged. The
current case 0 changes only `mutation` to `staged`; after a mechanical v2
conversion retaining non-null internal statuses, it also violates V1. It
then rejects even with the success rule removed, masking the desired kill.
AGENTS requires “Every behavior needs a meaningful test”. Permit narrowly
scoped anchor and counterfactual migrations, re-isolate each existing mutant
against the new cross-field rules, and retain assertion attribution and
zero error/compile credit. Do not preserve the number 23 as a substitute
for those properties.

**ST-A1 — advisory: 33 names are candidate mutations, not 33 earned kills.**
The proposed runner correctly excludes equivalent mutants. In particular,
dropping only final-file `O_NOFOLLOW` while retaining `O_CREAT|O_EXCL`
cannot demonstrate a symlink-following write; the existing-entry refusal
still blocks it. Do not make the fake OS follow that symlink to manufacture
a kill. Distinguish this from dropping ancestor-walk NOFOLLOW. Split the
combined nonblock/type-check mutant if claiming each guard independently;
assert flags before a fake FIFO open rather than allowing a test to hang.
Every case must otherwise satisfy schema, ownership, size and canonicality
checks. Reassess ASCII/canonicality overlap and duplicate trust-flag mutations
for equivalence after implementation. The Fowler baseline raises no separate
hard violation; keep one shared bounded validator rather than adding a
general transport framework (possible Speculative Generality).

## Spec/security

**SP-B1 — completion evidence is overstated (lines 285–290, 336–337).**
The F0b contract says “after systemctl stop (so no writer is alive)”. The
proposal improves this to stop-returned plus cgroup-absent, but asserts the
subtree “contains every writer” while lines 433–438 and 592–593 explicitly
leave root-process escape unknown. Directory absence supports completion
inside that subtree; it does not prove absence of escaped root writers.
Likewise, monotonically growing file existence is a source assumption about
these writers, not a guarantee against concurrent privileged modification.
Correct the normative claim to a conditional, bounded post-stop snapshot
under the existing containment assumption. State that escaped-root coverage,
atomic consistency and complete capture are unproved. Keep bounded strict
reads safe without assuming immutability; retain unknown/no-channel-read
when the completion condition is unavailable. Do not broaden F0b into
process discovery, a new containment mechanism or an observation run.

**SP-B2 — reader failure has contradictory exit requirements (lines
384–389, 494–495).** The original contract says the reader “never changes
exit or cleanup semantics”. V5 intentionally makes an otherwise successful
run fail when statuses are unknown/malformed/captured. A reader exception
that produces unknown therefore can change exit 0 to exit 1, contrary to
the blanket test requirement that reader exceptions never change exit.
Explicitly adopt this narrow success-policy deviation and qualify the
invariance tests: body/stop/cleanup exceptions, first/final classification
and cleanup tokens remain unchanged; a telemetry anomaly on an otherwise
successful run yields only the closed E_RECORD fallback and exit 1.
Test both cases through real entry/run with effects mocked, including a
captured status on apparent success. No invented first/final refusal or
cleanup failure may explain away the anomaly.

The remaining fixed-channel, bounded-record direction meets the original
contract in design; implementation evidence remains outstanding.

## Q1–Q6 disposition and required boundaries

| Question | Disposition |
| --- | --- |
| Q1: statuses plus one carried refusal | **Accept deviation.** Three fixed statuses and one payload stay bounded and expose more honest coverage than one selected status. Drop redundant internal_count; only the number of captured mode slots is derivable, never the number of refusals or writers. Precedence service → fixture-check → harness is display selection, not chronology or cause. |
| Q2: stop returned plus cgroup absence | **Conditional only; SP-B1 blocks the present wording.** Ignore rc, including rc 5; rc 0 proves no more. Only lstat FileNotFoundError establishes the chosen absence signal. Other errors/presence or a raised stop yield all unknown and no channel opens. This is completion inference within the existing subtree, not proof of escaped-root coverage or a real-hardware result. |
| Q3: non-absent statuses on success | **Accept intentional fail-closed tightening, subject to SP-B2.** Success still requires first=null, final=null and cleanup=confirmed, plus service-or-higher lower bound and three absent statuses. Absent alone never establishes success. All unknown/malformed/captured success combinations must fail closed. |
| Q4: remove v1 replay | **Accept for this undispatched local mechanism.** No retained real v1 line exists in the supplied evidence. Keep synthetic v1 rejection, bump emission/parser together and document incompatibility. Historical synthetic counts confer no compatibility requirement or real-record replay credit. |
| Q5: nested try/finally around stop | **Accept structural ordering, conditional on Exception-total snapshot and SP-B2.** A raised stop still escapes after snapshot, skips the old quarantine check and later cleanup, and leaves cleanup unconfirmed. A returned stop still reaches the unchanged independent quarantine fence. Snapshot closes descriptors before returning and cannot replace an existing failure. BaseException remains the disclosed no-record case. |
| Q6: root-down fd walk and marker swallow | **Accept.** A single stage open protects only its last component; root/opt/stage descriptor-relative NOFOLLOW checks close that ancestor gap. Check directory/root ownership and no group/other writes at every component, plus private stage permissions. Close parent/final fds on every failure. Best-effort marker failure lowers evidence only and must not skip or reorder control.mkdir. |

Retain these requirements in the corrected design and later review:

- Fixed filenames only in the root-owned private stage (created 0700);
  feedback files regular, root-owned, exact 0600, nlink 1 and bounded size.
  Keep EXCL, NOFOLLOW, CLOEXEC, descriptor-relative opens and trusted
  ancestors. The writer runs only after installed passed, for exact Refusal
  type, mode/site allowlist and initial phase. It writes once; short writes
  remain malformed. Later writers cannot overwrite the first creator.
- The reader runs after stop evidence and before quarantine/cleanup/deletion.
  Unknown skips feedback-channel opens, but may open the trusted stage for
  separate existence-only progress checks; clarify the test phrase “no open
  happens” accordingly. No progress-file contents are opened. Do not confuse
  such stage/marker metadata reads with feedback payload reads.
- The account/cgroup markers are attempted/observed lower bounds only.
  Neither statuses nor missing markers fabricate progress or prove absence
  of mutation. Identity-file existence proves neither successful fixture
  creation nor removal. No fake account or cgroup progress is permitted.
- Preserve exact schema/types/closed tokens, duplicate rejection, canonical
  internal encoding, ASCII/single-newline bounds, MAX_TRIPLES and historical
  decide semantics, including allowed regular files and conjunct order.
  Measure both 1024-byte internal and TOTAL 4096-byte outer maxima; reject
  overflow without truncation. Retain first/final ordering and all old
  cross-field semantics except the explicitly accepted version/progress/
  success changes. Provenance remains matching SHA/run/attempt association,
  never attestation. Arbitrary bytes, paths, payloads and raw logs never
  become output or formatted exception arguments.
- Coverage is conditional, not seven-site capture: installed is **never**
  captured; service_stage and harness_stage generally cannot write after a
  stable trust failure; service_cg/service_runs/capture_base/harness_binary
  require an escaping exact Refusal and a successful writer. Service-finally
  masking, first-creator loss, exec, termination and non-Refusal errors remain
  gaps. Absent never means no refusal or successful internal execution.

## Recommended next action and unchanged gates

Have the coordinator obtain a bounded proposal correction for ST-B1, SP-B1
and SP-B2, then a fresh independent design recheck. Do not start implementation
or observation from this review. Re-pin stale authoring-base/CI notes against
the prepared current base; the pending CI follow-up already contains the
discovery-line change, so do not duplicate it in F0b. CI status remains
unverified here. Later delivery still requires a different-provider source
review and all required CI/ci-time-budget/agent-review gates; only the
coordinator may merge. No owner workflow or technical fence is weakened.

The smallest correction keeps the three fixed files, shared validation and
existence-only markers. It needs no retry, extra runtime probe, new production
seam, configurable channel, root-process tracking, or product approval flow.
All proposed tests/mutations remain future synthetic mechanism work, using
mocked effects without privilege; optional owned-temp cases earn no credit
when skipped. No hardware result, root-process escape coverage, reproduction,
hypothesis or fix is credited.

**M2 is incomplete. Real-Runner PA6 and real-record replay remain OPEN. Native,
installed and dedicated paths are unverified on real hardware.** F2 remains
undispatched; ADR 0019 D7/C2 stand. A new source SHA supplies no retry authority,
resets no failure counter and closes no real-record evidence gate.

Standards: one blocker and one advisory; worst is the non-isolating regression
migration. Spec/security: two blockers; worst is overstating completion evidence
as coverage of every writer.
