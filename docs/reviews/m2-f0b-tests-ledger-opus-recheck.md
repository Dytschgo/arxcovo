# M2 F0b: test and ledger correction recheck

2026-11-14. Reviewer: Anthropic / claude-opus-5-5 / high. Single review
sub-agent; no subagents. Builder reviewed: OpenAI / gpt-6.1-sol / medium.
This is a narrow different-provider recheck of the test and evidence
correction that answers blockers ST-B1 and SP-B1 from the prior review
`0c3aa33` (retained as `92df103`). It grants no publication, execution,
dispatch, retry or merge authority.

**Standards: PASS (ST-B1 resolved; no new blocker; one advisory).
Spec/security: PASS (SP-B1 and SP-A1 resolved; no new blocker; one
advisory).** The prior source/security verdict still stands, because
`driver.py` is byte-identical.

## Scope, isolation and exact pins

The cwd equalled the Git top level, an isolated T3 worktree on branch
`t3/f0b-test-ledger-review`. `git status --porcelain` was empty before and
after every run, and HEAD was the prepared target. Paths are
repository-relative. No owner filesystem paths or private state are kept.

| Object | Exact identity |
| --- | --- |
| Reviewed head | `d649f6c4868e60f2c34b7b0e8d58362bca595783` (parent `92df103`) |
| Base (prior review, retained) | `92df103d11a3227f5e6a64020d26bb7175e4dceb`; tree-identical to original `0c3aa3356dbd0b53594860766657776621f85744` (`git diff 0c3aa33 92df103` empty) |
| Prior report | `docs/reviews/m2-f0b-independent-security-review.md`, read in full |
| Delta | `git diff 92df103 d649f6c`: 2 files, +73/−4 (`test_f0b_transport.py`, F0b evidence) |
| `driver.py` blob | `bb5a3004a6420379a2c7fb4cd327d840181d864e` at base and head (**unchanged**) |
| `driver.py` SHA-256 (working file = committed blob, LF) | `b147b117e7287e663ffdc202688877f79c43c150cb271fe927dcb9d445d3551a` |
| `test_f0b_transport.py` blob | `9c633bd…` → `fc24155e0d8b5d4bb7f59643f1a30504e18c0155` |
| `test_f0b_transport.py` SHA-256 (working file = committed blob) | `8088fe6096d5d4eb3113fa817f9376226a07d69d096f09b3a54be8bf39d67b85` (matches evidence) |
| Evidence `docs/evidence/unix-protection-f0b-transport.md` blob | `7c0005e…` → `e78ab2a0460d24c49671f0c05ffb7d6578ba727f` |
| `test_driver.py` / `test_f0_blockers.py` blobs | `41aa2287…` / `3982a55b…`, both unchanged |
| Accepted proposal | `fc36a37136bd5e1bcba50ae3c644c66bb49a7dc7` |

Read: AGENTS, the dytschgo-review skill, the full prior report, the full
delta, the touched `FakeOS`, writer, marker, record and maxima tests, the
mutation runner, the relevant `driver.py` validators, and the Delivery
section of the accepted proposal.

## Checks actually run

All checks ran locally under Python 3.12.10 on Windows, with `-I -B` and
mocks only. No driver CLI, native, Cargo or Rust command was run. Nothing
touched an account, service, cgroup, proc/sys, host stat, VM, owner state,
install, network, GitHub, CI or dispatch. I did not run full unittest
discovery.

| Check | Result |
| --- | --- |
| Documented safe subset (evidence command; `test_cli_wrong_platform_refuses` excluded) | **66 selected, 66 passed**, 0 skipped, 0 errors; printed `MEASURED internal=757 outer=2447` |
| Legacy `test_f0_blockers.mutations()` | **23 KILLED**, 0 errors; driver SHA-256 `b147b117…` unchanged |
| F0b `test_f0b_transport.mutations()` | **40 candidates: 36 KILLED, 4 EQUIVALENT**, 0 errors, no survivors; driver SHA-256 `b147b117…` unchanged |
| Reviewer in-memory attribution probe (scratch directory outside the repo, not committed; exact-once anchors, compiled in memory, no write) | per-test tracebacks below |

## Blocker dispositions

**ST-B1 (fake truncation kill): RESOLVED.**

- `FakeOS.open` now uses `self.files.setdefault(name, b'')` for `O_CREAT`
  without `O_EXCL`. An existing entry keeps its bytes, which matches the
  kernel when `O_TRUNC` is absent.
- New `test_writer_empty_existing_channel_is_not_reused` puts an empty
  `feedback-service` in place. The fake's default metadata makes it a
  regular file, uid 0, mode 0600 and nlink 1, which matches a dead first
  creator. The test then asserts that the channel stays empty.
- Probe under `writer_drops_excl`:
  - The content assertion in `test_writer_and_first_creator` (line 153) now
    **stays green**. The failure moved to the explicit request assertion
    `assertTrue(flags & O_EXCL)` (line 158), which is a flag-request check
    and not kernel behaviour.
  - The new test fails its kernel-faithful content assertion (line 165):
    the record was written into the dead creator's file.
  - Neither kill depends on invented truncation.

**SP-B1 (three untested behaviours): RESOLVED.**

- **V4 carried refusal.** `test_replay_carried_refusal_is_mode_checked`
  sends three `replay` lines through the real untrusted-input path. They
  carry a `harness_stage` refusal under `service`, an `installed` refusal,
  and a `final`-phase refusal, inside an otherwise valid failed-body record.
  Under `replay_carried_guard_omitted`, **all three subtests** fail with
  "RuntimeError not raised". That proves each line is otherwise valid.
  The reviewer's clause probes isolate them further:
  - dropping only the site-allowlist clause fails the `harness_stage` and
    `installed` cases;
  - dropping only the phase clause fails the `final` case.
- **V1 isolation.** `test_v1_relation_isolated_without_type_errors` uses
  `mutation_lower_bound='staged'` with a present all-`absent` summary and
  `first == final == E_BUILD`. Under `v1_relation_omitted` it fails an
  assertion. The old `internal=None` case in `test_cross_fields` now uses a
  failed body. Under the same mutant it also fails an assertion, with 0
  errors and no `TypeError`. Under the real code it still exercises V1.
- **Marker swallow.** `test_marker_failure_swallowed` patches
  `create_channel` to raise `OSError` and calls `self.fail` if anything
  propagates. Under `progress_marker_reraises` the probe shows the
  `self.fail` line (270) is reached. The effect-order test is unaffected.
- The evidence records that the first mutation run stopped on the old
  success/`internal=None` counterfactual's `TypeError` and earned no credit.
  That is consistent with the runner, which raises on any errored run. My
  final runs had 0 errors.

## Standards

ST-B1 is resolved as above. The delta follows the runner's rules: exact-once
anchors, in-memory compilation, a green baseline, errors earn no credit, and
the source hash is checked before and after. The three new mutant names
differ from the names the prior review suggested. That is harmless, because
each one maps to exactly one guard.

**ST-N1 (advisory): the addendum states counts without the changed ledger
lines.** The correction section gives the 36/4 totals and the attribution
prose. It omits the `KILLED … assertion=` lines for the three new mutants,
and it omits that `writer_drops_excl` is also attributed to
`test_writer_and_first_creator` through its `O_EXCL` request assertion. The
runner reproduces both, so the evidence is not wrong, only incomplete.
Optionally paste the seven changed lines.

The prior advisories ST-A1 (swallowed fake assertions) and ST-A2 (smells)
are untouched and remain disclosed. This recheck does not expand them.

## Spec/security

- **SP-B1:** resolved, as above. Required tests 5 and 6 now have isolated,
  assertion-attributed cases for V1, the V4 carried refusal and the marker
  swallow.
- **SP-A1 (outer maximum): RESOLVED.** `test_measured_maxima` now also
  encodes `final = first`: both are `cleanup_capture` with phase `initial`,
  under `cleanup=unconfirmed`. It pins **2447**, which equals the reviewer's
  earlier brute-force maximum. Internal **757** is unchanged. Both are under
  1024 and 4096. The evidence marks 2445 as superseded.
- **Hash correction: VERIFIED.** The evidence now states that the driver's
  raw working bytes and committed LF SHA-256 both equal `b147b117…`. That
  matches this checkout. It also warns that the earlier `dce176c6…`
  working-copy hash must not be read as a committed hash.
- **SP-A2:** unchanged and still disclosed ("Other review advisories remain
  disclosed").
- **SP-A3: dispositions now explicit.**
  - The separate F0b evidence file is declared the coordinator's chosen
    evidence path. Accepted.
  - The `test_driver.py` change is still the one-line `feedback_internal`
    reset in `F0Mechanism.setUp`. Its blob is unchanged since the prior
    review.
  - **SP-N1 (advisory, wording):** the evidence says "The reviewed migration
    explicitly permits the narrow test_driver.py v2 counterfactual changes."
    That overstates the source. The accepted proposal's Delivery list and its
    migration permission name `test_f0_blockers.py`, not `test_driver.py`.
    The change is also a global reset, not a counterfactual. I still accept
    the change on its merits, as before. Record it as a coordinator scope
    disposition rather than as an explicit permission in the proposal.
- **SP-A4 (publication condition):** disposed by the coordinator's statement
  in the evidence. F0b publication will be cut from merged main without the
  Windows diagnostic proposal and history. Those Windows docs stay local and
  will **not** be published in the clean F0b PR. The condition holds until
  that clean PR exists. It is not satisfied by this branch.
- No source change, redesign or advisory expansion appears in the delta.

## Disposition and unchanged gates

Both prior blockers are closed in tests and evidence. The two new items are
advisory wording only and do not block. This recheck is not `agent-review`
and gives **no merge approval**. Publication still needs:

- a clean one-concern F0b PR cut from main;
- CI and `ci-time-budget`;
- a different-provider `agent-review` on that PR;
- the coordinator's merge.

Ordinary CI is unverified here. The Windows ordinary-test `ExecutionFailed`
was not run, retried or diagnosed.

F0b earns local mocked mechanism credit only. **The real-record replay gate
and real-Runner PA6 remain OPEN. M2 is incomplete. All native, installed and
dedicated paths are unverified on real hardware. F2 remains undispatched.**
ADR 0019 D7 stands. A new SHA resets no failure counter and authorizes no
historical retry.

Standards: 0 blockers, 1 advisory; worst is the addendum omitting the changed
ledger lines (ST-N1). Spec/security: 0 blockers, 1 advisory; worst is the
`test_driver.py` authority wording (SP-N1).
