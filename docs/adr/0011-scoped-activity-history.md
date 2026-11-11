# 0011: Durable caller-scoped activity history

Status: implemented candidate, independent review and hosted checks pending.
Extracts the activity contract from the separately prepared service ADR 0009.
It does not add command execution or claim milestone 2 acceptance.

## Store and recovery

Schema version 2 migrates existing identity and activity data in one transaction,
together with restart reconciliation. Existing host identity, activity IDs and
the AUTOINCREMENT high-water mark are preserved. Legacy rows have nullable
timestamps and execution metadata. Failure rolls back the migration.

Metadata uses typed values and SQL constraints. It includes bounded timestamps,
actor kind, a fixed tool vocabulary and optional execution observations. It
never stores command arguments, environment values, paths, output, credentials
or caller-supplied tool names. Unknown calls use `unknown_tool`.

Each operation commits its `started` row before dispatch. Completion updates
exactly one started row transactionally; missing, duplicate or repeated
completion fails. A completion failure withholds the result. Restart changes
unfinished records to `interrupted`.

Cancellation, timeout and shutdown are recorded as `failed` with a separate
typed stop reason. They are not additional storage outcome strings.

Retention keeps 200 read records and 1000 action/lifecycle records separately.
Read floods cannot evict action records. The existing database page/size cap
continues to apply, including after migration.

## MCP visibility and bounds

`list_activity` accepts only `limit` (1–50, default 20), a positive `before_id`,
and `include_reads` (default false). The authenticated principal comes from the
transport and is never an argument. Queries return only that agent's rows;
daemon and other agents' rows are excluded.

The read itself commits record R before querying. Results have IDs below R and
the supplied cursor, use descending keyset order, and fetch at most limit + 1
rows. The extra row determines the next cursor. R is excluded from its own
result, and completion must succeed before any result is returned.

Requests remain limited to 8 KiB. Responses allow 64 KiB through the SDK pump
and stdio bridge. Maximum-field 50-row pages are checked using actual SDK
serialization over HTTP and subprocess stdio. Oversized or incomplete frames
are withheld.

## Verification boundary

The candidate has migration rollback, principal isolation, pagination, retention,
storage-failure and real SDK transport regressions. Local Windows validation
passed 38 workspace tests, formatting and Clippy. Hosted Linux/macOS validation
and independent review remain required. No service installation, privileged
execution or dedicated-account acceptance is established by these tests.
