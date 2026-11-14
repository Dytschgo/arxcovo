# macOS launchd absence observation preparation

Author: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-14.
Status: source/tests preparation only. **Fresh independent Grok exact-head
review required before publication or any hosted execution.** No review,
native tuple, compatibility fixture, or acceptance is claimed here.

## Scope and baseline

The explicit coordinator brief authorizes this bounded preparation and a later
source-reviewed hosted read-only query under ADR 0014. It overrides generic
kickoff delegation and PROGRESS updates for this subtask. No agents were spawned.
No PROGRESS/ADR, production source, owner permissions, runtime API, actual CI,
optional Windows harness, or owner query was changed.

Initial HEAD: `fedee69bf8e2b244f787de15ead410412156bcfe`; initial status empty;
Git top level matched the supplied isolated worktree. The checkout has a
different lineage from supplied merged-main `cbc908ca54fce7a5f2d9c1dd922c928ce5ba0fec`
(the merge-base is `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74`). A direct diff
against that merged-main pin was empty for `crates/arxd/src/service`, `tools/ci`,
`.github/workflows/ci.yml`, and ADR 0014. This is content equivalence for those
paths, not a claim that the supplied main commit is an ancestor of this branch.

Read AGENTS/KICKOFF, engineering skill and safeguards, ADR 0014/addendum,
proposal `657d06a`, accepted Grok design `b353a21`, source review `6029681`,
empty-profile evidence, existing manager collector/tests/README and current CI.
Retained design/source review acceptance applies to the empty production profile;
it does not review or authorize execution of this new diagnostic.

Source commit: `1cc2cd186eda4827147373b381d389e7b2816f83`.
Files: `tools/ci/observe_launchd_absence.py`, its pure test counterpart, and
`tools/ci/README.md`. This document is the separate evidence commit.

## Fixed dataflow and publication contract proposal

There is no command, label, timeout, environment, stdin or path argument.
Additional CLI arguments fail with one fixed error label before metadata/capture.
Non-macOS entry fails before reads/capture. Metadata failure prevents the query.
There is no retry, control query, diagnosis loop or additional native command.

The sole child argv is exactly:

```text
['/bin/launchctl', 'print', 'system/com.arxcovo.arxd']
```

The child uses `shell=False`, null stdin, cwd `/`, unbuffered separate pipes,
and a replacement environment containing exactly PATH `/usr/bin:/bin`, LANG `C`
and LC_ALL `C`. No inherited environment reaches launchctl. No Apple control,
broad domain query, manual command, runtime command, service changes, privilege
escalation, or arbitrary CLI is present.

Before spawn, a monotonic two-second budget starts. Pipes are made nonblocking;
each loop reads at most 1024 bytes once from each non-EOF stream, polls exit,
and pauses at most 5 ms within the remaining budget. A hot stream cannot starve
the other pipe or polling. Stdout cap is 65536, stderr 4096, aggregate 69632;
the read size includes at most one excess byte to distinguish exact cap plus
EOF from truncation. Excess bytes are not retained. The aggregate bound is
also checked, and is mathematically implied by the two independent caps.

Only a real nonnegative exit code plus both EOFs within the deadline can yield
`completed`. A negative subprocess return code yields `signalled` with no
invented 128+signal exit code. Exit before EOF and EOF before exit are distinct;
WouldBlock is not EOF. Spawn/setup/read/wait/deadline/overflow/close failures
use fixed labels. Failure attempts kill and a wait bounded by the remaining
one-second cleanup budget, then closes pipes. Reap timeout, kill and wait
errors have fixed labels. No cleanup outcome establishes absence. Python
cannot preempt OS latency inside spawn/kill/close; this is a polling/wait budget,
not a proven hard three-second wall-clock bound or proof of descendant cleanup.
Real pipe and cleanup behavior remain unverified.

Raw bounded bytes exist only in private in-memory buffers, are excluded from
Capture repr, compared without decoding/normalizing, then discarded. Nothing
writes streams to files, logs or artifacts, or computes a hash of them.
Publication contains only the closed fields below:

- Fixed schema version, fixed target `system/com.arxcovo.arxd`.
- Fixed status/cleanup labels, actual exit code or null, per-stream EOF and
  truncation booleans. These are transport facts, not service-state evidence.
- `query: Unknown`, `fixture_credit: false`, `diagnostics: withheld` always.
- `tuple: synthetic-candidate-1` only on a complete bounded **entire** exact
  code/stdout/stderr match to CANDIDATE in the pinned source; otherwise
  `tuple: unsupported`. No substrings, wildcard suffixes, whitespace changes,
  stream swaps, output excerpts or diagnostic byte publication.
- Public provenance fields described below. No additional fields or byte lengths.

CANDIDATE is an explicitly **synthetic literal proposal**: code 113, empty
stdout, and the source's two-line diagnostic naming only the fixed label and
system domain, with exact LF endings. It is not copied from a native capture.
Fresh independent review must examine the literal and entire publication
contract before execution. The published ID is only a reference to those source
constants; even an exact future match supplies no automatic fixture credit.
No diagnostic bytes are published even when matching. This chooses a narrower
publication boundary than emitting reviewed literals themselves.

For any unknown, loaded, private-looking or extra output, only sanitized
unsupported/Unknown facts survive. Exit 113 alone never matches. The production
accept-set remains empty and every production observation remains Unknown.

## Provenance and fixture limits

The only metadata file read is the fixed public Apple
`/System/Library/CoreServices/SystemVersion.plist`. The final leaf is opened
O_NOFOLLOW and nonblocking, required to be regular and no larger than 16384
bytes, and read with a 16385-byte overflow sentinel. No returned service path
is opened. Stock OS ancestors are trusted here; this is not a new owner-path
validator or an ancestry-proof mechanism.

Only ProductVersion and ProductBuildVersion may survive, with strict numeric
version and Apple-build formats. Other plist keys are discarded. Only
GITHUB_SHA (40 lowercase hex), ImageOS (macos plus two digits, optional arm64)
and ImageVersion (bounded dotted numeric format) are read from the parent
environment. No environment dump, UID/account/home/hostname/private config is
collected. Fixed metadata also records macOS, C locale, both deadlines and caps.
These are **public-runner claims**, explicitly labelled as such, not an
attestation. The coordinator must associate the emitted source pin and image
with the actual public job/checkout; this mechanism does not run git or inspect
process identity to verify them. Unsupported metadata formats stop before query.

A future exact candidate ID plus provenance can identify a full native tuple
by equality to the source literal; it cannot establish absence semantics by
itself. Installed Apple manual/domain semantics, independent provenance review
and the ADR 0014 compatibility review remain gates. No such evidence ran here.
For unsupported tuples this mechanism intentionally loses all diagnostic syntax:
there is no full tuple, derived fixture or fixture credit. Do not reconstruct
one from booleans, exit code, hashes or historical summaries. If sanitation
cannot preserve full trustworthy absence grammar, return no fixture credit and
leave Unknown. Further candidates require a concrete separately reviewed
source proposal; this script offers no runtime selector or private-output route.

## Pure verification and assertion mutants

Ran twice after the final test/code edit, including before source commit:

```text
python -B -m unittest discover -s tools/ci -p test_observe_launchd_absence.py
12 tests: OK (last source-commit run: 0.041 s)
git diff --check: passed
```

All child/pipe/clock/sleep operations are mocks. Metadata tests mock open,
fstat, fdopen/read/close, plist decoding and environment; entry tests mock
platform/argv/provenance/capture. No native platform probing, actual pipe,
ordinary child process, real collector CLI or host metadata read is exercised.
Synthetic strings used in tests are not compatibility fixtures.

Nine compile-valid mutants were executed in memory against the exact source
bytes, each killed by assertion failure with zero test errors:

| Mutant | Named assertion |
| --- | --- |
| Allow first excess byte | first-excess-truncated |
| Swap stdout/stderr capture | stream-identity |
| Replace actual nonnegative exit with zero | real-exit |
| Ignore EOF in tuple eligibility | complete-bounded-tuple |
| Ignore truncation in tuple eligibility | complete-bounded-tuple |
| Accept 113 without the stream tuple | whole-tuple-stream-code |
| Publish decoded stdout as diagnostics | closed-publication-schema |
| Widen query budget to three seconds | absolute-query-bound |
| Widen cleanup budget to two seconds | cleanup-one-second |

Two initial mutation-runner attempts expected the wrong bound assertion name;
the mutant already failed `first-excess-truncated`. The corrected runner
confirmed that named assertion, then all nine mutants. No source correction
was needed. Mutated modules were compiled from in-memory strings only. Finally,
byte equality with the original source and zero CR bytes were asserted; disk
source was never mutated, so exact LF restoration is equality without a rewrite.

## Minimal CI integration proposal (not applied)

After fresh Grok exact-head review and coordinator authorization, change only
the existing `Bounded read-only Unix manager observations` step's condition
from `runner.os != 'Windows'` to `runner.os == 'Linux'`. Keep its Linux commands
unchanged. Immediately after it, add this macOS-only step in the existing
`verify` matrix job (same checkout/public hosted runner, before cargo fmt):

```yaml
- name: Bounded fixed macOS absence observation
  if: runner.os == 'macOS'
  run: |
    python3 -B -m unittest discover -s tools/ci -p 'test_observe_launchd_absence.py'
    python3 -B tools/ci/observe_launchd_absence.py
```

This replaces the old macOS manager collector path, which queries the Apple
control; do not append this while retaining the old collector on macOS. Keep
all application checks, stock-directory observations, job timeout, required
checks, ci-time-budget and agent-review unchanged. No workflow_dispatch/manual
workflow, upload-artifact, raw-output shell piping, sudo or extra query is
proposed. The integration assumes public runner metadata formats pass strict
validation; if not, report the fixed provenance failure and return to source
review rather than diagnosing via new commands or widening collection.

Actual CI is unchanged. No GitHub/network/dispatch/hosted execution/publication
ran. Fresh review may reject this proposal or require tighter source; there is
no inferred authority to enable it. This source preparation does not replace
independent exact-head review or green required checks before coordinator merge.

## Remaining state

M2 **incomplete**; real-Runner PA6 **OPEN**; Linux F/G **OPEN**. main71 cause
**unknown**, no retry authorized. Optional Windows harness remains parked.
All native capture, compatibility, installed lifecycle/boot/restart and dedicated
execution paths are **unverified on real hardware**. No owner/admin action,
service/account/process/VM/private-state inspection, installation, network,
GitHub, dispatch or native command was performed. No owner action is requested.
