# Evidence pin maintenance after A5

2026-11-14. Sol medium coordinator; independent review pending.

PR94 merged as `294542a21320a34786dc08710f8f57c674086a03` after all five required checks and Muse exact-head review. Its test-only A5 additions changed two files pinned by existing non-required evidence jobs. E3 run `43639838229` and S2b run `43639838284` refused their old source pins; no native credit is taken from those failures.

Refresh only E3's `linux.rs` SHA-256 to `af82315a49d97d090086d2ef1cc0fe7069be8894ad597f70b3238f7c0b6e9479` and S2b's `tests/e6.rs` SHA-256 to `05e30be5a57921492bd6289f23cd3618fb56270d89a78947cab18779516c05bb`. Both are calculated from committed LF Git blobs, not Windows checkout bytes. Every other pin, mutation, assertion, workflow, guard, classifier and restoration step stays unchanged.

Checks run: `python -I -B tools/ci/e3-native/test_driver.py` — 25 tests, one skip, exit 0; `python -I -B tools/ci/s2b-native/test_driver.py` — 11 tests, exit 0. The suites use their existing owned temporary fixtures and synthetic Python children; neither native driver was invoked. All eight E3 and both S2b manifest edits apply exactly once in memory to the committed source. This is applicability only, not compiled mutant credit. No native or manual workflow was run. `git diff --check` passed.

This repairs existing automation without weakening pin refusal. Future ordinary hosted evidence results must be reviewed on their exact head; no A5 execution, M2, Stage F/G or real-Runner PA6 acceptance follows. Installed/dedicated paths remain **unverified on real hardware**. D7c is consumed.
