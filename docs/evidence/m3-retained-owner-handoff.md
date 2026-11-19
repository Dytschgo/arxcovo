# Offline retained owner handoff candidate

Sol6.1/high builder committed `92eade4fbf866709aabb464bb0e8facb2f4afe11`
over docs-only `74e61d2ba8216396faa743372ef1d2fa0723c68c`, based on qualified
main `8bd12b7f4954f4622b4487fad054e4a32e5d551a`. Exactly startup_plan.rs and
consensus/startup_recovery.rs changed: 930 additions, three removals. Independent
source review and required hosted checks remain pending.

Before, Prepared retained offline preparation but offered no validated transfer
of that owner into checked startup inputs. The candidate validates the exact
intent, moves the same owner, and captures only the controlled fresh-store delta.
Fresh creation becomes spent before its first opener; errors preserve the first
refusal and returned owner even after fixture restoration. This is an offline
component, with no operational startup caller or actual SQLite-object binding.
Source rollback is a two-way door; created fixture/store effects are not promised
to roll back. Failed state is never automatically repaired or cleaned.

## Validation inspected by coordinator

All 49 artifact hashes and lengths, both source hashes/Git blobs, and five input
hashes matched. The manifest and raw logs remain under
`D:/Temp/arx-m3-owner-handoff-build-v1`.

| Exact final-source check | Result | Body / command seconds |
| --- | --- | --- |
| startup_plan unit cases | 3 pass | 0.03 / 14.8764512 |
| startup_recovery unit cases | 20 pass | 1.53 / 1.9496942 |
| Workspace all-target locked offline Clippy, warnings denied | pass | command 5.1270671 |
| Workspace formatting check | pass | command 0.7914641 |
| Source diff whitespace | pass | duration not recorded |

The builder report says plan body0.10s; the exact raw plan-exact.log says0.03s.
The table uses the raw log. No individual case durations were reported.
Seven cases are new. The prewritten selection contains 24 platform-dependent
cases; the Unix-only hardlink/reparse case did not run on Windows.

The first compile failure (fixture borrow), four subsequent fixture failures
(Windows raw sharing errors and held zero-length lock reads), and Clippy
failures remain in first-failures.md and distinct raw logs. They were corrected
within this new offline scope. Intermediate WIP source bytes were not individually
pinned; final execution is pinned to the committed source. No historical Engine
attempt counter changed and no Engine test ran.

Real fresh initialization with both zero-length journals survived store drop and
immutable inspection. Other journal masks and mutation branches remain unqualified
locally where Windows denied file removal/replacement. Sharing denial is prevention
evidence only; required Unix mutation coverage must execute on hosted runners.
No separate no-run compilation or static quarantine/seal guard is claimed.

Operational startup, external writer exclusion, opened-object binding, retained
process ownership, CA/key custody, Found publication, attachment and container
acceptance remain unavailable. M2 OPEN / M3 UNCHECKED;
**unverified on real hardware**. No founder diagnostic/fix, retry, timeout,
quarantine, capacity, SDK or policy change is authorized by these results.
