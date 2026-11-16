# ADR 0024: Windows three-node test quarantine

Status: accepted under delegated steering, 2026-11-16; hosted gates pending.

Three suite-level repair attempts left hosted Windows cluster tests intermittent. The independently reviewed disposition recommends quarantining exactly T1–T4, U6 and three R6 tests on Windows. Their bodies still compile and pass Clippy; each skip names the disposition. Ubuntu/macOS continue executing all eight. Other consensus, enrollment and storage tests remain required everywhere. No assertion, deadline, retry, scheduling or production code changes.

Delegated babysitter clarification supersedes the pending owner question: a reviewed quarantine restoring green main may merge after all five PR gates pass even while its base main is red. This applies to this repair, not feature merges. The next exact-main CI must be green before parallel M3 resumes. No manual rerun or fourth Sol fix is authorized.

Windows three-node coverage is unverified on hosted Windows and **unverified on real hardware**. Earlier green Windows runs do not prove stability. M3 stays unchecked; M2 remains open, A7 FINAL NO-GO and A12 consumed/no-retry unchanged. Opus owns a separate diagnosis/re-enable follow-up, requiring independent review and fresh hosted evidence before removing all eight gates. Revisit before M3 acceptance or Windows cluster use in M4.
