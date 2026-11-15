# M2 D7h package readonly - Muse independent architecture/security/source review

Scope: Sol commit `8439aa14e87bf334a82d03f5f06888dfe4015fe6` over base main
`4f1a6db3f652291ffb94550fea85aa395f78109a` (PR #123 main squash; tree
`6fb6ffd467d81a5f1d2cd0a39da6ae92aab5bc3b` equals reviewed `40e24e7`).
Sol tree `c8c46e7ba5b128c4dbf7405a91657be4b6fb1610` differs only by the
reviewed delta below. Reviewed files ONLY (6 paths changed vs base):

- `.github/workflows/sudo-package-readonly.yml` (new, 100644)
- `tools/ci/sudo-package-readonly.py` (new, 100644)
- `tools/ci/test-sudo-package-readonly.py` (new, 100644)
- `docs/adr/0019-unix-native-execution-bridge.md` (D7h section only)
- `docs/evidence/linux-a7-package-readonly-preparation.md` (new)
- `docs/PROGRESS.md` (status lines only)

Read before judging: AGENTS.md, docs/KICKOFF.md, ADR0019 D7a-D7g (all
consumed) and D7h, ADR0020 D9b, the package source evidence
(`docs/evidence/linux-a7-sudo-package-comparison-source.md`), the comparator
source review (`docs/reviews/m2-a7-sudo-package-comparison-muse-review.md`),
the comparator module (`tools/unix-exec-validation/sudo_package_profile.py`),
and all six scoped paths in full. No PROGRESS/ADR/source edit, test run
beyond the mocked suites below, host/root read, sudo/native/helper/account/
admin operation, dispatch, variable operation, push, PR, subagent, or old
Lab Manager access in this review.

## Standards - PASS with one process nit

- One-concern dormant wiring plus evidence note; small diff, UTF-8, LF only,
  no BOM, `git diff --check` clean and `py_compile` clean (all re-verified
  here). New-file SHA256 pins recompute exactly to the evidence rows
  (`fe7e4235...`, `d4e2c648...`, `db6136f3...`); comparator pin still
  `12a122db...` at exactly 11180 bytes.
- Every behavior has a test: 10 mocked wrapper tests plus the 10 existing
  comparator tests (all green here, fully mocked - see checks). Test 10
  pins the workflow/test-before-privilege order and the prohibited surfaces.
- No product approval flow, ceremony, or audit framework added. Fixed
  technical fences only (exact env/SHA/ref/UID/path/size/hash gates,
  fail-closed refusal), proportionate to the steering.
- Smell baseline: nothing to flag. Fixed gate/load/compare helpers are
  single-purpose, not Middle Men; no duplicated logic, data clumps, message
  chains, or speculative generality. The test `result()` builder mirrors the
  closed schema by necessity and never touches production pins.
- Nit (process, not blocking): the Sol delta touches `docs/PROGRESS.md`,
  while D7h reserves that path to the coordinator. Content is status-only
  and accurate, and matches repo practice of builder-side PROGRESS notes,
  but the strict allowlist reads coordinator-only. Minimal fix: coordinator
  owns that hunk at squash (keep or reword, no functional change).

## Spec - PASS against D7h

D7h proposes exactly one fresh-hosted read-only sudo package comparison,
inactive until this review, exact-head review, five gates, coordinator merge,
exact-tree equality, and the once-only pin/dispatch lifecycle. Every
requirement maps:

- Distinct variable: `SUDO_PACKAGE_REVIEWED_SHA` is the sole pin; the string
  `UNIX_EXECUTION_REVIEWED_SHA` appears nowhere in the workflow or wrapper
  (re-verified). The old native workflow gains no authority.
- Workflow gates mirror the settled pattern (`unix-execution-experiment.yml`,
  same pinned checkout `11d5960a...`, `persist-credentials: false`):
  manual `workflow_dispatch` only, repo `Dytschgo/arxcovo`, actor and
  triggering actor `Dytschgo`, ref `refs/heads/main`, and
  `sha == workflow_sha == SUDO_PACKAGE_REVIEWED_SHA`. Manual-only job has
  `contents: read`, 3-minute timeout, `ubuntu-24.04`, no inputs, no
  artifacts, no secrets, no permission expansion.
- Wrapper gates before any import/read (AST-verified order
  `_gate < _load_comparator`, sole `comparator._read_linux()` call site):
  exact env values including attempt `1`, bounded run ID
  `[1-9][0-9]{0,19}`, GitHub-hosted `Linux`/`X64`/`ubuntu24`, real and
  effective root UID, zero argv, fixed workspace/script path, exact
  workflow ref
  `Dytschgo/arxcovo/.github/workflows/sudo-package-readonly.yml@refs/heads/main`,
  and the SHA triple equality. Ten mocked tests precede privilege in the
  job (order asserted in test 10).
- Privilege grant is exactly the coordinator-host tool D7h names:
  `sudo -n /usr/bin/env -i` with fixed `PATH` and the fixed forwarded gate
  fields into isolated system Python (`-I -B`) on the absolute fixed
  repository script. It grants no subject execution as a version/list/true
  oracle and no `sudo -V`, `sudo -l`, policy/account/proc/sys/systemd query,
  or helper/native/fixture surface (all asserted absent).
- Module loading is exactly the reviewed comparator bytes: fixed
  `__file__`-relative sibling, held nofollow directories from `/`, regular
  single-link file, exact 11180-byte size, SHA256 hash before `compile`,
  no argv/path/expected-hash override, no CLI activation, no import search.
  Comparator CLI stays refusing; baseline blob `520d3416...` unchanged.
- Read scope is exactly the D9b file leg: three fixed payloads, two fixed
  links, ancestor metadata, with the existing readlink/ELF/digest/budget/
  reopen fences. No extra host collection is authorized or reachable: the
  only invoked entry is the existing `_read_linux()` once.
- Report is closed and sanitized: full-schema validation (fixed revision
  `1.9.15p5-3ubuntu5.24.04.4`, `amd64`, status/booleans, known digests only
  on match, null digests on refusal, `qualified` always false), one JSON
  print binding source/workflow/run/attempt, constant
  `{"schema":1,"status":"refused"}` with exit 2 on any gate/schema/error,
  exit 0 only on `candidate_match`. No raw bytes, unknown digests, path
  metadata, or exception text escape (mutant/silence tests cover this).
- Lifecycle matches D7h: first eligible squash only, exact-tree allowlist
  (this review file is the listed 7th path), current-main readback before
  pin, one URL-bound dispatch, pin held until that exact run is terminal,
  finally variable deletion with independent HTTP 404 verification, any
  accepted dispatch consumes D7h with no retry/rerun/redispatch/host reuse.
  Future identities are correctly not invented in the evidence.

## Security - PASS with one fail-closed availability finding

- Gate integrity: triple-SHA equality plus workflow-SHA/REF, attempt-1, and
  run-ID bounds close the obvious substitution/redispatch paths available
  to this design. Old-variable-only authority refuses (test 02). Actor,
  repo, trigger, ref, OS/image/arch, UID, argv, and script-path mutants all
  refuse without invoking the loader or reader (tests 01-04).
- Source integrity: bounded nofollow held-dir open, `O_NOFOLLOW` on every
  component (asserted per call), regular/`nlink == 1`/exact-size checks,
  4096-byte chunked reads with an EOF oversize probe, pre/post `fstat`
  drift check, hash-before-compile, all descriptors closed even on close
  failure (test 09, including fault injection for read/drift/close).
- Least privilege: `env -i`, fixed `PATH`, `-I -B`, no secrets/env
  expansion beyond the gate fields, checkout credentials disabled, least
  `contents: read`. The sudo transport executed is the coordinator tool
  D7h explicitly grants, not a subject oracle.
- Privacy: refusal is one constant shape; schema mutants (missing/extra/
  wrong-type fields, `qualified: true`, wrong arch/revision, unknown or
  mismatched digests) all collapse to it; loader/reader exceptions never
  print partial payloads (tests 07-08, re-verified by reading `_main`).
- Residuals are explicit and unchanged from the accepted reviews: privileged
  concurrent writers/mounts and root-protected OS dependency resolution are
  trusted (ADR0020 D8/A1); fences are observations, not host attestation.
  Read-only refers to this comparison leg; checkout and sudo transport keep
  their platform-managed effects. No whole-host audit is implied or needed
  given fixed literals.
- FINDING F1 (fail-closed availability, not a bypass): `_module_bytes`
  compares full `os.stat_result` structs (`os.fstat(fd) == before`), which
  includes `st_atime`/`st_atime_ns`. A read that updates atime (relatime
  promotion on the fresh checkout) would turn a correct host into a refusal
  (exit 2). The comparator itself deliberately excludes atime via its
  9-field snapshot tuple; the wrapper drift check is stricter than that
  settled convention. Impact is bounded to a wasted one-shot grant: any
  accepted dispatch consumes D7h even on refusal, so a spurious refusal
  burns the single attempt with no retry. Minimal fix (one spot, before
  pin/dispatch): compare a stable field tuple excluding atime (e.g. the
  comparator's 9-field snapshot shape) instead of full struct equality.
  Fail-closed either way; no private data or bypass is involved.

## Checks actually run

- `python -I -B tools/ci/test-sudo-package-readonly.py -v`: 10/10 PASS.
- `python -I -B -m unittest discover -s tools/unix-exec-validation -p
  test_sudo_package_profile.py -v`: 10/10 PASS (existing comparator suite,
  fake adapters only).
- AST/call-graph audit (re-ran here): wrapper `_main` calls exactly
  `{print, _gate, _load_comparator, _comparison}` in gate-first order;
  exactly one `comparator._read_linux()` site; no
  `UNIX_EXECUTION_REVIEWED_SHA`; comparator `_cli` calls only
  `{_result, print}` (+ `json.dumps` attribute) with zero read edges and no
  `subprocess`.
- Pins: comparator 11180 bytes / `12a122db...` recomputed; new-file SHA256
  rows recompute to the evidence pins; blob `520d3416...` retained;
  `git diff --check` clean; `py_compile` clean on all three Python files;
  LF/no-BOM verified.
- Workflow: all six required strings present
  (`timeout-minutes: 3`, `runs-on: ubuntu-24.04`, `contents: read`,
  `persist-credentials: false`, `sudo -n /usr/bin/env -i`, pinned checkout);
  all nine forbidden strings absent (`UNIX_EXECUTION_REVIEWED_SHA`,
  `secrets.`, `upload-artifact`, `inputs:`, `pull_request:`, `push:`,
  `schedule:`, `sudo -V`, `driver.py`); mocked tests precede privilege.
- Trees: base `4f1a6db` tree `6fb6ffd...` equals reviewed `40e24e7` tree
  (as D7h requires); Sol tree `c8c46e7...` differs only by the listed delta;
  new workflow/test paths are mode 100644 with no executable blob or mode
  change elsewhere.
- NOT run (by scope): no Linux/root read, no sudo/native/helper/account/
  admin execution, no dispatch, no variable operation, no network fetch, no
  push/PR/subagent. `GITHUB_*`/`ImageOS` semantics were checked against the
  existing workflow pattern (`unix-execution-experiment.yml`), not against
  a live runner.

## Publication eligibility

- Eligible: publication of exactly this delta (workflow, wrapper, wrapper
  tests, D7h ADR text, preparation evidence, this review, coordinator
  PROGRESS note) as dormant wiring through the normal exact-head
  different-provider review, all five required gates, coordinator merge,
  and reviewed-head/squash-tree plus current-main equality. This closes
  nothing by itself.
- Recommended before pin/dispatch: apply the F1 one-spot atime-insensitive
  drift comparison (or accept the documented spurious-refusal risk against
  the one-shot grant). Either way, no re-review of the security posture is
  needed beyond that mechanical change plus the standard exact-head gates.
- NOT eligible and NOT granted here: any actual `_read_linux` invocation
  outside the D7h once-only lifecycle, any host qualification or
  `qualified` claim, any fixture admission, and any A7/A11 native
  acceptance or M2 credit. `candidate_match` will establish only the
  read-only FILE leg. C2, floor, four-mask in-unit view/literals/NSS, and
  actual NNP allow/deny oracles remain open. A7/A11/M2/full F/G/real-Runner
  PA6 remain **unverified on real hardware**.
