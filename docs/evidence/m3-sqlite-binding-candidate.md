# First SQLite binding source candidate

Sol6.1/high builder committed source-only
`9c3b6828ece7c9bd6baf04cae893005e6dfad139` from docs-only
`5f8ce2bcefa98575030c9eca903a3da702c8a6f2`, source base8bd12b7f.
Exactly35 allowed paths changed. All28 upstream archive members remain intact
except the four specified patched members; licenses and provenance retained.
Coordinator verified35 source, four input and140 artifact SHA256/size pins.

Before, the pinned public Unix interface supplied no actual-open object identity.
The candidate adds a private fixed ABI and native Unix descriptor measurement,
Windows handle qualification, a pinned local dependency patch and bounded verifier.
Rust proof helpers are cfg(test) only. No store construction or production caller
is introduced; independent review must assess this boundary before activation.
Source rollback is reversible; no persistent store migration is involved.

## Builder evidence inspected

| Final check | Result | Wall seconds |
| --- | --- | --- |
| Targeted Windows binding tests | 10 passed | 15.2176404 |
| All-target locked offline check | passed | 5.976061 |
| All-target locked offline Clippy | passed | 5.0268408 |
| Formatting | passed | 0.7774093 |
| Extracted exact build guard | 2 positive /71 expected refusals | cases1.235 |
| Inventory verifier | 30 pure negatives passed | 0.1489233 |

The guard's isolated compilation took0.203s. Two unresolved-provider link
controls failed as intended in0.14/0.11s; neither executes a binary or establishes
behavior against an actual unpatched SQLite library. Pure malformed ABI cases
and inventory controls are distinct from real native identity evidence.

Real NTFS fixtures cover stable identity, a different real SQLite file versus the
original guard, read-only identity, hardlink/directory/ACL negatives, unsupported
memory/longpath VFS and rename-denial prevention. A separate-file mismatch proves
comparison refusal independently of denied rename. The source/byte audits and
initial compile, sharing-error oracle and static-library alias verifier failures
remain retained at `D:/Temp/arx-bound-binding-build-v1`. Each distinct issue had
one correction; historical Engine counters remain unchanged.

The full staged whitespace check exited2 on preserved upstream archive bytes and
literal diff context. This is disclosed rather than claiming a clean full check.
Source-only diff adds about582k lines because the accepted scope retains the
complete upstream crate. Review can compare its archive and isolate the small
maintained patch instead of treating that volume as authored code.

Unix/cross-target execution and normal CI are UNRUN. Reparse/ancestor substitution,
same-name spoofed VFS, actual unpatched-library and runtime source/ABI mismatch
negatives remain UNRUN. No broader filesystem support, real hardware, malicious
toolchain defense or continuous immutability is claimed. Effective config/build
verification has the trusted-toolchain limits recorded in the builder report.

Required next gates: independent exact-source review, appropriate verifier CI
wiring, first normal hosted execution and all required checks. Store-only and
Engine integration remain later slices. PR189 stays HOLD/unwatched; no founder
diagnostic/fix/retry authority. M2 OPEN / M3 UNCHECKED;
**unverified on real hardware**; operational NO-GO.

Source review at70036105 found one P2: architecture macros alone admitted Unix
x32/ILP32. Root correction016ffaf8 adds sizeof(void*)==8 to the actual native
qualification predicate, refreshes patch provenance and adds three width-policy
source controls to the verifier. Six compile-only checks use that exact extracted
function, real32/64-bit MSVC pointer code generation and controlled Unix OS/arch
macros. Optimized refusal constants cover32-bit pointer cases and unsupported
OS/arch; supported64-bit cases retain success. No native Unix or x32 library/
filesystem execution is claimed. The initial assembly parser expected spaces
instead of tabs; that failure is retained and its successful first compilation
was reused. No source mutant or executable ran.

Correction checks: targeted Windows10PASS (8.537s command/body0.01s), all-target
check4.371s, Clippy5.051s, fmt0.835s, verifier33 source/inventory negatives PASS.
Fresh metadata and build messages are retained at `D:/Temp/arx-binding-width-fix`.
Independent exact-source re-review remains required; CI wiring/native gaps stay open.

Independent source re-review at69e170ca completed GO, Standards0/Spec0/SP1 closed;
report SHA256 `c2880c1552e5a6a438d4ad4377e66ef60460509529c9cbde86971482721c66a3`.
Qualification and next-store gates remain pending.

Coordinator CI integration adds one required PowerShell step before Clippy and
normal nextest in each existing Rust worker. It collects locked metadata and JSON
all-target check output from the same head/build, runs the verifier self-test,
records SHA256/compiler identity and propagates explicit Cargo/temp paths through
GITHUB_ENV. It preserves all existing checks, matrices, deadlines and aggregates.
The existing nextest step supplies native execution once; the new step adds no
second targeted execution or Engine retry. Hosted qualification is still UNRUN.

The exact extracted workflow step passed locally with offline Cargo and D-only
paths (exit0,1.328s). Three injected command-failure controls propagated exit37
(metadata),38(check),39(verifier). These mock process exits are workflow failure
controls, not native binding evidence. The first test wrapper failed to propagate
the called script's LASTEXITCODE to its own process; initial files/log remain,
and corrected wrapper propagation matches the runner's exit handling.
Evidence: `D:/Temp/arx-binding-ci-wiring`. Independent workflow review is pending.
