# PR184 resulting-main first CI

Exact main `ff599a2433bc5a34967d965d0f0da2b3a4517cc8`.
[CI44379630740](https://github.com/Dytschgo/arxcovo/actions/runs/44379630740)
attempt 1 completed SUCCESS. Existing policy measurement: 606 seconds wall,
603 charged, 3 validated idle-queue credit, longest job602. No rerun, dispatch,
policy change or diagnostic retry was used.

| Job | Seconds | Rust PASS / skip | Rust body | Node |
| --- | ---: | ---: | ---: | ---: |
| Windows 135068004766 | 602 | 709 / 36 | 206.188 s | 48 PASS |
| Ubuntu 135068004778 | 280 | 695 / 22 | 51.372 s | 48 PASS |
| macOS 135068004502 | 265 | 663 / 8 | 63.529 s | 48 PASS |

All fourteen pure clock tests and the new anchor refusal test passed once eachOS.
Expanded issuer i6 passed Ubuntu/macOS; Windows retains its existing source
quarantine, with no execution credit. Inventory/fmt/Clippy and UI/browser/Electron
steps passed eachOS. The automatic optional Windows sample reported eight PASS,
orphans0, nextest0 and29.593s wall. This observation grants no stability, repair,
quarantine-removal or re-enable credit; exhausted counters remain unchanged.

The exact first-attempt run/jobs, main run listing, policy output and case-level
execution proof are retained at `D:/Temp/arx-pr184-merge-main/`. History entry36
was appended using the existing policy output, preserving prior record bytes
around the insertion; `history-append-proof.json` records before/after hashes.

All three job downloads had 8 MiB / 45-second bounds, exit0 and no truncation:

- Windows:255,946 bytes, SHA256
  `5b45f91ec5fa1d03a28aacb7ccb6485e40883b1604a23cbab943a83aadcf9c82`.
- Ubuntu:259,701 bytes, SHA256
  `7cbf711cc1e9a4c4b154e53c9348908cb6f9b146639d44a1804fd8802ffaf03d`.
- macOS:238,399 bytes, SHA256
  `340bac5dd825246232c1e90b1faf5d7cac2b22e0f34f10992c60560077970181`.

The earlier failed1cd CI and corrected a3 PR CI627s are separately preserved.
The source remains an unwired prerequisite, not healthy UTC, real expiry,
transport/container or hardware acceptance. PR182 HOLD/NOFIX, engine Sol3/3
exhausted and historical Opus0/2 unchanged. M2 OPEN / M3 UNCHECKED;
unexercised paths remain **unverified on real hardware**.
