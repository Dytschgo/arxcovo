# PR183 resulting-main first CI

Exact main08c4b999c23f9b516b3a942039ebc3c3f1a6dd04 after guarded PR183 merge.
Normal push [CI44351538977](https://github.com/Dytschgo/arxcovo/actions/runs/44351538977),
attempt1. This is a PARTIAL observation, not a main-green acceptance result.
No rerun, manual dispatch or pending result inserted into duration history.

On independent clock-design completion, one read found Ubuntu/macOS SUCCESS,
Windows134968209866 still RUNNING. All three acquired runners. Per-job evidence:

| Job | Seconds | Rust PASS | Skip | Body seconds | Node PASS |
| --- | ---: | ---: | ---: | ---: | ---: |
| Ubuntu134968210365 | 348 | 680 | 22 | 50.986 | 48 |
| macOS134968210476 | 368 | 648 | 8 | 64.785 | 48 |

Both completed every selected Rust case, including all10activity_v2 integration
tests and corrected inventory corruption case, plus UI unit, browser and
Electron smoke. No unrun Rust cases under the existing selection. This does not
resolve local ACL setup, optional dedicated/native/hardware or other M2 gaps.

Bounded logs retained at `D:/Temp/arx-pr183-merge-main`,8MiB/45s per job,
exit0/no truncation; metadata and distinct pass names in partial-execution-proof.json:

- Ubuntu254390bytes SHA256
  `a8a272c8a25abd1affd4a53c008e254e17d73e11ea8a04f1af38606faf74a433`.
- macOS235487bytes SHA256
  `4a51e5d071fe940e2876926c3a9e58f8f07a7cf5909e4a43f156caa57e1ed1dd`.

Coordinator Sol6.1/high retained actual execution output; no local tests/builds/
native observations were repeated. Windows/full matrix and resulting-main health
remain pending. Owner launcher and actual Claude Code acceptance remain UNRUN.
M2 OPEN / M3 UNCHECKED, full v1 incomplete. Prior engine/founder exhausted holds
and PR182 HOLD/NOFIX remain unchanged.
