# Independent Opus assessment of the consumed D7l A12 attempt

2026-11-15. Anthropic Claude Opus 5.5 / high, review sub-agent, no subagents.
Independent of the Sol builder of PR136-138 and of the coordinator's disposition text.

**Verdict: the closed failure disposition is ACCEPTED as written. Standards PASS,
Spec PASS, Security PASS (no security finding). One interpretation constraint
(I1) is added below; it does not contradict the record. No A12 credit.**

## Pinned target

- Assessed parent: `0802f168cace09173e4a729c7ef8a589941c453e` on top of
  `3d41ace06b9e21a4ad3d012cc06b661992413d17` on top of merged main
  `2a1d5ad32cfb62cac3fb87c8a799b857e1a5f59c`.
- `git diff --stat 2a1d5ad 0802f16` touches only PROGRESS, ADR 0019 (D7l
  CONSUMED banner), `docs/evidence/linux-a12-hosted-result.md` and
  `docs/evidence/linux-a12-run-43741300777.json`. The driver and all non-doc bytes
  are the same as the dispatched main.
- Read: AGENTS, KICKOFF, ADR 0019 D7l, brief 0020 A1-A12, ADR 0020 A4/A9 text,
  the result disposition and closed record, the retained A12 one-run source review
  and recheck, the delivery and residue reviews, and the actual driver
  `run_body`/`cleanup_a1a2`/`cleanup_a12`/`prepare_a12`/`snapshot_internal`/
  `read_internal`/`write_internal`/`monitor_a1a2`/`harness`/`validate_record`/`entry`.

## Checks actually run here, compared with the coordinator's facts

| Coordinator fact | This assessment | Result |
| --- | --- | --- |
| PR138 head `3cf4a33`, merged `2a1d5ad` at 17:28:03Z | public PR API | equal |
| Exact-head PASS comment 6898267544, status 58969899019 | public comment/status API: Opus PASS heading; `agent-review` success id 58969899019 | equal |
| Five green gates | `ci-time-budget`, `agent-review` statuses plus three `verify` check runs, all success | equal |
| Reviewed-head and squash full tree `68db17fb` | `git rev-parse` of both `^{tree}` gives `68db17fb0bb41aa6991712dbf3a4b898d1dfe3a9` | equal |
| 223 non-doc rows, digest `dbc11a5a...c710` | recomputed from `git ls-tree -r 2a1d5ad`, excluding `docs/`, rows `mode\tblob\tpath\n` sorted by path | 223 blobs, `dbc11a5ad3a0e37d6bf659b1e1bac65c2758565d8a11cf5c500bb415e375c710` |
| Run 43741300777 attempt 1, job 132894851824, main `2a1d5ad`, failure | public run/job API: workflow_dispatch, main, actor/trigger as recorded, `ubuntu-24.04`, attempt 1, failure | equal |
| 67 s, 17:28:37-17:29:44Z, 8 steps green, 1 root fail | job API: steps 1-6, 14, 15 success; step 7 (guarded root step) failure, 17:28:57-17:29:43Z (46 s) | equal |
| Record passes committed `validate_record` | loaded the committed driver with `python -I -B` and ran `validate_record`, `encode_record` and `replay` on the feedback object only: valid, 442-byte line, no E_PROTECTION entries. Module flags: both A12 flags true, `TEST=exec::a12::native::disposable_linux_a12` | equal |
| All three pins absent before and HTTP404 after; monitor held the pin | **not rechecked**: variable endpoints are out of scope here | coordinator-attested only |
| Raw logs bounded, not saved or printed | not observable from here; no raw log was fetched by this assessment | coordinator-attested only |

No tests, builds, native code, host/proc/sys/account operations, Runner, LinuxFs,
sudo, dispatch, variable access or job-log access were performed.
One deviation from the scratch-location rule: the inventory rows were written to a
~25 KB file in the Windows C: temp directory and deleted at once. It held only
public git mode/blob/path rows.

## What the record shows, read against the source

This reading assumes the executed bytes match reviewed main. The provenance
envelope binds `source_sha == workflow_sha == 2a1d5ad`, but the driver itself calls
it "association, not attestation". These statements limit which paths are possible.
They do not name a cause.

1. **First `E_SERVICE_OR_TEST`.** In the outer run path, only `monitor_a1a2` raises
   this code: either the ACK `MainPID` comparison or the terminal-state check
   (`acknowledged`, `Result == success`, `ExecMainStatus == 0`). The record does not
   say which one. In both cases `verify_test_result` was never reached, so the
   NORMAL/CANCEL/COMPLETE markers and the one-test summary were **never
   evaluated**. The attempt produced no evidence for any A12 oracle.
2. **Internal `absent` ×3, carried/refusal null.** `snapshot_internal` replaces its
   `unknown` default only if `systemctl stop` returned and the unit cgroup was
   observed absent. `absent` means `read_internal` found no channel file.
   `write_internal` writes a channel only for a typed `Refusal` from a trusted
   internal mode. So the three `absent` values mean only that no internal
   E_PROTECTION refusal was captured. They are **not** success evidence for the
   service, fixture-check or harness.
3. **Mutation lower bound `cgroup`.** This was raised by a root-owned regular
   `progress-cgroup` stage file, which the in-unit harness writes just before it
   `execve`s the test binary. It is a lower bound, not an inventory.
4. **Final `E_COMMAND_FAILED`.** The cleanup in the `finally` block raised a
   `CalledProcessError`, and it replaced the original exception, while the
   already-set `first` was kept. A non-zero stop would have produced
   `E_QUARANTINE_CGROUP`, and an unrun snapshot would have left `unknown` or
   failed validation. So the failing `check=True` call came later in the owned
   cleanup sequence. The record does not identify that call, and this assessment
   does not pick one. Account, group, `/var/lib` root/workspace, runtime unit
   files and stage removal are each **unconfirmed**. The fixed-name reservation
   lock is kept on purpose, because cleanup is not `confirmed`.

**I1 (interpretation constraint, not a contradiction).** The `account` rung of the
mutation ladder is raised only by the `fixture-identity` marker. The A12 account
is created in `prepare_a12` (groupadd/useradd, `/var/lib/arxa12…` root and
workspace) *before* the ladder reaches `service`. Any record at `service` or above
under the A12 selection therefore implies that `prepare_a12` returned, which means
the driver created a fixture account even though the bound reads `cgroup`. The
disposition's sentences "lower bound, not an exhaustive effects inventory" and
"does not establish ... account removal" are correct. Readers should go further:
the source implies the account **was created**, and its removal is unknown. This
assessment proposes no change for it. It is noted so that `cgroup < account` is
never read as "no account was created".

## Standards — PASS

- The disposition commits are docs-only (PROGRESS, ADR banner, evidence markdown
  and JSON), UTF-8/LF, mode 100644, with no executable or source change. Each
  commit is one concern with a conventional subject.
- The closed record is sanitized. It holds fixed codes, public run/job metadata and
  public SHAs, with no raw log, private payload, secret or owner data. The
  `actor` login is already public run metadata.
- Minor readability note (no change requested here): the D7l section now begins
  with a CONSUMED banner, followed by the historical "No effective grant yet"
  paragraph. The banner states that the text below is historical, so the two do
  not contradict.

## Spec — PASS

- D7l says any accepted dispatch consumes the grant. It is consumed exactly once,
  by the URL returned at dispatch, attempt 1, on the first eligible main squash.
  Tree and inventory equality were independently reproduced here. Nothing was
  retried, rerun or redispatched, and no diagnostic leg, fence relaxation, new
  grant or host reuse is claimed.
- D7l allows credit only after true Exited/0, real post-spawn cancellation, ACK,
  markers, summary, residue=false, held-leaf ENOENT and independent final cleanup.
  None of those are evidenced (item 1 above). **No A12 credit**, in any form.
- `cleanup: unconfirmed` rules out any clean-host, acceptance or reuse claim.
  Excluding the host is correct and required.
- The disposition infers no cause and proposes no fix, run or grant. This
  assessment adds none.
- A7/A11 hosted FINAL NO-GO is unchanged and stays closed. It is a separate item,
  and this attempt resets no counter.
- PROGRESS still lists crash/A9/U9, A7-A12, full F/G and real-Runner PA6 as
  open, and M2 as incomplete. A12 stays **unverified on real hardware**.

## Security — PASS, no finding to escalate

- No new privilege, network, secret or policy surface was added. The fences
  worked as designed: the reservation lock was kept on uncertainty, and no
  replacement was adopted.
- The possible residue (a nologin, passwordless fixture account plus root-private
  directories and runtime files) is confined to one ephemeral GitHub-hosted VM
  that is excluded from reuse. Nothing points to exposure beyond that VM. This
  is a cleanup-uncertainty fact, not a vulnerability.
- Pin absence comes from the coordinator's HTTP404 checks and was not rechecked
  here. It is not evidence of host cleanup, and the disposition correctly says so.

## Precise credit (closed accounting only)

Credited: the D7l grant was consumed once by an accepted dispatch of the exact
reviewed main, and a sanitized closed record exists that passes the committed
validator. Its values are first `E_SERVICE_OR_TEST`, final `E_COMMAND_FAILED`,
lower bound `cgroup`, cleanup `unconfirmed`, and internal absent/absent/absent/
null/null. Not credited: any A12 oracle, normal or cancel completion, residue
correctness, cleanup, account removal, clean host, cause, A7/A11, full F/G,
real-Runner PA6 or M2 completion.

## Next independent remaining M2 item (no native work)

Among the open items, A12, A9/U9 (whether systemd removes delegated children at
unit stop) and real-Runner PA6 can only be closed by native observation, and
none of them has a live grant. Full F/G depends on those F items. **A10**
("cgroupfs writes and migration need no capability beyond the drop-in's bounding
set", currently an ADR 0019/0020 inference) is the one open item that can be
narrowed without native work, by joining evidence that is already accepted.

Bounded handoff (Sol / high builds, Opus reviews; docs/evidence only):

- **Inputs.** The accepted hosted records and reviews for A1/A2, A3 Standard,
  A4 present-residue/omitted-kill and A5; the packaged
  `50-arxcovo-standard-rights.conf` (`CapabilityBoundingSet=CAP_SETUID CAP_SETGID
  CAP_CHOWN`); and the production setup/janitor cgroupfs calls in
  `exec/hierarchy`.
- **Work.** For each accepted run, establish from its record whether the Standard
  drop-in was loaded. Then map each cgroupfs operation that production code
  performed in that run (mkdir, `subtree_control`, kill, rmdir, `cgroup.procs`
  migration) to that evidence. Do not infer anything for unexercised operations.
- **Output.** One evidence note listing which A10 sub-claims are covered by
  accepted records and which remain inference. Run-leaf migration of a real
  Runner child is expected to remain open, because A12 produced no such
  evidence. A10 stays open unless every sub-claim is covered.
- **Fences.** No native run, dispatch, variable, host or grant; no source or
  production change; no reuse of the A12 record as positive evidence.

Independent M3 work may proceed in parallel or next, as the user steers. Do not
reopen A12 or A7.
