# Offline startup identity preparation: candidate evidence

2026-11-19. Base `afcf2e9d80f56d57c94135456101a010ecf655b1` carries verified
main `eea7af57` and preserved documentation only. Six allowed Rust paths changed.
Builder OpenAI GPT-6.1 Sol/high commits `a9291113` and `2eb3ba10`; coordinator
Sol6.1/high corrections `90fb1521`; coverage Sol6.1/high `937065c1`.
Independent source GO at `e77c1e4e` is recorded below; final delta/exact-head review,
required hosted checks and merge remain pending.

## Result boundary

Private offline preparation retains the outer owner and checked identities,
validates public founder/joiner records against a separate trusted expectation,
and observes actual persisted state and clock floor. It starts no Engine or
listener and releases no key. Opaque artifact presence does not prove custody.
Config/main, store cores/formats, Engine, founder, PKI, dependencies, capacities,
deadlines, SDK, quarantine and CI are unchanged.

The accepted design is recorded in [ADR0033](../adr/0033-startup-identity-preparation.md)
and the [bounded brief](../briefs/m3-startup-identity-preparation.md).
Frozen original contract SHA256:
`fe9bcaaa0d9fbe2aeb89edebc0e0cea6eb0f3b272d580a857b095934fefe930f`.
Independent Sol6.1/high design review GO is retained at
`D:/Temp/arx-m3-startup-contract-review-v2`.

## Review corrections and validation

Independent Sol6.1/high source review at `2eb3ba10` was NO-GO: Standards0/Spec3.
Report and58 checked pins are retained at `D:/Temp/arx-m3-startup-source-review-v1`.

| Finding | Candidate correction and evidence |
| --- | --- |
| S1: untrusted schema reached integrity traversal | Exact schema rejection precedes quick_check; defensive/untrusted SQLite configuration. Harmless innocuous CHECK probe confirms the fixture can evaluate. Zero-traversal assertion failed on original order, then passed. Earlier weak probes passed and are nonqualifying. |
| S2: applied index omitted the i64 bound | Read-only preflight refuses before mutable open. Canonical rechecksummed oversized snapshot failed the before-opener assertion on original source, then passed. |
| S3: membership identity discarded | Owned recovered observation retains exact optional position/blob and compares real StateStore readback. Founder None and joiner exact-membership assertions pass. |

Coordinator correction evidence is retained at `D:/Temp/arx-m3-startup-review-fix`.
All9 recovery tests passed after corrections, body1.52s; fmt/diff check and
all-target locked/offline Clippy passed. No Engine/founder case was executed.

Coverage commit `937065c149a3b7dd95aa4305accc4caff130bc60` adds:

- Real current-store vote, tail, partial/full purge and applied/preflight comparisons.
- Native512-row success/513th append Full; independently encoded raw513th row
  reaches the overflow sentinel. Both copied inspector and current opener refuse
  the corrupted store without changing names or bytes.
- Eight independent mutations: row magic/tag/trailing bytes, metadata trailing
  bytes, nonboolean vote flag, negative index, undeclared row and extra metadata.
  Grammar mutations are rechecksummed. Raw validator, preparation and current
  opener each refuse; post-mutation snapshots remain unchanged.
- Identical-byte replacement attempts before inspection, mutable open and final
  publication. On Windows all three were OS-denied with code32 and preparation
  remained valid. This qualifies OS prevention only. Actual identity-mismatch
  refusal remains unqualified locally; Unix execution is UNRUN.

At this final source,13 exact-name recovery tests passed once each,0 failed/
ignored. All-target no-run compilation passed3.510s; Clippy passed5.131s; final
fmt check passed0.785s. Initial four mistaken binary-target selections ran ZERO
tests and are retained, excluded from pass counts. Correct library selections
passed first execution. No same-issue failed-fix counter changed.

Raw results, exact commands, source diff and32-entry SHA256 manifest:
`D:/Temp/arx-m3-startup-coverage-v1`. Coordinator verified every entry and source
SHA256 `a999302135c225268f720f535319a713b71a66b4d6bcbaf1273efd2f23a04218`.
Earlier builder evidence includes plan3, identity2 and accessor1 passing tests;
those files were unchanged by later recovery corrections. These are historical
applicable results, not claimed executions at the final commit.

## Remaining gates and limits

Fresh independent Sol6.1/high source review at `e77c1e4e` closed S1–S3 and the
significant validator gaps: SOURCE GO, Standards0/Spec1 nonblocking P3. Report and
pins: `D:/Temp/arx-m3-startup-source-review-v2`. The P3 identified the brief's
additional literal codec fixture prescription. Coordinator added a single pure
test with handwritten founder/joiner v1 JSON bytes, independent typed values and
exact serialization equality. Its opaque leaf bytes qualify codec shape only;
separate real-certificate tests provide crypto outcomes. Exact-name test PASS
first execution15.617s including compilation; body<0.01s. fmt and all-target
locked/offline Clippy PASS5.442s. Raw evidence: `D:/Temp/arx-m3-startup-final`.
Only test code changed; focused independent delta/exact-head review is pending.

Before publication, live main remained `eea7af57`, latest push CI44432988620
attempt1 completed SUCCESS, freshly read alongside branch metadata. No rerun or
dispatch. Normal first-attempt Linux/macOS/Windows CI and all required gates
remain mandatory. Unix file replacement, platform alias/reparse paths and other
unexecuted hardware paths receive no local credit. No local Engine/founder,
native executor, production/client, network or container observation occurred.

All scratch/cache/target paths were explicitly D:. Coverage checks measured over
687GB C: free; no owner cache/data/admin, LabManager/state/VM, OAuth or babysitter
action. Engine Sol3/3 exhausted and historical Opus0/2 remain unchanged.
M2 OPEN / M3 UNCHECKED; **unverified on real hardware**.
