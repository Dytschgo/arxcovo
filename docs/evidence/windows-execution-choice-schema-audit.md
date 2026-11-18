# Activity schema-version audit: every occurrence, with its classification

Base source: HEAD `f5cee7b6ef9dde1345435be7046346fc06115387` (branch `work/windows-execution-choice`).
Method: `rg -n -i "user_version|schema_version|schema version"` over the whole repository
except `docs/` (it respects .gitignore). This found 33 lines. **Each hit was read with its full
surrounding test or function body**, so the result does not depend on how many lines follow a match.
No `schema_version` identifier or helper exists. No other constant or helper exposes the activity
version: every activity read goes through `pragma_query_value(None, "user_version", ..)` inline, and
every write is a literal `PRAGMA user_version=N`. A second search for indirect schema
expectations (`unsupported local state`, `identity_mode IN`, `activity_v3`,
`table_info(activity)`, `sqlite_schema`) found no further version expectations.

The production gate `store.rs:109-122` accepts versions 0 (fresh), 1 through 4, and refuses
everything else. A successful `Activity::open` / `open_service_fixture` / `open_inner` always
leaves the store at version 4: the v4 rebuild is unconditional when version < 4.

Legend: **STALE→fixed** = demonstrably stale and corrected; **OK-4** = success expectation already 4;
**LEGACY** = deliberately seeded old version (kept); **ROLLBACK** = asserts the old version stays after a
refused/failed open (kept); **MUTATION** = corruption input expected to be refused; **PROD** = production
code (not editable, context only); **CLUSTER** = unrelated cluster databases (untouched).

| # | Location | Occurrence | Classification | Action |
|---|---|---|---|---|
| 1 | crates/arxd/src/service/executor/tests.rs:655-660 | `schema` read after `open_service_fixture` + `start_dedicated`, `assert_eq!(schema, 3)` | **STALE→fixed** (successful open ⇒ 4). Hosted Ubuntu failure 134928130147; reproduced locally | `3` → `4` |
| 2 | crates/arxd/tests/inventory.rs:716 | mutation `"PRAGMA user_version=4"` in `corrupt_identity_fails_closed_without_replacement`, expected `Activity::open(..).is_err()` and file bytes unchanged | **STALE→fixed**. Under base schema 3, 4 was the first *unsupported future* version. On this branch 4 is the current version, so the mutation does nothing: open would succeed and panic with "accepted mutation: PRAGMA user_version=4". The coordinator's search for an expected `3` could not find this. Not yet executed on hosted CI (the inventory binary was unrun in 134928130147) | `4` → `5` (the first unsupported version again; same `_ => bail` refusal before any write) |
| 3 | crates/arxd/tests/inventory.rs:717 | mutation `"PRAGMA user_version=0"` | MUTATION, still valid: version 0 tries `CREATE TABLE local_host_identity`, which already exists, so open fails | kept |
| 4 | crates/arxd/tests/inventory.rs:749 | seeds pre-identity v0 + failing trigger | LEGACY | kept |
| 5 | crates/arxd/tests/inventory.rs:752-754 | expects `0` after a failed open | ROLLBACK | kept |
| 6 | crates/arxd/tests/inventory/activity_v2.rs:227 | seeds v1 | LEGACY | kept |
| 7 | crates/arxd/tests/inventory/activity_v2.rs:236-238 | expects `1` after a corrupt v1 open fails | ROLLBACK | kept |
| 8 | crates/arxd/tests/inventory/activity_v2.rs:254-256 | expects `4` after a successful v1 migration | OK-4 (feature commit) | none |
| 9 | crates/arxd/tests/inventory/activity_v2.rs:706 | seeds v2 | LEGACY | kept |
| 10 | crates/arxd/tests/inventory/activity_v2.rs:711-713 | expects `4` after a successful v2 open | OK-4 (feature commit) | none |
| 11 | crates/arxd/src/development/account_choice_tests.rs:375 | seeds the exact released v3 constraints | LEGACY | kept |
| 12 | crates/arxd/src/development/account_choice_tests.rs:391-395 | expects `4` after a successful v3 open | OK-4 | none (ran: PASS) |
| 13 | crates/arxd/src/development/account_choice_tests.rs:512 | seeds a damaged v3 store (`rights='standard'` with checks ignored) | LEGACY | kept |
| 14 | crates/arxd/src/development/account_choice_tests.rs:550-554 | expects `3` after the damaged-v3 open is refused; snapshot unchanged | ROLLBACK (intentional, already noted by the coordinator) | kept (ran: PASS) |
| 15 | crates/arxd/src/development/account_choice_tests.rs:591 | seeds v1 | LEGACY | kept |
| 16 | crates/arxd/src/development/account_choice_tests.rs:594 | seeds v2 | LEGACY | kept |
| 17 | crates/arxd/src/development/account_choice_tests.rs:623-627 | expects `4` after a v1/v2 upgrade | OK-4 | none (ran: PASS) |
| 18 | crates/arxd/src/store/account_choice_tests.rs:126 | seeds legacy v3 with every column populated | LEGACY (no version assertion in this test; rows, index and constraints asserted) | kept (ran: PASS) |
| 19 | crates/arxd/src/store/account_choice_tests.rs:196 | seeds empty v1 with a deleted-row sequence | LEGACY | kept |
| 20 | crates/arxd/src/store/account_choice_tests.rs:200-204 | expects `4` after upgrade | OK-4 | none (ran: PASS) |
| 21 | crates/arxd/src/store/crash_tests.rs:156-160 | expects `4` after a successful pinned hot-journal reopen | OK-4 (coordinator fix in f5cee7b6) | none; **UNRUN locally** (native crash child, excluded) |
| 22 | crates/arxd/src/service/application/tests.rs:419-423 | expects `4` after successful response-only refusals | OK-4 (coordinator fix in dd9d602b) | none (ran: PASS) |
| 23 | crates/arxd/src/store.rs:109 | production version read | PROD | not edited |
| 24 | crates/arxd/src/store.rs:134 | production `user_version=2` | PROD | not edited |
| 25 | crates/arxd/src/store.rs:137 | production `user_version=3` | PROD | not edited |
| 26 | crates/arxd/src/store.rs:160 | production `user_version=4` | PROD | not edited |
| 27 | crates/arxd/src/cluster/state_store.rs:319 | cluster state version read | CLUSTER | untouched |
| 28 | crates/arxd/src/cluster/state_store.rs:326 | cluster state `user_version=2` | CLUSTER | untouched |
| 29 | crates/arxd/src/cluster/log_store.rs:201 | cluster log version read | CLUSTER | untouched |
| 30 | crates/arxd/src/cluster/log_store.rs:203 | cluster log `user_version=2` | CLUSTER | untouched |
| 31 | crates/arxd/src/cluster/log_store/tests.rs:489 | cluster log test sets 1 | CLUSTER | untouched |
| 32 | crates/arxd/src/cluster/state_store/tests.rs:232 | cluster state test sets 1 | CLUSTER | untouched |
| 33 | crates/arxd/src/cluster/state_store/tests.rs:239 | cluster state test sets 2 | CLUSTER | untouched |

Rows 1-22 map one-to-one onto the 22 activity test hits; each multi-line assertion is cited with
its full span. Rows 23-26 are the 4 production hits and rows 27-33 the 7 cluster hits (33 total).

Indirect schema expectations checked, no version-dependent change needed:
- application/tests.rs:425 `PRAGMA table_info(activity)`: the v4 rebuild recreates `schema()` and
  then appends `identity_mode`, the same column order as v3. PASS locally.
- store/account_choice_tests.rs:137/147 and development/account_choice_tests.rs:526/572 compare
  `sqlite_schema` before and after, or check that `activity_v3` is absent. These are rollback and
  rebuild checks, not version literals. PASS locally.
- inventory.rs:758 `local_host_identity` count after a failed v0 open: rollback. Unchanged.
