# ADR0025 local preparation evidence

2026-11-16, implementation sub-agent, gpt-6.1-sol high. Base
`d6dc883f68c1605e1a52c83ac4201159dcad4a84`; source HEAD
`874dc9ec79de6d10e5489ba7485832463fc2efd3` (implementation `51ccbb8` plus
completion-metadata correction `874dc9e`). This evidence and administration/progress
updates are a separate documentation commit. Root review then independent Opus high
source/security review remain pending before publication. No independent review is
claimed here. ADR remains Proposed, unaccepted and inactive.

## Commands and results

Node `v24.18.0`, Windows PowerShell host. All entrypoint executions used the synthetic
Node VM harness; its child-process stub runs Node with JSON on stdin, never gh. FS
record writes are intercepted in memory. No live policy entrypoint or live --record ran.

```text
node --test scripts/policy.test.mjs scripts/evaluate-budget.test.mjs scripts/ci-budget.test.mjs scripts/ci-workflow.test.mjs
```

Initial focused result: 39/39 pass, exit0 (5401.7895ms). One complete Node script-suite
run followed test expansion:

```text
node --test scripts/*.test.mjs
```

44/44 pass, exit0 (5180.6018ms). This full run precedes the two final validation
corrections; it is not claimed as a complete-suite run on final source. Correction1
requires a positive safe repository id rather than letting missing ids compare equal.
Its focused rerun passed39/39 (5034.482ms). Correction2 lets a calculable wall
measurement reach run-invalid on a completion-status defect. Uncalculable/negative
wall duration still fails through the existing duration validation. Its final focused
rerun used the exact first command above:40/40 pass, exit0 (5580.1889ms). Two
implementation corrections used; no further corrections attempted. The five unchanged
auto-merge unit tests passed in the full run; no live merge was performed.

```text
git diff --check
```

Exit0. CRLF normalization warnings occurred for files rewritten through Python;
Git diffs contain no unrelated line-ending changes. Python assertions compared
base/current git blobs for workflows, protection, author allowlist and history;
all equal. ADR comparison excludes only its Status line; all remaining lines equal.
PROGRESS before/after its Current state section is equal. No other ADR text changed.

## Contract coverage

1. Geometry: serial/interleaved gaps, concurrent waits counted once, pending intervals
   overlapping execution, prefix/tail/no-pending idle, coincident boundaries and zero
   wall/execution. Sweep uses half-open segments; no longest-queue subtraction.
2. Slow execution:600s idle queue plus1000s execution still fails;900s charged boundary passes.
3. Defects: each run and job validation clause, unsafe/duplicate ids, exact platform
   inventory including a duplicate with matching labels, counts2/4/101 and wrong lengths,
   strict/invalid/missing timestamps, all ordering/range inequalities, head/repo/attempt,
   event/path/status. Defects produce null metrics and wall gate, denying ci-speed.
4. Failed runs/jobs: complete failures compute but withhold credit; failed800s passes
   the old wall rule and failed1475s fails. Cancelled incomplete jobs are invalid.
5. ci-speed: strict charged AND longest execution reductions; failed baseline denied
   even with a faster green valid candidate. No queue-variance speedup claim. Existing
   live-label reread, changed head, same-head/repo selection and actual first attempt
   regressions retained. Synthetic latest listings show attempt2 while /attempts/1
   supplies the measured data; actual returned attempt2 is rejected.
6. Exact actual fixtures reproduce the table below and reject PR162's speed claim.
7. Status marker,<=140 untruncated descriptions,1100 idle dedupe evaluations; real
   execFileSync8MiB bounds exercised on synthetic workflow and jobs JSON. Main/candidate
   workflow overflow and jobs overflow fail closed. Missing jobs transport fails closed.
   VM asserts12 calls for a valid speed evaluation, exact attempt1 paths, one jobs page,
   no page2,<=2 jobs calls, and refused partial inventories.
8. Synthetic record appends one charged-v1 record after the30 old records, preserving
   old record bytes and values; duplicate id causes no write. Actual repository history
   is untouched. Trusted workflow structural pins require one unconditional verify job,
   exact three-platform matrix, no needs/if/environment/services/job concurrency, one
   top-level concurrency and12min timeout. Existing step ordering, smoke checks and
   action pins remain pinned. Apple Silicon macos-latest is unchanged.

## Intentional runtime mutants

Executed this Python harness twice (the second on final policy blob). Every mutant
compiled under node --check and failed with ERR_ASSERTION at runtime, exit1. No
syntax/compiler-only failure is counted. The source bytes were restored in finally.

```python
from pathlib import Path
import subprocess, hashlib
p=Path('scripts/policy.mjs'); original=p.read_bytes(); source=original.decode()
mutants=[
 ('queue-overlap',' && !intervals.some(({ s, e }) => s <= t && t < e)','','geometry'),
 ('credit-failed',"run.conclusion === 'success' && payload.jobs.every(job => job.conclusion === 'success') ?",'true ?','failed run or any'),
 ('platform-inventory','!names.delete(job.name)','false','every metadata defect'),
 ('attempt2','run.run_attempt !== 1','false','every metadata defect'),
 ('ci-speed-fakequeue',' && candidate.longestJobSeconds < latest.longestJobSeconds','','ci-speed rejects queue-only')]
try:
 for name,old,new,pattern in mutants:
  assert old in source
  p.write_text(source.replace(old,new),encoding='utf8')
  check=subprocess.run(['node','--check',str(p)],capture_output=True,text=True)
  assert check.returncode==0,check.stderr
  result=subprocess.run(['node','--test','--test-name-pattern='+pattern,'scripts/policy.test.mjs'],capture_output=True,text=True)
  assert result.returncode!=0 and 'ERR_ASSERTION' in result.stdout,result.stdout+result.stderr
  print(name+': compiled JS; runtime ERR_ASSERTION red (exit '+str(result.returncode)+')')
finally: p.write_bytes(original)
assert p.read_bytes()==original
print('restored SHA256 '+hashlib.sha256(p.read_bytes()).hexdigest())
```

Final restored policy SHA256:
`6e9911bfaf8c4076583168d896c85b999e0c15593b5cfab3ff98688d0e192480`.
The later completion-status correction touches only the entrypoint and its VM test;
the mutated policy blob remains identical.

## Fixture provenance

Read-only public GitHub API,2026-11-16. The following exact gh argument arrays
were executed via Node execFileSync with encoding:utf8,maxBuffer:8388608, once to
inspect allowlisted fields and once to write the sanitized fixture (8 GETs total):

```text
gh api repos/Dytschgo/arxcovo/actions/runs/43880904887/attempts/1
gh api repos/Dytschgo/arxcovo/actions/runs/43880904887/attempts/1/jobs?per_page=100
gh api repos/Dytschgo/arxcovo/actions/runs/43889323450/attempts/1
gh api repos/Dytschgo/arxcovo/actions/runs/43889323450/attempts/1/jobs?per_page=100
```

Only run{id,head_sha,head_repository.id,run_attempt,path,event,head_branch,status,
conclusion,run_started_at,updated_at} and jobs{total_count,jobs[{id,run_id,run_attempt,
head_sha,name,labels,status,conclusion,created_at,started_at,completed_at}]} were retained
or printed. No logs, annotations, steps, private fields, raw payloads, headers or
credentials were stored in source/evidence. The fixture is public metadata, not
independent hosted test execution of this amendment.

| Attempt1 run | Wall | Q | Charged | Longest job | Credit |
| --- | ---: | ---: | ---: | ---: | --- |
| main43880904887 |1475|737|738|372|withheld:not-green|
| PR16243889323450 |1311|434|877|585|applied|

Main remains RED and gates on1475s wall. PR162 has four green gates but budget failure;
it cannot claim ci-speed against failed main. Neither record grants merge authority.

## Pins

Git blobs on source HEAD:

| Path | Blob |
| --- | --- |
| scripts/policy.mjs |4b52a44df0b24cfa518d896d58ed18c1c2487a4d|
| scripts/evaluate-budget.mjs |6d312a8050d9ace7269677ab86565a5f0c7e0f1d|
| scripts/ci-budget.mjs |b315b25d51663ea13ce9f6191d129553012bcea1|
| scripts/fixtures/ci-queue-credit-runs.mjs |70d60463057a9426e82801b4ca3167f9b1235e5c|
| .github/workflows/ci.yml (unchanged) |0c7af38b33f6c127029d8312a0c9ecd92f6361da|
| .github/workflows/policy.yml (unchanged) |4925b7f12bdebcd624024e64af5ce9e000743b67|
| .github/workflows/policy-signal.yml (unchanged) |1898f78e1f4f529ab575af6d5a1db3961631b6ea|
| .github/main-protection.json (unchanged) |575540057aee74d979b86ff6c086bf8ae465dd09|
| .github/agent-authors.json (unchanged) |de7a8c79940b870926acec71eeeb7707e2f09886|
| docs/ci-duration-history.json (unchanged) |23f63dfb74048551b12bf7df900f0bcccaa1d8cd|

Fixture SHA256:`9ef25a64cb17f191ebc03a39adba1e1e2aaba1bb3412a25a7c4d9c907489622e`.
History SHA256:`3b9d9d5494c8aea0ef715348f7e407b1b8cd50016a2c80d8e2776b90ade82e51`.

## Unrun and remaining boundary

No UI/Cargo/native/VM/hardware build or execution, installs, cache deletion, dispatch,
rerun, live recording/policy/status/label/protection/admin/bootstrap, watch, push,
PR, merge or child agent. M2 stays open, M3 unchecked; hardware paths remain
**unverified on real hardware**. Required contexts, App pins, Actions permissions,
source boundaries, architecture and900s threshold are unchanged.

Root review then independent Opus high exact-source/security review must precede
publication. Platform checks for this source are unrun. Activation requires separate
acceptance of the KICKOFF wall-clock deviation and a concrete bounded bootstrap route;
preparation grants neither. ADR0022's historical exception is exhausted. No claim is
made that the current main health, PR162 deadlock or R5 is fixed.
