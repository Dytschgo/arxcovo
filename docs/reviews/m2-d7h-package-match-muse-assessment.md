# M2 D7h package match — Muse FILE-leg evidence assessment

Scope: commit `0446d36` atop `73017f5` (parent actual main `47b440a5faeaa24434e155b81d5d03a5c977e4e9`, PR #124).
Read: [AGENTS.md](/abs/path/AGENTS.md), `docs/KICKOFF.md`, ADR0019 D7h
(consumed form in `0446d36`), ADR0020 D9b, official Ubuntu package provenance
plus [15c5d0d review](../reviews/m2-a7-ubuntu-package-path-muse-review.md),
comparator source plus
[82f9e91 review](../reviews/m2-a7-sudo-package-comparison-muse-review.md),
wrapper plus [6d8dab4 review](m2-d7h-package-readonly-muse-review.md) and the
`895af0c` atime fix. Closed record:
[run JSON](../evidence/linux-a7-package-readonly-run-43693642693.json) and
[result note](../evidence/linux-a7-package-readonly-result.md). No
PROGRESS/source/ADR edit in this assessment; no rerun of build, tests,
native/helper reads, host/account/sudo/admin actions, dispatch, variables,
settings, push, PR, or subagents.

## Standards — PASS

- One-concern evidence record: `0446d36` touches exactly 4 paths —
  `docs/evidence/linux-a7-package-readonly-run-43693642693.json` (new),
  `docs/evidence/linux-a7-package-readonly-result.md` (new),
  `docs/adr/0019-unix-native-execution-bridge.md` (D7h status line only),
  `docs/PROGRESS.md` (status line only). UTF-8, LF, no payload bytes,
  identities, unknown hashes, policy, paths, or raw log dump in the retained
  record; logs were read once under 2 MiB per the result note.
- Closed record is complete and machine-checkable: run `43693642693`,
  attempt `1`, job `132749349754`, `workflow_dispatch` on `main`, exact head
  `47b440a`, actor and triggering actor `Dytschgo`, `success`,
  `2026-11-15T08:15:30Z–08:15:38Z` (8 s), six steps all `success`.
- Report binding is exact: `source_sha == workflow_sha == 47b440a`,
  `run`/`attempt` match the closed run. Comparison is
  `candidate_match: true`, `qualified: false`, `schema: 1`,
  `status: candidate_match`, `amd64`, revision
  `1.9.15p5-3ubuntu5.24.04.4`, with all three expected digests
  (`sudo 6ae6665c…`, `sudoers 151d681d…`, `util aae167b9…`).

## Spec — PASS (FILE leg only)

- Grant match: candidate parent is baseline `4f1a6db`; `4f1a6db..47b440a`
  delta is exactly the 7 D7h-allowed paths (workflow, wrapper, wrapper tests,
  ADR D7h text, preparation evidence, `6d8dab4` review, coordinator PROGRESS
  note), new files mode `100644`, no executable blob or mode change
  elsewhere. Squash tree `b490c0479564ad895201e2a7872cb9fd11dad721` equals
  reviewed `895af0c` tree; comparator blob
  `520d341633c67dc52ec483e9b3fcc92131a3da1f` (11180 bytes,
  `12a122db…`) is byte-identical to baseline. PR #124 five required gates
  plus different-provider Muse exact-head PASS precede the squash (coordinator
  record; not re-fetched here).
- Executed-source join verified from the reviewed chain, not from green-job
  status alone: wrapper `_gate` precedes `_load_comparator`, sole call site
  is the existing `comparator._read_linux()` once; loader enforces fixed
  `__file__`-relative sibling, held nofollow directories from `/`, regular
  single-link file, exact size plus SHA256 before `compile`, with the
  `895af0c` atime-insensitive 9-field drift comparison; reader scope is the
  fixed three payloads, two links, ancestor metadata, ELF
  NEEDED/RUNPATH/RPATH, digest, budget, and reopen fences. Report prints only
  on full-schema match (known digests on match, null digests on refusal,
  `qualified` always false, exit 2 refused / exit 0 only on
  `candidate_match`).
- Lifecycle closed: monitor used the returned URL directly, held the
  dedicated pin through that exact run terminal, finally deleted it and
  verified 404; coordinator independently verified both dedicated
  `SUDO_PACKAGE_REVIEWED_SHA` and old native `UNIX_EXECUTION_REVIEWED_SHA`
  absent 404 afterward. D7h is consumed; no retry, rerun, redispatch, or host
  reuse remains.

## Security — PASS with stated trust boundary

- Substitution/redispatch fences hold for this leg: triple-SHA equality,
  workflow SHA/REF, attempt-1, bounded run ID, repo/actor/trigger/ref/OS
  gates, plus current-main readback before pin, per `6d8dab4`. Old-variable
  authority refuses. No secrets, artifacts, permission expansion, inputs, or
  extra host collection in the executed path.
- Fail-closed posture preserved: any gate, load, read, schema, or drift
  failure emits only the constant refused shape. The atime fix (`895af0c`)
  removes the known spurious-refusal burn of the one-shot grant; impact was
  availability-only, never a bypass.
- Residual trust is explicit and unchanged: privileged concurrent
  writers/mounts and root-protected OS dependency resolution trusted per
  D8/A1; fences are observations, not whole-host attestation. Read-only
  refers to this comparison leg; checkout and sudo transport keep their
  platform-managed effects.

## Credit granted

- Installed FILE-leg comparison only: the pinned Ubuntu 24.04
  `1.9.15p5-3ubuntu5.24.04.4` `amd64` image's three installed payloads match
  the official Ubuntu source/binary association and compiled defaults under
  D8/A1 privileged-writer trust. This is source/run entailment through the
  reviewed join, not reproduced-build proof.
- Expressly NOT granted: C2/alias/floor, effective in-unit four read-only
  mappings/literals/NSS, artifact/account admission, either real NNP
  allow/deny case, host qualification (`qualified` remains false), or any
  A7/A11/M2/full-F/G/PA6 acceptance. Installed/dedicated paths remain
  **unverified on real hardware**. No reusable host or native authority
  survives; D7h is consumed.

## Remaining limits and next step

- This host cannot be reused. Do not run another diagnostic loop on it.
- Minimal next work is an ACTUAL integration under a new grant on a fresh
  host: revalidate package plus C2/floor, the read-only fixture view, and
  both NNP cases — each needs its own reviewed disposition, not a
  redispatch of D7h.
- A metadata-API read was available but unnecessary: the retained closed
  schema plus binding is sufficient for this FILE-leg credit, and no raw
  payload or log bytes were needed or retained.
