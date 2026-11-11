# Owner-store fixture isolation diagnosis handoff

OpenAI / gpt-6.1-sol / medium, 2026-11-11.
Production candidate2604939 integrated onto reviewed engine4b2b769.
No admin, rights, account, task, service or installed-layout operation ran.

Reproducer: cargo nextest run -p arxd --lib --locked -E 'test(owner_windows)'.
Original concurrent run failures: 3/6; serial --test-threads 1: 4/4 passed.
Failure: n4_parent_pin_allows_publication_blocks_parent_rename line256,
rename after drop(s), OS sharing violation32. It passes when run alone.

A two-sibling fixture probe holds another session after dropping the first;
rename first fails32 until the sibling is dropped. The final regression expects
independent fixture parents to release independently and failed before the
fixture-only change. This proves ancestor interference without assuming a leak.

Fixture-only attempts: stop retaining unowned global ancestors, then allow full
sharing during those brief structural observations. The private parent still
has its production-equivalent no-delete pin. Native owner suite: eight repeated
concurrent passes after both changes. Full workspace: 142/144 passed, n4 and
sibling still fail. Earlier full run: 142/143 passed, n4 failed. The second full
failure's exact stderr was truncated; do not invent its specific assertion.

Production native::open_impl and SQLite state fixtures retain common ancestors,
so isolation across the whole native suite remains to be established. Production
owner_windows.rs was not edited. Do not change its security policy to green a
fixture. Investigate the actual global resource/fixture boundary, or test runner
coordination if appropriate; preserve native rename/DELETE negative controls and
post-drop release checks. A module-only pass is insufficient. Nextest executes
tests in separate processes; the existing Rust static Mutex is process-local.
No arbitrary sleep/retry or disabled assertion counts as a resolution.

Remaining: native OS behavior as SYSTEM, LSA calls and installed-service/M2
acceptance are unverified on real hardware. This branch is a committed candidate,
not a passing delivery.
