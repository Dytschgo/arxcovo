# M3 step 5a source delivery

Sol 6.1/high builder, 2026-11-20; exact reviewed base e9e44c91.
See [ADR0039](../adr/0039-retained-bound-start-ownership.md).
The bounded implementation changes only bound_open, startup_recovery, the
consensus sibling declaration, and the new bound_start sibling/test child.

Process custody precedes effects; readonly-to-writable acquisition never resets
the Owner. One consuming pair transfer feeds unchanged adapters. Start and
single-shot close remain independent of observation; failed/unknown close is
sticky and keeps custody. Legacy start and terminal Inspect remain unchanged.

Local named tests stop before Engine. Required normal CI, independent exact-head
review, coordinator merge and separately authorized 5b remain pending.
No real Engine was run locally. Engine counters remain Sol 3/3 and Opus 0/2.
Two failed new-source verification rounds were used (lint; test type inference).
Full commands, counts, first failures, function inventory, immutable diff hash
and commit IDs: D:/Temp/arx-m3-step5a/report.md.
Do not treat policy tests as live close evidence. Hardware remains unverified.
