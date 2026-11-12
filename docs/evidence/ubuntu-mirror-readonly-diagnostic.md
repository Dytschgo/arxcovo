# Ubuntu mirror read-only diagnostic candidate

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12.
Prepared locally only. **Fresh independent Anthropic Opus 5.5 high security review
is mandatory before the coordinator pushes or executes any hosted diagnostic.**
No independent review or hosted acceptance is claimed here.

## Pins and scope

- Authoritative prepared worktree/top level: `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-8366903c-dcbb09d972636f18`;
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
