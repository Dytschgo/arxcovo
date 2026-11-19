# Complete peer RPC candidate

Current status: merged as `eea7af57c944d134d50ab284f1bf9b003246073e` after
independent final GO and first hosted CI490wall/484charged. All67focused cases
passed once eachOS. [Merge evidence](m3-pr186-first-ci.md). Own first-main
CI44432988620 qualified560wall/556charged/4idle; [main evidence](m3-pr186-main-first-ci.md).
M2 OPEN / M3 UNCHECKED. Earlier sections below preserve
the candidate's staged evidence and historical holds.

2026-11-19. Sol6.1/high implementation on verified main
`e2908c98dfcdf97cb8e0aba431bafdb51f328c95`, after PR182. Source base
`084b1648b9b87e40d7fc5da0b75b785f0145e866`; candidate
`a46a5203160c6725310d60a580884094d3694d3a`. No PR or hosted CI yet.

The candidate adds closed ARXP/v2 Vote, explicit PreVote and full snapshot,
retains Append and v1 byte compatibility, and supplies the actual OpenRaft
factory/Engine adapter with attach-once ownership and transport-before-node
shutdown. Production cluster startup, custody, joining, admission and container
acceptance remain separate prerequisites. This is private source, not a new
reachable production listener.

The independently reviewed frozen contract is retained at
`D:/Temp/arx-complete-rpc-closure/contract-v2.md`, SHA256
`c921bbae5747b9c7e19292c425fbb6fae24d5ba52b5b2adcacbeeabfcfa69c5d`.
The completed compatibility preflight found no concrete API incompatibility at
merged main. Its report and pin proof remain under
`D:/Temp/arx-m3-resume-preflight`. The coordinator verified the six frozen
manifest files again and all44 source/evidence pins in the builder manifest.

## Inspected implementation evidence

Seven allowed source paths changed in three conventional commits. Cargo,
dependencies, engine implementation/parameters, checked clock, founder paths,
quarantine, capacities and workflow settings are unchanged. Coordinator documents
were preserved separately.

The builder ran52 unique non-Engine tests successfully: v1 codec8, v2 codec3 and
transport41. Across retained runs there were127 test executions,125 passes and
two original failures. The caller-loss regression was corrected while preserving
its assertion; a valid alternative vote term was removed from the malformed
snapshot inputs. Those first failures remain evidence, not discarded runs.
Formatting and locked/offline workspace all-target Clippy passed. The final
Clippy process took5.496s. All four learner Engine cases compiled but are UNRUN.
Early command intervals are explicitly labelled as log-redirection intervals,
not invented process durations.

Raw commands, logs, source diff, selection and SHA256 manifest are retained under
`D:/Temp/arx-m3-full-rpc-build-v1`. The coordinator inspected the actual codec,
factory, ownership/dispatch paths and learner setup/readback, as well as the
reported evidence. Separate exact-source Sol review is pending; inspection here
is not independent review or acceptance.

## Independent review at a46a5203

Sol6.1/high independently reviewed the exact seven-file source diff and all44
source/artifact identities. Standards:0 material findings. Spec:3 findings.
Observation gate and whole-contract/merge: NO-GO. No production regression was
established, and the four histories were assessed consistent; their evidence and
ownership implementation is incomplete. No Engine case was executed.

- S1/P1: retain owners on incomplete joins and preserve the first failure plus
  structured cleanup/release/readback evidence before any assertion.
- S2/P2: check initial effective and committed membership, reclaimed32/4 units,
  complete sender seed on reopen and unchanged enrollment bytes after Append.
- S3/P1: complete the method-specific validation matrix listed below, including
  higher-vote snapshot, independent nondefault-membership mismatches, partial
  bind/attach cleanup and caller-loss/close races.

Report SHA256 `b2b380fb0aaf3e8837e0821d1adc3dcf5bbc80f9817884c605a8cf8d0cad6c4c`,
retained with its pin proof under `D:/Temp/arx-m3-rpc-source-review-v1`.
One Sol6.1/high builder is correcting these fixture and coverage findings under
the same source fences. New independent review is required before observation;
no Engine fix, diagnostic, retry or counter reset is authorized.

## Correction candidate e0e6fae2

Sol6.1/high corrected two allowlisted files at
`e0e6fae2a02db1a3cea864204e961386400e00e4`. The coordinator verified all453
source/input/evidence entries in the returned manifest. Source review remains
pending; builder closure claims are not acceptance.

The correction adds retained supervision and structured first-failure/cleanup
records, exact membership/reclaimed-capacity/enrollment/sender-seed oracles, and
the new-method real-TLS coverage.61unique safe cases passed at the baseline,
then four directly affected cases and the expanded binding matrix were rechecked
after refinements. Do not claim all61 ran on the final hashes. Final formatting
and locked/offline all-target Clippy passed; Engine cases and controls are UNRUN.

The supplied arithmetic proves a conservative valid-image upper bound16361:
the fixed envelope is at most4866bytes, each record at most209bytes, and the
unchanged decoder/application reservation permits at most55records. Thus a valid
16384byte image is impossible; this does not establish that16361 is attainable.
The exception to that one positive boundary test is pending independent review.
Native, wire and header oversize refusal evidence remains required.

Coordinator static inspection also found the evidence writer requires a Windows
D: root, while hosted CI does not provide ARX_OBSERVER_ROOT. The terminal writer
additionally recognizes one task root literally. Portable, truthful failure
evidence needs resolution before publication; no Engine case was run to diagnose
this. The independent re-review is examining the complete corrected fixture.

Correction bundle: `D:/Temp/arx-m3-rpc-review-corrections-v1`; independent round2
will write under `D:/Temp/arx-m3-rpc-source-review-v2`. Source is frozen during
that review. M2 OPEN / M3 UNCHECKED; no new fix authority or acceptance follows.

## Residual setup correction e3afd1f5

Round2 independently closed S2/S3 source gaps and accepted the exact16384-byte
positive-image exception above. It retained one P2 S1 finding: setup errors and
panic could leave only the initial empty evidence object. Observation remained
NO-GO, and no Engine case was executed. Round2 report SHA256:
`d6511de6854f0e9d323015c1f561a7eaf04d0c3b45d1a2988dc9bef452ea0ddc`.

Coordinator Sol6.1/high corrected that one test file in commits663a3d93 and
`e3afd1f5de86038567ade0adaeee9e743d35810a`. Every fallible pre-Engine setup step
is now in a caught scope, with dropped reservation count and registered store
Released observations recorded after its locals drop. Partial-seed reopen and
unregistered resource release remain explicitly unconfirmed. The original first
failure survives artifact errors. The task-specific terminal writer was removed;
explicit roots are validated, and absent configuration uses fallible private-temp
allocation. Local commands explicitly keep that temp on D:. Artifact failure
after otherwise successful setup drops owners and refuses before Engine startup.

New safe tests exercise a real second-bind failure and a controlled setup panic
after a disposable store/socket exists, independently observing rebind/reopen and
release. Portable roots, invalid relative paths, write-failure evidence and owner
drop are tested too. Both new tests and the two existing evidence/supervision
tests PASS. After fallible allocation refinement, the two affected tests were
rechecked PASS; final fmt and all-target locked/offline Clippy PASS. One initial
compile failure for a missing mutable factory binding is preserved; no behavioral
failure occurred. Four Engine histories remain compiled UNRUN.

Bundle: `D:/Temp/arx-m3-rpc-evidence-fix` contains exact source/artifact pins,
commands, timings and logs. Focused independent round3 is pending at this head,
writing only `D:/Temp/arx-m3-rpc-source-review-v3`. No observation/merge claim is
made. [ADR0032](../adr/0032-complete-peer-rpc-factory.md) records the design.

## Source GO and first learner observations

Independent Sol6.1/high round3 cleared S1 with Standards0/Spec0 at exact
`e3afd1f5de86038567ade0adaeee9e743d35810a` and issued bounded four-case observation
GO. Report SHA256 `4c54142b6896c6b01777e7b35ccbfc08a3b6e21d8835c5204398a815c0a6fd26`,
retained under `D:/Temp/arx-m3-rpc-source-review-v3`.

The coordinator verified all7 reviewed source hashes before each command, wrote
an exclusive attempt marker and ran P,V,A,S sequentially, exactly once each.

| Case | Observed effect | Test body | Whole command |
| --- | --- | --- | --- |
| P | Actual Engine PreVote; persisted V2 unchanged | 1.46s | 8.703s |
| V | Actual Engine Vote; V3u persisted | 1.52s | 1.966s |
| A | Exact Append acknowledgement plus separately observed A2 apply | 2.27s | 2.726s |
| S | Exact B_S installation, membership, applied/purged S2 and reopened bytes | 0.87s | 1.410s |

Every case passed with no first/artifact failure, successful startup/body,
both transport closes Ok, reclaimed32/4, unchanged node shutdown Ok, receiver and
sender release flags, three reopened-store release flags and weak-client refusal.
Source assertions checked exact initial memberships, rows/votes/bounds, unchanged
enrollment and sender seed. Setup's live sender release flag is false until normal
teardown; the final release observations are all true. No timeout, fix or retry.

Logs, attempt markers, exact commands, structured evidence, stopwatch durations
and hash manifest: `D:/Temp/arx-m3-rpc-engine-once`. C free before688107864064bytes,
after687927631872bytes; every output/cache/temp path explicitly D:. No separate
pre-execution test-binary hash was captured or retrospectively claimed. Concurrent
volume changes are not attributed without evidence.

These are local Windows component observations through actual TCP/TLS, applied
Registry, a passive receiver Engine and real stores. They do not establish M2
native qualification, three voters, production joining, failover or containers.

## Two semantic controls and restoration

Sol6.1/high ran exactly two baseline/mutant pairs after source GO and the positive
Engine observations. Each baseline passed once; each mutant compiled and failed
once at the intended assertion. No Engine case was rerun.

| Control | Baseline command / body | Mutant command / body | Intended failure |
| --- | --- | --- | --- |
| SDK synthetic PreVote grant | 0.453s / 0.02s | 29.188s / 0.03s | Pending cannot receive a fabricated grant |
| Snapshot metadata/blob/cluster-match bypass | 21.359s / 0.06s | 13.610s / 0.04s | Valid foreign-cluster blob cannot reach recording dispatch |

Both tests await owner close before the failing assertion. The snapshot control
retained its native size cap and external framing/type checks. Raw failures,
single exact mutant diffs, source mapping and restored pins establish the intended
semantic kills; compiler or unrelated failures were not counted. An initial
runner line-ending assumption stopped before the second mutation; only its
unstarted operation continued, without repeating either baseline or mutant.

Exact original bytes were restored after each mutation and all7source pins match.
`git diff -- crates` is empty. Final restored fmt and locked/offline workspace
all-target Clippy passed once (0.890s/5.563s). No mutant code was committed.
The coordinator verified all83 returned artifact entries, the restored source
hashes and raw first failures. Bundle: `D:/Temp/arx-m3-rpc-controls-once`.
Windows quarantine guard9/9 also passed locally in0.192s; its policy is unchanged.

## Gates still open

- PR186 merged from `0a323793986d705785eff7e409cc85a75cce2836`. Source7files match
  reviewed/observed/restored `e3afd1f5`; the added commit records documentation.
  First normal CI44431134508 attempt1 passed; no rerun/dispatch. Its main's own
  firstCI44432988620 qualified560wall/556charged/4idle, all9jobs PASS.
- Independent final review completed GO, Standards0/Spec0, with exact-head
  `agent-review=success`59319264686 and Conversation attestation6996145608.
  Report/proof retained at `D:/Temp/arx-pr186-final-review`.
- Partial first-CI proof: Ubuntu754PASS/22skip,175s job/51.342s tests; macOS722PASS/
  8skip,251s job/72.661s tests. Each executed all67focused source cases once,
  including all4learner cases. All three UI jobs passed Node60/browser/Electron:
  Windows77s,Ubuntu136s,macOS61s. Five logs bounded8MiB/45s each, all untruncated,
  retained with hashes at `D:/Temp/arx-pr186-first-ci`. Windows Rust remained in
  progress at this snapshot; no terminal run/gate/budget acceptance is claimed.
- Final source/evidence review, PR hosted gates/budget and main's first CI health
  are complete; production joining/containers are still
  separate dependencies, not acceptance supplied by this transport slice.

Engine Sol3/3 exhausted and historical Opus0/2 remain unchanged. No retry,
founder diagnostic, runtime fix, setting change or acceptance credit follows from
this candidate. M2 OPEN / M3 UNCHECKED; hardware paths remain **unverified on real
hardware**. Existing owner OAuth wait and babysitter remain untouched.
