# PR186 merged-main first CI

Coordinator verification, 2026-11-19. Exact main
`eea7af57c944d134d50ab284f1bf9b003246073e`, normal first push
[CI44432988620](https://github.com/Dytschgo/arxcovo/actions/runs/44432988620),
attempt1 SUCCESS. No rerun or dispatch.

Existing policy:560seconds wall,556charged,4validated idle; longest Windows Rust
537seconds. All9jobs SUCCESS and Windows workers below660s. Duration-history
record39 appends the real main measurement, preserving the previous38 records'
bytes. Full wall time remains visible; no cause, speedup or stability claim.

| Platform | Rust passed / skipped | Rust job / body | UI job / Node |
| --- | --- | --- | --- |
| Windows | 768 / 36 | 537s / 230.045s | 68s / 60passed |
| Ubuntu | 754 / 22 | 209s / 52.615s | 78s / 60passed |
| macOS | 722 / 8 | 269s / 72.152s | 55s / 60passed |

All67focused source cases passed exactly once on every OS, including each of the
four real learner cases. All UI/browser/Electron phases passed. Final aggregate
gates: Windows8s,Ubuntu15s,macOS7s. Optional Windows sample8PASS,orphans0,41.344s;
this observation grants no stability, repair, re-enable or native qualification.

All9job logs retained on D: with8MiB/45s fetch caps, none truncated; metadata,
SHA256s, source inventory/counts, duration measurement and history insertion proof
are under `D:/Temp/arx-pr186-main-eea7`. One bounded20-minute observer watched only
this exact main run, then fetched its authoritative terminal attempt1 metadata.
Observer stdout30089bytes, stderr0, no truncation. PR186 stays unwatched.

Main health and the independently reviewed revised offline-preparation design
permit its ordinary scoped implementation under standing owner authorization.
Live startup/custody/admission needs a distinct contract and closes no hardware
item. M2 OPEN / M3 UNCHECKED; hardware remains **unverified on real hardware**.
Engine Sol3/3 EXHAUSTED/historyOpus0/2 unchanged. No founder diagnostic/fix,
settings/timeout/quarantine/capacity/SDK change or owner admin action.
