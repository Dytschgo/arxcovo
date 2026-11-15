# M2 A9/U9 stop and drain evidence join: Opus independent review

2026-11-15; Opus 5.5 high review sub-agent; documentation only.
Target: Sol note `docs/evidence/linux-a9-u9-stop-drain-evidence-join.md`.
Builder `e5779a3` (parent `657b825`) retained as `1f8a8b6` (parent `e845a95`);
the note is byte-identical in both. Reviewed tip `5056d90` (PROGRESS +2 only).

## Verdict

Standards PASS, Spec PASS, Security PASS; no blockers. **Eligible for
publication** as evidence only; A9/U9 stay OPEN; gates/merge are the
coordinator's.

## Standards: PASS

- Scope: `1f8a8b6` adds only the note (+89, accepted against the A10
  review's "about 80"); `5056d90` adds only PROGRESS (+2).
- UTF-8, no CR or trailing whitespace; non-ASCII is only the `→` arrows on
  lines 36–38. All 29 relative links resolve from `docs/evidence/`.

## Spec: PASS

Checked in this session from the executed blobs, not reused:

- **Run binding.** Each local record or record review shows its run ID,
  attempt 1, `cleanup: confirmed` and the table's exact SHA: A1/A2 `1e9d881`,
  A3 Standard `beb7117`, A3 None `4bb1561`, A4 positive `a328060`,
  A4 omitted `1958f99`, A5 `cf3a251`. Each SHA's experiment workflow pins
  `ubuntu-24.04`.
- **Driver order** at all six SHAs. `run_body` runs `systemctl start
  --no-block`, then `monitor_a1a2`. The monitor acknowledges MainPID, `exe`
  and `/system.slice/arxd.service/supervisor` membership, then requires
  `inactive|failed`, `Result=success`, `ExecMainStatus=0` and exactly one
  passing test. In `finally`, `cleanup_a1a2` runs `systemctl stop` (40 s,
  must exit 0), snapshots, then requires `not lexists(.../arxd.service)`.
  Only after that does it remove the A4 helper (A4 only), the runtime files
  and the stage, and set `confirmed`. The driver's `rmdir` calls target only
  owned runtime directories or the helper directory, never a cgroup. The
  fixture override (`ExecStart` python driver, `Restart=no`,
  `RuntimeMaxSec=140`) adds no `ExecStop*`.
- **Driver deltas** match the note (profile/`verify_runtime`; `TEST` only;
  A5 selection; helper after absence; omitted selection/marker); none changes
  the stop/absence order.
- **Runs state at exit.** A1/A2 `prepare_a1a2` requires `runs_created` with
  `leaves_removed == 0`. A5 runs `a5::finish`, then `a5::empty`. A4 positive
  runs `a4::finish`, then `a5::empty` before READY. A4 omitted proves
  rejection while the helper is still unreaped, then `retained_a4`, the marker,
  `cleanup_failed_acceptance` and `cleanup_a4`, then `a5::empty` before READY.
  A3 only asserts hardening and creates no `runs`. "No populated leaf at exit"
  is exact.
- **Claim bound.** The fixture exited naturally, so stop hit an already
  inactive unit. The note credits only systemd cgroup removal at deactivation
  (`supervisor/`, empty `runs/` where created) and withholds signal stop,
  SIGTERM, `KillMode=mixed`, populated-leaf, crash and production Drain
  credit, matching the A10 review limits and ADR0020 D9/U9.
- **Historical versus current code.** Historical claims are pinned to SHAs
  (the driver link uses the current path, but the text says "at each SHA").
  Production claims are labelled "At the parent" and match current source:
  `Signals` term/int cancel; lifecycle runs `stop_accepting`, cancel, a 10 s
  drain (timeout → `ExitCode::Store` = 5), a 5 s audit only if drain
  returned, then `release`; `Application::drain` joins serving, then
  `Execution::drain` (shutdown, join workers), then `blocking.drain()`, with
  the discovery join `#[cfg(test)]`; runner `kill_tree` reaping is bounded at
  2 s; `Group::kill_and_confirm` writes `cgroup.kill`, polls `populated 0`,
  calls `unlinkat`, and its `Drop` sets `residue`; the unit has
  `KillMode=mixed` and `TimeoutStopSec=30s`. Both named tests exist
  (`service/tests.rs:207`, `e7.rs:723`).
- **Status.** A9/U9 OPEN, A10 open, M2/full F/G/PA6 open. A12 failed on
  `2a1d5ad` with cleanup unconfirmed and gets no credit. A7/A11 FINAL NO-GO
  stays closed. No grant, diagnostic, activation or native proposal.

Reused, not re-derived: A10 review/recheck A1/A2-vs-A3+ attribution.

## Security: PASS

No secrets, tokens, private paths, log payloads, owner data or new authority.

## Advisory (optional; does not block)

- A-1: Line 5 calls `657b825`'s A10/PR139 work "pending"; in the retained
  lineage PR139 (`0df242e`) is merged and A10 published (`e845a95`). The claim
  (no A9 credit from either) holds; "builder parent" would be more exact.
- A-2: The A12 line could add "D7l consumed; no retry" (the linked result
  states it; the note proposes nothing).

## Next independent M3 recommendation: consistent with ADR0005

The proposed slice (KICKOFF line 19: 15-minute single-use join code, plus
line 75: rejection of expired or reused codes, with CA/mTLS routed to Opus)
matches ADR0005 lines 13 and 31. ADR0005 is still *proposed*, and lines 24
and 26 require settling clock/skew fail-closed rules and CA-key decisions
through security design before coding. Start with an Opus design/ADR update.
It should cover: the hashed-secret redeem transaction, deterministic
authorized retry, a race between two keys, no certificate before commit,
and clock rollback. Build source only after that. This does not close M2.

Checks run: git object/diff/log/blob reads at the six SHAs and parent;
current source/doc reads; link, UTF-8, CR and whitespace checks. No tests,
builds, network, host, native, root, Runner, dispatch, variable, push, PR,
comment, status or merge actions.
