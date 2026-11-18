# PR178 Windows founder deadline evidence

Root coordinator,2026-11-18. Published head363d5320684505c15b2e3284b7c6f5a4f9ea842b,
main base e3e79089e6c36f6483cb3db8842e5894f26ee00d. CI44262803622 attempt1.
Windows job134659990323 failed nextest; guard/fmt/Clippy passed.
Required existing anchored_preproof_single_voter failed at anchored_join_tests.rs194
with original founder refusal Unavailable. That source file equals main exactly.
35/652 cases ran:34pass/1fail,36skipped,617unrun. Identity3 did not run on Windows.
Ubuntu and macOS jobs succeeded; root read finished logs and found3 identity PASS
each. This is not all-OS acceptance and does not satisfy feature Windows timing.

Observed trace: primary WaitElapsed, pending proposal, reason Unknown. Preflight
completed213us; initialize100444us; leaderwait entered100449us and returnedOk
at4655216us; proposal entered4655324us; finaldeadline returnedUnavailable
at5004265us. Roughly4.55s spent in leaderwait,345ms left at proposal submission.
Cleanup router/shutdown/store/log allOk. Snapshot is sequential at capture return,
not proof of underlying latency cause, cross-process overlap or no future writes.
No test rerun, timeout/settings/quarantine/source change or fix credit.

Artifacts D:/Temp/arx-pr178-363d5320-hosted, raw log reads each<=8MiB:
windows.log SHA256 fe582435040f10a49cf57f1df47e5a63b60c4d73f677243477e1b283d19be986
windows-job.json SHA256 961c910f39ac98918b12b6c51447479cd7655d9161afb90531c3d69dd5daf091
ubuntu.log SHA256 e7bc25f7de340af921e320b118c7acd647c1f39966f9975740530d7a662c8f0f
macos.log SHA256 3170e4e970ce50c59754f340d7c5884d39cb5927f00254f223d696dbeec02f15

PR178 held. Independent Astra trace disposition pending. Main health is a separate
last-green checkpoint; no evidence here changes main. M2open/M3unchecked; Sol3/3
and historicalOpus0/2 unchanged. Hardware **unverified on real hardware**.
