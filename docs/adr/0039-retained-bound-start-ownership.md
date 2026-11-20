# ADR0039: Retained bound-start ownership

Date: 2026-11-20. Source-only step 5a; live acceptance remains step 5b.

Base: e9e44c912e1fa7c01fbd72428077ee46225951ed. Owner authority is
limited to the reviewed inspection-to-start transaction and outer custody.
Design SHA256 a357af504ff0a66f94daeb1871e045e8980b35e950a5eab81c0d34ee035f767f;
independent design review SHA256
00845c8668d139d2ca5fd8baedadc6ab5fe5e63b911b89ef9d5c275f7c6e4b81.

A new operational transaction spans one irreversible Begun period: authenticated
recovery identity and readonly validation, retirement of readonly connections and
leases under the original outer lock, writable acquisition with identity checks,
snapshot/membership comparison and unchanged preflight on the exact pair.
Existing offline preparation reuses factored authentication and inspection bodies.
Legacy terminal Inspect and existing start ordering remain unchanged.

Owned slots preserve partial resources and first failure after caught ordinary
unwind. Transfer validates completeness before infallible field moves; successful
Inspect, partial, failed and spent bundles cannot transfer. The unchanged adapters
receive the original SQLite connections and store leases.

The private bound-start sibling creates a leaked, process-retained resource cell
before preparation. The observer owns a watch receiver only. A driver joins real
blocking preparation and supervised startup. EngineMayExist is sticky before
Engine polling; returned nodes enter the retained cell before notification.
Unconfirmed startup retains strong adapter ownership and the original Owner.
The module-private original-Owner composition loan requires Running, an actual
retained node, no close request and original identity checks; no publication
capability or integration is exposed.

Close is latched before dispatch. Observers cannot abort its execution or dispatch
shutdown again. The actual node stays in an asynchronous mutex in the process
anchor while the single supervised close task polls unchanged shutdown. Extra
startup adapter clones are dropped first. Failure/unwind/lost execution never
becomes clean through an absent node or release flags. First close failure remains
retained. Successful close requires actual-node shutdown success, joined startup
and store release evidence. Deliberate pre-Engine disposal needs no Engine join.
Uncertain custody has no production disposal operation. The small result anchor
remains even after clean close; this deliberate process-lifetime allocation trades
availability/memory for retention and supports repeated observation.

Ordinary tests exercise real production preparation/transfer, SQLite failures,
unwind, real locks, cross-process probes, synchronization and the outer close
policy. The cfg(test) driver boundary returns before Engine unless the job carries the
private step 5b test witness. Releasing the existing test barriers cannot
continue into Engine. Policy tests are
not Engine acceptance. No service activation, Found, issuance, transport,
publication, Engine/shutdown/Owners repair, tuning or workflow change.

Local evidence is in the bounded builder report on D:, linked by the brief.
Default hosted CI remains unchanged and may execute its existing Engine/founder
inventory. First hosted CI costs are pending; no rerun-to-green is authorized.
Engine Sol 3/3 exhausted; historical Opus 0/2 unchanged. New-source budget is
separate. Runtime step 5b, publication composition and hardware acceptance remain
open. Native/multi-host paths are **unverified on real hardware**.

## Step 5b ordinary CI test amendment (2026-11-20)

The owner replaced the one-shot observation with an ordinary cross-platform
library test in the default CI selection. The test
bound_start_real_engine_start_close is the sole caller of the private witness
constructor. The witness applies to its own job. The other tests and production
callers keep their respective paths through the same driver.

The test uses a fresh private fixture and the original outer Owner and SQLite
connections. It runs the shared Engine constructor once on an uninitialized
node with a counted in-memory network. A test-only gate holds the startup
worker at EngineMayExist until the test drops its observer. The test waits for
the job's startup completion signal with no observer, recovers the retained node
and requests close once through the existing shutdown path. It checks the joined
startup and close workers, Engine shutdown and store release proofs. The two
store locks must be free while the outer lock is still held, then the outer lock
must be acquirable after deliberate disposal. The ten-second timeout bounds the
cooperative async wait. A blocking worker can outlive that wait and delay runtime
teardown. The CI job timeout is the outer bound. The production five-second
shutdown deadline remains in force.

The fixture does not run Found, Raft initialization, proposals, signing,
publication or listeners. Existing CI workflow and quarantine guards are
unchanged. Independent review and required checks still apply. Historical
Engine repair counters remain Sol 3/3 and Opus 0/2. Service and multi-host paths
remain **unverified on real hardware**.
