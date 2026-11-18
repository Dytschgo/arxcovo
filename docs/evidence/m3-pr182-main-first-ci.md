# PR182 merged-main first CI

Coordinator verification, 2026-11-19. Exact main
`e2908c98dfcdf97cb8e0aba431bafdb51f328c95`, first normal push
[CI44418600817](https://github.com/Dytschgo/arxcovo/actions/runs/44418600817),
attempt1 SUCCESS. No rerun or dispatch occurred on this main run.

The existing policy measured 537 seconds wall, 531 charged, 6 validated idle,
longest job523 seconds. The 38th duration-history record preserves prior bytes.
Every job passed; all worker jobs remain below the existing limits.

| Platform | Rust passed / skipped | Rust job / test body | UI job | Node |
| --- | --- | --- | --- | --- |
| Windows | 741 / 36 | 523s / 177.170s | 71s | 60 passed |
| Ubuntu | 727 / 22 | 193s / 51.317s | 103s | 60 passed |
| macOS | 695 / 8 | 317s / 79.388s | 54s | 60 passed |

The exact source inventory of32 transport cases matched one PASS each on every
OS. Checked-clock14 plus anchor refusal1 also passed once per OS. Existing Unix
issuer case passed on Ubuntu/macOS; Windows source quarantine is preserved.
All UI jobs passed npm install/lint/test/build, browser smoke and Electron smoke.
The final aggregate gates passed in Windows6s, Ubuntu3s and macOS4s.

All9 per-job logs are retained on D:, each bounded to8MiB and fetched with a45s
deadline; none was truncated and all fetches exited0. Metadata, lengths, SHA256s,
execution proof, policy measurement and history insertion proof are in
`D:/Temp/arx-pr182-main-e290`. The observation restarted only its read-only
metadata retrieval after the T3 server restart; remote CI was not restarted.
The first local evidence parser had a copied UTF-8 character decoded incorrectly;
correcting that local parser let it read the unchanged60-test logs. No check was
rerun or source changed to obtain this evidence.

Earlier failed PR182 API attempts and the separately authorized zero-start
recovery remain recorded; this result does not relabel them green. No Windows
stability, fix or re-enable credit follows from optional sample outcomes.
Engine Sol3/3 and historical Opus0/2 remain unchanged. M2 OPEN / M3 UNCHECKED;
production joining, three-container acceptance and real multi-host hardware are
still unverified. Hardware paths are **unverified on real hardware**.
