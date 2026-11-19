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
