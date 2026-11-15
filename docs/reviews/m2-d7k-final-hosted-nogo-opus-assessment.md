# D7k final hosted A7/A11 NO-GO: Opus disposition assessment

2026-11-15, Anthropic claude-opus-5-5, high, review sub-agent, no subagents.
Branch `t3/m2-d7k-hosted-nogo-assessment`, parent `f7697f2` atop `2ae25b4` atop
main `07d88692e2f1480c6e18ff5df846cfb871a18033` (PR #134). I am the owner of the
D7k attempt, so this is the correction-owner disposition, not the cross-provider
review. Git reads only. No build, test, host, root, native, sudo, account, mount,
policy, network, dispatch, variable, push, PR or merge action. No GitHub log or
API query: I reuse the committed sanitized record and the coordinator's pin
evidence.

## Decision

**Hosted A7/A11: NO-GO. Final and irrevocable under the pre-registered D7k rule.**

- Run `43728169361` attempt 1 is a miss. The closed record does not show an `exit_zero` with
  `no_new_privs=true` security outcome; the NO-GO miss branch applies without
  inferring an unobserved sudo result.
- **No acceptance.** No A7, A11, M2, full F/G or real-Runner PA6 credit. A7/A11
  stay **unverified on real hardware**, pending a dedicated Linux host owned by
  the owner (systemd >= 254, kernel >= 5.14, cgroup v2, no writable
  `/usr/local/bin`). Providing that host is not an agent admin step.
- D7a–D7k are all consumed. No retry, rerun, image retry, diagnostic leg,
  read-only follow-up, new grant name, fence or timeout relaxation, hosted bind
  or source correction. This assessment does not reopen the hosted route, and
  no later assessment can.
- Next: the remaining M2 items that do not depend on A7/A11 (crash/A9/U9, A10,
  A12, full F/G, real-Runner PA6), each under its own bounded disposition. Then
  independent M3. **M2 is not complete.**

## Inputs read

- PROGRESS current state and the latest task-history entries.
- The [achievability decision](m2-a7-a11-opus-hosted-achievability.md) and the
  [attempt-ceiling handoff](m2-a7-a11-cross-provider-correction-handoff.md).
- ADR0019 D7k, including the consumed banner added in `f7697f2`.
- The [preparation evidence](../evidence/linux-a7-final-opus-execution-preparation.md),
  including its actual-dispatch appendix (`2ae25b4`).
- The [hosted result](../evidence/linux-a7-final-opus-hosted-result.md) and the
  [closed record](../evidence/linux-a7-final-opus-run-43728169361.json).
- The `validate_record` and `CODES` definitions in
  `tools/unix-exec-validation/driver.py` at `07d8869`. I read these only to
  check that the record has a legal shape.

## Standards

**PASS.**

- Every file in `07d8869..f7697f2` is UTF-8 with LF line endings (byte count:
  zero CR). `.gitattributes` sets `eol=lf`. The record parses as JSON.
- Scope is docs-only. `f7697f2` changes PROGRESS, the ADR0019 D7k banner, the
  new result note and the new record. `2ae25b4` changes PROGRESS and the
  preparation appendix. Neither touches source, tests, workflows or policy.
- The ADR deviation is recorded where it belongs: a consumed banner on D7k, with
  the rest marked historical. No new ADR is needed, because the outcome is a
  pre-registered branch, not a design change.
- PROGRESS records the NO-GO, the hardware label, the consumed budget, and the
  next work, in the order the steering asks for. It does not claim M2 is done.

## Spec

**PASS.** Each D7k requirement against the evidence:

| D7k requirement | Evidence | Verdict |
| --- | --- | --- |
| The candidate is the first eligible main squash containing D7k | PR #134 squash `07d8869` after Sol exact-head PASS 6896501718 and five greens (supplied coordinator facts) | Met |
| Reviewed-head and squash trees are equal | Recorded as `a85a093` = `ec024a1`. I re-read `07d8869^{tree}` = `ec024a1fdbd5801f13912951bcf59d951d82e1f8` | Met |
| Strict docs-only predicate against `52ce2a5` | I re-ran `git diff --name-status` and `--summary`: exactly the four allowed paths, two new 100644 files, no deletions, renames or mode changes | Met |
| Non-doc inventory equals the recorded one | I recomputed it at `07d8869` and again at `f7697f2`: 222 records, 19629 bytes, SHA256 `04a7f684092042c0001598725d6c9d3e6890e2d9a5d8dc5d7842ab3557d749b3`. Spot-checked blobs (`sudo_nnp.rs`, `driver.py`, `unix_shim_rights_linux.rs`, the experiment workflow) match the pins | Met |
| Run is bound to the exact source | Record: `source_sha` = `workflow_sha` = `07d8869`, run `43728169361`, attempt 1, job `132854558246`, `workflow_dispatch`, `main`, actor and triggering actor `Dytschgo`. The provenance inside the closed record matches | Met |
| One dispatch, URL captured, monitored to terminal | The appendix and PROGRESS record the direct returned URL and monitor 55560 to terminal. There is no second run ID anywhere | Met (supplied) |
| Pin lifecycle | All three pins absent before the set. The native pin was set once and read back, kept until terminal, then cleared by the monitor. The coordinator independently repeated the check after terminal and confirmed all three return HTTP 404 | Met (supplied, not re-queried) |
| Budget consumed whatever the outcome | The ADR banner, the result note and the record (`retry_authorized: false`) all say so | Met |
| Outcome classification | `conclusion: failure`, 121 s, eight steps passed and one root step failed. First and final codes are `E_ARTIFACT`. That is a miss outside the security branch, so it is NO-GO | Correct |
| Retain bounded evidence and exclude the host | Fixed closed schema extracted once, within 2 MiB. No raw log is kept. The result note says the host is excluded | Met |

### Closed accounting and precise credit

- The record is a legal closed `unix-protection-f0` v2 shape:
  - `internal` is null. That is required exactly when `mutation_lower_bound` is
    `none` or `staged`.
  - `cleanup` is `confirmed`, which requires `first == final`. Both are
    `{"code": "E_ARTIFACT"}`, a member of `CODES`.
  - The record has no inconsistent fields.
- The record retains no case evidence: no CASE0, CASE1 or COMPLETE marker,
  no NNP value, no candidate choice and no sudo outcome. `staged` is a LOWER
  bound, not proof of absent effects. Coordinator Sol source review notes that
  `prepare_controlled_sudo_nnp` and `handoff_sudo_nnp` run before the driver
  advances its marker to service, so account effects cannot be ruled out from
  this record. Confirmed cleanup is the reported postcondition, not a claim
  that nothing needed cleanup.
- **Precise credit: none.** Not partial A7, not A11, not C1 identity, not the
  five-view proof. Earlier hosted credits (A1/A2, A3, A5, D9a A4) are neither
  widened nor reduced by this run.
- `native_credit: false` and `hosted_route: NO_GO_PENDING_DEDICATED_HOST` are
  correct.

### No cause inference

- At `07d8869`, `E_ARTIFACT` is raised from about thirty separate `require`
  lines in `driver.py`. They cover image digests, size bounds, the
  digest-candidate set, cargo artifact selection, ELF and copy counts, staging
  writes, the `sudo_nnp` digest pin, account/artifact pairing, the record size
  bound, and `/proc/<pid>/exe` identity.
- The closed record names no site. So nothing in the record or the source
  attributes this miss to a specific site, to host image state, or to the earlier
  D7i/D7j observations.
- I make no historical-cause claim and propose no fix. The result note correctly
  declines both.

## Security

**PASS. No security finding.**

- **No `exit_zero` with NNP true is shown or inferred.** The closed invariants
  above mean no case ran to a sudo result. A multi-site `E_ARTIFACT` code cannot
  imply any NNP or sudo outcome, and I infer none. The security-escalation
  branch does not trigger. It also cannot be "ruled out" for real hardware: that
  question stays open with A7/A11, pending a dedicated host.
- **Full shutdown.**
  - Cleanup is `confirmed`.
  - The native pin was cleared after terminal. The other two pins were never
    set. All three are independently 404.
  - No grant, variable or run authority remains for hosted A7/A11, and the host
    is excluded.
- **No private data.** The record holds only public commit SHAs, public run, job
  and attempt IDs, the public repository login, closed enum codes and ISO
  timestamps. There is no raw log, payload, path, account name, environment
  value or unknown hash.
- **No owner admin action.** No Lab Manager, static VM or backup action was
  taken or requested.

## Advisories (non-blocking, coordinator's choice)

1. The preparation appendix still ends "run inprogress". The ADR banner and the
   result note supersede it, and the appendix is dated and historical. No edit
   is needed.
2. The excluded-host lists in D7k and the preparation evidence do not yet name
   `43728169361`. The result note does exclude it. The coordinator checkpoint
   may add it to the list for the dedicated-host follow-up.
3. The pin evidence in the record is a single boolean with no timestamps. That
   is acceptable under the reuse-only rule. Its authority is the coordinator's
   independent post-terminal check recorded in PROGRESS, which I did not
   re-query.

## Verdict

Standards PASS. Spec PASS. Security PASS. **Hosted A7/A11 NO-GO is enforced
correctly and is final.** No acceptance and no credit. A7/A11 stay **unverified
on real hardware** pending a dedicated host. Remaining M2 work comes first, then
independent M3. M2 is not complete. This assessment grants no run, pin, grant or
reopening.

## Coordinator Sol correction at publication

2026-11-15: independent source check of run_body corrects the earlier inference
from staged to no-account-effects. Account provisioning can precede the service
marker; the retained lower bound cannot establish absence. No case evidence or
sudo security outcome is inferred. This wording correction does not alter the
closed JSON, consumed grant, confirmed-cleanup report or final NO-GO decision.
