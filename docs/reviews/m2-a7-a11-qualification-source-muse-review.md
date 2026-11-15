# M2 A7/A11 qualification source review - Muse (independent source/security)

Scope: Sol injected qualification collector `b4c3205` (228-line
`tools/unix-exec-validation/sudo_host_qualification.py`, 279-line
`tools/unix-exec-validation/test_sudo_host_qualification.py`, 104-line
`docs/evidence/linux-a7-a11-qualification-source.md`) on base main
`d18a721` (PR119) plus PROGRESS-only `3be59fe`/`3025977`. Reviewed at HEAD
`3025977`, worktree clean before this review file. Read-only: AGENTS.md,
`docs/KICKOFF.md` M2/routing, accepted sudo-NNP plan plus Muse plan review,
delivery contract `docs/reviews/m2-a7-a11-delivery-contract.md` plus Muse
contract review, prior Muse source/delivery-source reviews, both new Python
files in full, and the evidence file. No implementation, build, dispatch,
account, sudo, probe, push, or native operation ran beyond the explicitly
permitted pure injected test rerun below. A7/A11/M2/full F/G/real-Runner PA6
remain **unverified on real hardware**.

## Standards - PASS, no findings

- One-concern docs-side source slice: exactly two new tool files plus one
  evidence file; no driver, fixture, workflow, packaging, TEST, ADR, or
  PROGRESS bytes touched by this review. Small conventional scope, UTF-8 LF,
  Markdown/Python only, no secrets or owner data.
- Consistent with settled style: source-only qualification language,
  hardware-unverified labelling, closed `qualified=false` posture, and
  `agent-review` before merge. No product approval flow, no public-publishing
  default change, no host-audit framework, no diagnostic ceremony added.
- Smell baseline: no new abstraction to flag. `_Reader` is the requested
  syscall-shaped port surface, not a Middle Man; `_empty`/`_need`/`_snapshot`
  remove duplication rather than add it; fixed caps/labels are the requested
  contract surface, not speculative generality. Tooling-enforced items
  (formatting) are out of scope.

## Spec - PASS (source-only qualification slice; eligibility bounded below)

The change implements the accepted delivery-contract qualification precursor
and nothing else:

- Injected ports only: `_collect(ports)` is private and takes every byte
  through explicit mock read ports. Module import performs no collection; the
  only CLI entry `_cli()` takes no arguments, constructs no reader, prints the
  fixed `root_reads_not_authorized` record, and returns exit 2
  (`test_cli_never_constructs_reader` poisons `_Reader` to prove it).
- Fixed root-down read-only discipline: `open_root`/`openat` always pass
  `readonly/nofollow/cloexec/nonblock`, directories add `directory=True`,
  `openat` takes one component only (recursed per component from the held
  root fd). No path-based convenience reads, no caller-supplied paths.
- Ownership/type/link fences on every level: `_protected` requires uid 0, no
  group/other write bits, directory vs regular distinction, and `nlink == 1`
  for files. Ancestor directories, policy files, C1/C2, and every
  `sudoers.d` member all pass through it; the mock suite flips uid/mode/link
  at each level and expects refusal.
- Caps and stability: each policy file `<=16KiB` with pre-read size check
  (oversize refuses before any `read` call is asserted), the `sudoers.d`
  set `<=32` files / `<=16KiB` each / `<=128KiB` aggregate, entries bounded
  (`limit=33` requested, `<=32` enforced, uniqueness/charset/drift refused),
  C1/C2 `<=16MiB` ELF with magic plus executable bit, double-read drift
  (`names == reader.entries()`) and held-descriptor snapshot stability
  (`unchanged()`) before any result. Bounded chunk reads (`<=4096`) with
  exact `len(data) == st_size` match refuse truncation/padding.
- Complete include entry set with aggregate framing: all names enumerated
  are all read (including ones sudo might ignore), names plus bytes are
  length-framed into one aggregate `include_set` digest, never a per-entry
  listing or per-entry hash oracle. Fixed output keys and six fixed record
  labels only; records hold `{sha256, size}` digests, never bytes.
- ELF/alias/floor are structural observations only: `protected_elf` means
  bounded ELF magic plus exec bit under protected ancestry, claimed as
  observation not trust; `/bin` must be a root-owned link reading exactly
  `usr/bin` or `/usr/bin` with stat/readlink snapshot equality; the floor
  port is a fixed pure check (systemd `254..65535`, kernel `>= (5,14,0)`,
  cgroup2/memory/pids/user-delegate/hierarchy booleans), not host discovery.
- Failure discipline is constant: any exception and any `close` failure
  reduce to `_empty("refused")` with no messages, paths, identities, raw
  bytes, or traceback. The mock suite asserts stdout/stderr silence, full
  fd closure, and absence of private markers, entry names, `/etc`,
  `Traceback`, and `st_uid` from the JSON encoding.
- Qualification can never succeed in this slice: `_REVIEWED_POLICY` is
  `()`, every snapshot starts `qualified=false`/`policy_reviewed=false`,
  success path returns `policy_unreviewed` and re-asserts
  `_need(not _REVIEWED_POLICY)`. Unknown includes, plugins, NSS routes, and
  every other unknown byte therefore cannot qualify; the mock suite feeds
  `@include`, `Plugin`, foreign `sudoers:` routing, and plausible-but-valid
  syntax and still gets `policy_unreviewed` without touching private paths.
  Digests are never presented as semantic proof and no future hash is
  claimed; no allowlist, parser, or audit framework is added.

## Security - PASS WITH HARD CONSTRAINTS (source-only; adapter/grant still required)

- No new exposure: imports are exactly `hashlib`, `itertools`, `json`,
  `stat` (AST-verified, zero `open()` calls, zero `os`/`sys`/`subprocess`/
  `socket`/`environ` tokens). No OS adapter, driver import, subprocess,
  environment selector, network, write, or proc port exists in the collector.
- No raw policy/identity/pathname release by construction: `_digest`
  publishes only hex plus size; `_collect`'s docstring and both `except`
  paths prohibit raw bytes, names, stat identities, and tracebacks from
  leaving the call, including on errors; `close()` failure also refuses.
- Load-bearing assumptions are stated honestly, not proven here:
  root-writable ancestry makes the held-descriptor snapshot meaningful;
  concurrent privileged writers are outside the trust boundary and the
  output must never serve as execution-time attestation; ELF magic is not
  proof of a functioning sudo; the floor port does not substitute for the
  existing hierarchy/delegation checks or a real adapter measurement.
- The exact policy review remains the acceptance blocker: no public
  source-literal candidate is selected, pinned, or assumed equal to any
  host; digest-only output cannot review private bytes, and no private
  comparison channel is created by this change.

## Actual checks run and limits

- `python -B tools/unix-exec-validation/test_sudo_host_qualification.py`:
  10 tests PASS in 0.139s, rerun here on Windows (this rerun, not Linux).
  The passing set includes `test_security_mutants_are_killed_in_memory`
  (17 in-memory mutants assertion-killed by the 9 base tests).
- AST/import audit of `sudo_host_qualification.py` rerun here: imports
  exactly `hashlib, itertools, json, stat`; no `ImportFrom`; zero `open()`
  call nodes; no `os`-attribute nodes; no
  `os/sys/subprocess/socket/pathlib/environ/Popen/exec/eval` tokens.
- Key-constant grep rerun here: `_REVIEWED_POLICY = ()` (line 25),
  `policy_unreviewed` success only (line 200), `_need(not _REVIEWED_POLICY)`
  (line 209), `root_reads_not_authorized` CLI only (line 223); sole
  `qualified` literal is `False` (line 56). PR119 gates/hosted compile are
  cited coordinator evidence, not rerun here.
- Limits: local run only; no host reads, root reads, sudo, accounts, proc,
  native/helper execution, install, dispatch, variables, push, PR, or
  subagents. Real floor provenance and any host-byte review are unrun by
  design. A Linux read adapter with exact floor-measurement provenance does
  not exist in this tree.

## Next eligibility

- Eligible next: independent exact-head review and ordinary publication of
  this source-only slice; then the settled follow-ups in order: (1) select
  and independently review a closed public source-literal policy candidate
  (complete bytes, include names, plugins/routing, NSS/defaults, both
  account cases) through the settled private-review route, (2) review the
  Linux read adapter with exact floor-measurement provenance, (3) obtain the
  new narrow root-read-only grant before any real capture. Execution needs
  its separate fresh-host grant thereafter.
- Not eligible: any A7/A11, M2, full F/G, or real-Runner PA6 claim; any
  provisioning, selection, execution, host-byte release, literal/hash
  allowlist enablement, or private upload/storage mechanism on this slice.
  Do not invent requirements beyond these settled gates; if only module
  input is available without public-literal provenance, that input alone
  cannot qualify a host, and the concrete missing piece is the reviewed
  candidate plus the adapter/grant above.

Findings: Standards 0; Spec 0; Security 0 new issues (hard constraints above).
Worst: no finding blocks source publication; the strongest caveat is that
this slice earns zero native credit until the exact policy review, the OS
adapter floor provenance, and the new grant all land. A7/A11/M2/full-F/G/PA6
remain **unverified on real hardware**.
