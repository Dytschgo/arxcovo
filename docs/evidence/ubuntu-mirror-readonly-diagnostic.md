# Ubuntu mirror diagnostic correction 1

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12. Local only,
no subagents. **Fresh independent Anthropic Opus 5.5 high DELTA review is mandatory
before ANY push or hosted execution.** Prior verdict remains changes requested;
this implementation is not reviewer acceptance. No GitHub action was performed.

## Correction pins

- Worktree: `<prepared-worktree>`, runtime-prepared and authoritative; pwd/top-level
  matched, initial status clean, outside canonical `D:/Code/Arxcovo`. Explicit workdir
  used for shell operations; no checkout created or canonical write.
- Branch: `t3/readonly-mirror-diagnostic`.
- Exact base: `22a98f61eb4b85e43fab65da0b3ca6939aba1b6d`.
- Source/test/workflow head: `b1c7a4d9b23302f69adcb59703c9077d977d2d77`.
- Evidence head: subsequent docs commit containing this correction; resolve with
  `git log -1 --format=%H -- docs/evidence/ubuntu-mirror-readonly-diagnostic.md`.
- Review `0689e82` retained at base: [complete independent review](../reviews/ubuntu-mirror-readonly-diagnostic-opus-review.md).
  Prior diagnosis, candidate evidence and all history below were read and retained.
- Actual-main ancestor remains `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`.
  This separate diagnostic is not PR54 or performance attempt 3.

## Review dispositions (implementation, pending independent delta review)

| Finding | Correction / retained limit |
| --- | --- |
| S1 required | Innermost traceback frame only; fixed function IDs and line <=251 only when filename exactly matches loaded spec origin and executed helper bytes match the manifest hash. Unexpected identity/origin/function/line emits 0/0. No traceback formatting, source line, filename or arbitrary local value output. |
| S1 compound sites | Only sources:97 required fields/Signed-By presence and pinned equality; sources:93 continuation/duplicate; replacement:35/39 and mirror-list:166/169 URI boolean comparisons. Only named locals, the exact fixed field regex and fixed comparisons are used. Unknown URI facts are false and refusal remains. |
| S2 | Kernel-host boolean distinguishes an already-rewritten mirror-list entry from a foreign host without naming either. Post-failure snapshot still cannot establish original state or publication success. |
| T1 required | New test step follows observation, both independently use explicit `failure() && runner.os == 'Linux' && steps.ubuntu_mirror.outcome == 'failure'`. Test command is normal runner-user python, no sudo, bypass or continue-on-error. Observation cannot be suppressed by tests. |
| T1 native | Owned Unix entries and regular reads, nonblocking FIFO and directory regular=false, hard-link single-link=false; actual nonroot owner=false is asserted. A test-local policy wrapper substitutes only root ownership to exercise later native read/link facts. Root skips ordinary native tests before filesystem work. These cases are wired but skipped locally on Windows. |
| T2 required | Injected main always 1 including actual complete fixture plan, extra argv 2 before context/tree, internal error, close error, bounded JSON/no stderr, exact category-to-directory mapping. Context, tree, OS platform, timers and output replaced locally; no production context or process alarm. |
| T3 recommended | Alarm cancellation before close with nested finally guaranteeing close attempt; outer cleanup catch handles Deadline and close errors into fixed JSON. Deadline remains 10 seconds. Raised cleanup/cancel alarms tested with fakes, no real signals. |
| T4/T5 info | Existing 2,200,000-byte cap and suffix-format false semantics unchanged. No budget or grammar relaxation. |
| S3/S4 | Owned snapshots/spies and source fences retain their limited evidence; no system-wide mutation/network proof or faithful failing-image encoder is claimed. No new source/image lookup. |

## Version 2 decoding and identity

Version 2 retains the bounded numeric event shape and stage/category/index meaning.
The complete stage and predicate ordering is pinned by a literal unit-test list;
IDs use sorted names. Function IDs: 0 unknown, 1 replacement, 2 trust,
3 rewrite_list, 4 rewrite_sources, 5 rewrite, 6 rewrite_mirror_list. Line 0 unknown;
otherwise decode against the unchanged helper SHA below. The helper is compiled
from the same bounded source bytes hashed before execution (CRLF normalized to LF
only for manifest comparison); no second source-path read or pycache substitution.
Only pure rewrite/rewrite_mirror_list are invoked by observation.

Additional predicates: raise-function, raise-line; has-types, has-suites,
has-components, has-signed-by, signed-by-pinned; continuation, duplicate;
host-allowed, host-ubuntu, host-kernel, scheme-allowed, path-allowed, netloc-host,
query-present, fragment-present. Required field booleans mean nonempty value;
Signed-By presence is separate from equality (an absent field uses the helper's
pinned default). Unknown comparisons remain false. All output remains fixed
numeric/boolean JSON; raw source/URLs/filenames/keyring/env/error values and
private paths never enter reports.

Bounds unchanged: 32 sources, 64 KiB source input/output, 4 KiB mirror input/output,
16 mirror entries, 128 enumeration entries, 2,200,000 bytes including rechecks,
10 seconds, 16 KiB report. Descriptor-relative nofollow opens, nonblocking children,
policy, fresh identity/byte rechecks and failure outcomes unchanged. No writer,
network, staging, chmod, fsync, replace or apply introduced. Reads may update atime.
A complete dry plan remains publication-unresolved and main exits 1; original
helper failure and required gates remain red. No compatibility fix is inferred.

## Personally executed checks (Sol high)

- Baseline: `python -B -m unittest discover -s tools/ci -p 'test*ubuntu_mirror.py'`:
  42 tests, OK, 1 skipped, exit 0.
- Final collector: `python -B -m unittest discover -s tools/ci -p 'test_observe_ubuntu_mirror.py'`:
  30 tests, OK, 2 skipped, exit 0.
- Final all-mirror: `python -B -m unittest discover -s tools/ci -p 'test*ubuntu_mirror.py'`:
  53 tests, OK, 2 skipped (51 passed), exit 0. The unchanged 23 helper tests
  remain owned-temp/pure checks, not production apply execution.
- Initial expanded suite had one source-fence failure on byte-string replace;
  corrected normalization to split/join without weakening the fence, then reran.
- Eleven focused mutation variants executed through compiled in-memory observer
  modules / one temporary workflow edit; no compile/import failures counted.
  Results (assertion failures/runtime test errors): site-line 20/0; origin fence
  1/0; identity fence 1/0; host boolean 5/0; keyring boolean 1/0; redaction 20/5;
  main return-success 3/0; context-before-tree 1/0; directory swap 1/0; cleanup
  catch removal 0/2; workflow order 1/0. The origin/identity test facade was refined
  to report assertion failures rather than missing-local test errors and those two
  variants rerun. All bindings and workflow bytes restored; observer source bytes
  never mutated. These are mutation detections, not hosted mitigation attempts.
- git diff --check exit 0; strict UTF-8/LF checks on changed files; helper/tests
  compared with exact base committed LF bytes and unchanged. Source fences remain
  supplemental, not semantic system-wide proof. No Rust/full UI tests needed/run.

No native Unix descriptor/ownership/FIFO/timer execution is claimed on this Windows
host. Two ordinary-user Unix tests skip here and skip root before fixture work.
Native/installed/dedicated paths remain **unverified on real hardware**. Tests use
only owned temporary fixtures and injected dependencies; no actual collector
context/main/apply, actual /etc, sudo, root runner/service or production operation.
Only the main function under fully injected context/tree/platform/timer/output was
exercised. No private state, ignored fixture, toolchain installation, VM/Lab Manager,
owner/account/service/admin/SCM/LSA, cgroup, experiment or babysitter action.

## Correction manifest (SHA-256, committed LF bytes)

| File | SHA-256 |
| --- | --- |
| `.github/workflows/ci.yml` | `228e48c9f1f8322c55e7dea8880339d0d990187d996dd5ac259622b129a4d09c` |
| `tools/ci/observe_ubuntu_mirror.py` | `45854ef7e0edc416a75686cd9f170f0baf464aa29156ce7439bf16df94bcc198` |
| `tools/ci/test_observe_ubuntu_mirror.py` | `3a8de42f605f08f7c206cf06413818a30f030ac2b433e0e771c4708ee94e229d` |
| `tools/ci/ubuntu_mirror.py` unchanged | `a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff` |
| `tools/ci/test_ubuntu_mirror.py` unchanged | `e1a3fe22a2fb7edae51c3bba69f5c3c71f2acc7552a98f251f1f09649f5b370c` |

Only collector/test/workflow plus PROGRESS and this evidence file change. Review,
helper, helper tests, trust/allowlist/keyring/permissions/parsing/rewrites unchanged.
Next is coordinator-arranged fresh Opus high delta review, before any publication
or hosted execution. No review delegated here. M2 incomplete; ExecutionFailed and
E_PROTECTION unknown with no fix, waiver or retry. Attempt 2 remains nonqualifying;
main's 906s failed baseline remains authoritative; attempt 3 untouched. No hosted
CI result, qualifying performance result, public push/PR/watch/dispatch/rerun/merge,
label/settings operation or manufactured CI success is claimed.

---

## Historical candidate evidence (version 1; superseded by correction above)

# Ubuntu mirror read-only diagnostic candidate

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12.
Prepared locally only. **Fresh independent Anthropic Opus 5.5 high security review
is mandatory before the coordinator pushes or executes any hosted diagnostic.**
No independent review or hosted acceptance is claimed here.

## Pins and scope

- Authoritative prepared worktree/top level: `<prepared-worktree>`;
  pwd matched top level, initial status clean, outside canonical `D:/Code/Arxcovo`.
- Prepared base: `caf8e3abc8e31e7edfe5ff5c398cfbdc11c89ebf`.
  Merge base with actual main: `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`.
  This is separate from performance PR54 candidate `344c406`.
- Actual branch: `t3/ubuntu-mirror-readonly-diagnostic`. A requested rename to
  `diag/ubuntu-mirror-refusal` failed because that branch already exists; it was
  not overwritten, checked out or advanced. The prepared worktree remains authoritative.
- Implementation commit: `3bba8ab275d2c48f3cc88b3631c1648dcee7e754`.
  Final source/test head: `c7524f6bc3ed4d61a00abbe0d1f0b1179a8aa4de`. Evidence head is the subsequent docs commit
  containing this file (resolve with `git log -1 --format=%H -- docs/evidence/ubuntu-mirror-readonly-diagnostic.md`).
- Only new collector/tests, four additive workflow lines and evidence/PROGRESS
  change. Existing helper, parser, trust, hosts, guards, rewriting, matrix,
  timeout, actions, dependencies, source and performance criteria remain unchanged.

Read complete AGENTS, KICKOFF and the [bounded diagnosis](../reviews/ubuntu26-mirror-refusal-diagnosis.md),
including all five next-proposal points and the refusal inventory. Existing
babysitter is unchanged; no agents were started. No GitHub, push, PR, watch,
rerun, dispatch, merge, label, settings, owner/admin/account/service, SCM/LSA,
Lab Manager, VM, container, installation or ignored-fixture operations occurred.
No production helper apply or diagnostic main/context was executed locally.

## Contract and refusal interpretation

The original helper step gains only `id: ubuntu_mirror`. The immediately following
step runs only for `failure() && runner.os == 'Linux' && steps.ubuntu_mirror.outcome == 'failure'`.
It uses the same fixed sudo environment preservation list and `/usr/bin/python3 -I -B`.
No continue-on-error, alternate success path or bypass is added. The original
failure stays red; collector main also returns 1, even for a complete dry plan.
All existing required gates remain. Publishing this workflow can execute it on
CI: review **before push**, not just before a manual rerun.

The collector accepts no CLI arguments or root selection. Linux/root and all
four exact intent comparisons precede filesystem access. These guards express
intent, not hosted provenance; execution is restricted by this work order to a
separately reviewed disposable GitHub Linux runner. Never run it on an owner host.

Descriptor-relative opens validate `/`, `etc`, `apt`, `sources.list.d` individually
with O_DIRECTORY/O_NOFOLLOW/CLOEXEC and fstat root/private-write directory policy.
All selected children use descriptor-relative O_RDONLY/NOFOLLOW/CLOEXEC/NONBLOCK;
fstat requires regular/root-owned/single-link/private-write files. Symlink or
unsafe parent/child refusal stops the scan: no fallback path or reopen workaround.
Names are internal only. Legacy presence is determined in the bounded parent
scan, avoiding exists/is_symlink path traversal. Fresh guarded re-reads compare
bytes, device and inode for every planned change, like the original helper.
Root writers racing the scan are outside this intent fence; retained descriptors
avoid retargeting through changed parent paths.

Only the existing pure `rewrite` and `rewrite_mirror_list` functions are called.
No copied grammar, apply/_apply, staging, application, APT, DNS or network operation
exists in the collector. Shared parser Refusal is reported as `parser-unknown`
within its source or mirror parsing stage: it does **not** identify a specific
trust/URI/token predicate. `deb822` is only a suffix-format boolean, not evidence
that arbitrary content is valid. Unknown errors yield fixed `unknown`, never exception text.

A complete dry plan reports `publication-unresolved`. Publication-only failures
remain untested; no future write is authorized. Reports observe the post-failure
snapshot; the original helper could have partially published before refusing.
This is not a replay of the absent failing runner's original bytes/metadata.

Structural fixture encoding is deliberately **unavailable**. The opaque shared
Refusal and arbitrary comments/fields cannot safely preserve unknown failing
syntax through an independently specified value-redacting encoder in this slice.
No bytes, source lines, URLs, arbitrary filenames, keyring values, environment,
exception text or traceback are emitted. No faithful failing fixture is claimed.
The exact 20261111.82.6 public image revision/provisioning source still needs to
be obtained when available; the matching public lookup was 404 in prior research.
No new public lookup occurred here.

## Bounds and safe output schema

Bounds: 32 source files; 65,536-byte input/output source; 4,096-byte input/output
mirror list; shared parser 1–16 entries. Additional caps: 128 total directory
entries across both enumerated directories (including unrelated entries),
2,200,000 total bytes read including rechecks, 10 seconds and 16,384 report bytes.
One source/mirror over-read byte detects growth; the aggregate budget is checked
before each syscall and read size is restricted to its remaining budget.
The CLI-only SIGALRM timer bounds blocking filesystem reads; cooperative monotonic
checks also guard loops. No timer or process-wide OS/network hook runs in tests.
Descriptor closure and bounded serialization follow timer cancellation.
No explicit filesystem mutation occurs, though filesystem reads may update atime.

Version-1 JSON has only `version` and `events`. Each event is
`[stage_id, category_id, index, predicate_id, boolean_or_bounded_integer]`.
Categories: 0=context/parent, 1=legacy source, 2=source-directory child,
3=fixed mirror list. Parent index follows the fixed parent chain; source index
follows internal sorted names; indices never reveal names. Values are booleans
or integers 0–2,200,000. Report overflow replaces the report with a fixed bounded
`internal/report-bound/false` report. IDs are pinned to this version/source.

Stages: 0=arguments, 1=context, 2=done, 3=enumeration, 4=internal, 5=mirror-parser, 6=parents, 7=parser, 8=plan, 9=read, 10=recheck.

Predicates: 0=actions, 1=ci, 2=deadline, 3=deb822, 4=directory, 5=entries, 6=identity-bytes, 7=linux, 8=no-arguments, 9=nonempty, 10=os-error, 11=owner, 12=parser-unknown, 13=passed, 14=private-write, 15=publication-unresolved, 16=read-size, 17=regular, 18=report-bound, 19=repository, 20=root, 21=runner, 22=single-link, 23=size, 24=sources, 25=total-read, 26=unknown.

OS-error value: 0=unlisted; 1=EACCES, 2=EPERM, 3=ENOENT, 4=ELOOP, 5=ENOTDIR, 6=EISDIR, 7=EIO, 8=EMFILE, 9=ENFILE, 10=EINVAL, 11=ESTALE, 12=EINTR. Actual native errno numbers/messages are
never exposed. This mapping reflects the pinned Python errno names on this
Windows test host; Linux uses the same ordered names (values may differ).

Safe example serialized **fixture-derived rows**, not collected runner evidence:

```json
{"version":1,"events":[[7,2,0,12,false]]}
{"version":1,"events":[[2,3,0,15,true]]}
```

The complete fixture report also includes context/policy/size/recheck rows.
No report has been collected on the failing or a current hosted image.

## Tests and limits

Executed in this Windows worktree:

```text
python -B -m unittest discover -s tools/ci -p 'test*ubuntu_mirror.py'
Ran 42 tests in 0.892s
OK (skipped=1)
git diff --check
exit 0
```

41 passed: the unchanged 23 helper tests plus 18 collector tests; one owned
Unix symlink test skipped pending hosted Linux. Tests use owned temporary trees,
injected root metadata and a local OS facade. No fixture opens actual /etc or
sudo, executes main/context/apply or needs root. Descriptor flags, relative
parent/child opens, cleanup and no-reopen behavior run through the injected
facade on Windows; native Unix permission, O_NOFOLLOW, timer and directory-fd
semantics are **unverified on real hardware** and await separately reviewed CI.

Coverage includes every refusal class in the prior inventory except publication
(which is intentionally inaccessible): each context comparison; unsafe directory,
owner/mode/file/link facts; missing/excess source files; unrelated-entry bound;
size and actual-read overflow; aggregate read/deadline/report caps; both parsers'
encoding/trust/URI/shape refusals; mirror bounds; empty plan; stale bytes/identity;
errno allowlist and unknown errno; output-schema/redaction; 32-source dry plan.
The shared original suite supplies additional pure trust/grammar boundary cases.

Mutation evidence: before/after owned-tree snapshots include file bytes, names,
file/directory mode, link count and mtime; they match. Module-local apply/_apply
spies are never called. The injected OS facade exposes only open/read/fstat/
scandir/close, and checks read-only/no-follow flags and no reads after unsafe
parents. Runtime snapshot/spies establish the exercised seam's no-writer behavior.
AST call-name denial fences and the inspected import/call graph supplement this;
they are weak source checks, **not a semantic proof against arbitrary hidden
mutation or outbound requests**. No process-wide socket/audit/OS hooks were added.
No outbound operation is implemented or invoked in these fixture tests; there is
no claim of sandbox-enforced network isolation or native system-wide syscall tracing.

Earlier local test runs exposed an overbroad redaction assertion matching the
fixed `private-write` label, verbose report overflow at 32 sources, and test
assumptions about output expansion/report capacity. Corrected tests/schema
retain refusal on output growth beyond 64 KiB. These were fixture development
failures, not hosted mitigation attempts or evidence of the original cause.

## Manifest (SHA-256, committed LF bytes at source/test head)

| File | SHA-256 |
| --- | --- |
| `.github/workflows/ci.yml` | `acd0d47613f7767c8f1394eed7e9f9b5377bc7ce8425c9c6f3c9b674cceb7959` |
| `tools/ci/observe_ubuntu_mirror.py` | `ef2a47a8cae99b019a6406ec39b8fbc492d945ff0e1362fa6b29d958297d259a` |
| `tools/ci/test_observe_ubuntu_mirror.py` | `1f0a63dc10dbea4e0e2e4e0d9b9b9726bb480b301cde50317d64bb7218c40721` |
| `tools/ci/ubuntu_mirror.py` | `a945b3d812f1fc47fa43ac696e3ec52026b657867b3f6c0d4b274a5d409b80ff` |
| `tools/ci/test_ubuntu_mirror.py` | `e1a3fe22a2fb7edae51c3bba69f5c3c71f2acc7552a98f251f1f09649f5b370c` |

## Reversal and queue

Revert the source commits to remove collector/tests and four workflow lines;
revert the evidence commit to remove this status record. No runtime rollback,
package/source repair or owner action is needed because none was executed.
No architectural mitigation change/ADR is introduced.

Next: fresh independent Opus 5.5 high security review of the exact candidate,
then coordinator-controlled separate diagnostic publication/execution if accepted.
This diagnostic is not performance attempt 3 and neither consumes nor resets it.
Attempt 2 remains nonqualifying: failed job 131364954122, helper tests 23 pass,
production refused 17:33:17 UTC; image revisions 20261101.186.1/20261111.82.6,
exact cause UNKNOWN, original runtime APT metadata absent. Main's 906s failed
baseline remains authoritative; no speed claim, waiver or criteria change.
M2 incomplete. Native/installed/dedicated paths are **unverified on real hardware**.
ExecutionFailed/E_PROTECTION remain unknown with no fix, waiver or retry.
