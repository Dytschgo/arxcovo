# PR182 first CI: held after Windows and macOS cancellation

Coordinator, 2026-11-18. Published head
`2e4e0d3f97ed2190ace698e185e29199b21f25c0`,
[CI44312645314](https://github.com/Dytschgo/arxcovo/actions/runs/44312645314),
attempt 1, terminal **cancelled**. 13:20:05–13:37:43Z: 1,058 seconds wall.
No rerun, dispatch, trigger-only push or merge occurred.

| Job | Result / duration | Rust passed / skipped | New named transport cases | Later steps |
| --- | --- | ---: | --- | --- |
| Ubuntu 134830484200 | success / 284 s | 698 / 22 | 32, each once PASS | Node48 and both smokes passed |
| Windows 134830484683 | cancelled / 732 s | 706 / 36 | 32, each once PASS | Node48 and browser smoke passed; Electron smoke interrupted |
| macOS 134830484766 | cancelled / 750 s | 666 / 8 | 32, each once PASS | cancelled during npm ci; Node/UI/smokes unverified |

The coordinator checked each platform's passing test-name set against the 32
actual source test names, with no duplicate rows. Cross-platform component tests
passed; required job cancellation still blocks merge. Agent-review and
ci-time-budget statuses were success, Ubuntu success, Windows/macOS cancelled.
Windows also exceeds the separate 660-second feature gate.

Fresh independent Astra6.2/high confirmed GitHub's Windows annotation: the job
exceeded its 12-minute limit. This establishes cancellation mechanism, not the
underlying slowdown cause. Compared with exact green base cd890750 first
CI44304025778, 109 seconds of the 110-second whole-job increase had accrued before
Electron smoke started. Its download message was only about six seconds before
cancellation; no Electron hang or failed assertion was established. The report's
macOS observation was still pending at its read; the terminal macOS facts above
are subsequent coordinator evidence, not attributed to that review.

**Disposition: hold PR182; no fix authority.** Preserve all stop rules and source.
No engine/founder diagnostic or repair, timer/settings/capacity/quarantine/CI
change, retry, bypass or counter reset follows. Sol3/3 exhausted, historical
Opus0/2 unchanged. Source review remains PASS; it does not satisfy failed gates.

Bounded logs, each below 8 MiB, retained at `D:/Temp/arx-pr182-2e4e0d3f`:

| File | Bytes | SHA256 |
| --- | ---: | --- |
| ubuntu.log | 260174 | 6b350926feb97ec155a0d85fb4a0d2756b57ff095fb3b498bc6974ce9380c6f6 |
| windows.log | 254092 | d6b95932a01c7af7e8a7620f8afd3f6147299b3512671c9eed7d91b8bb010494 |
| macos.log | 221239 | f2050aec547fb4345c067a6796d59f33b64dcfded13da41857fef78695c6e062 |

`run-status.json`, `all-job-log-index.json` and per-platform named-test proofs
retain the terminal metadata and exact-set checks. Full independent disposition:
`D:/Temp/arx-pr182-disposition/astra-review.md`.

Main remains cd890750 with its prior first-attempt green CI; PR182 is unmerged.
M2 OPEN / M3 UNCHECKED. No startup/join/container/native acceptance or stability
credit. Preserve unpublished documentation; do not push solely to manufacture CI.
