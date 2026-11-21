# ADR 0049 · Target-authorized Standard builds

Status: pre-push qualification. Hosted C14 execution pending.

A locally authenticated principal opts in by naming an enrolled node and a
B-local workspace selector. A maps the immutable principal to its configured
agent ID. B checks the authenticated source node, current key and agent ID
against its configured Standard command policy at grant and spend. The
private TLS 1.3 listener checks both enrolled peers and requires committed and
applied membership and current registry binding. It refuses bootstrap and
joint configurations. An explicit target selects only its configured route.

A19 supplies a weak handle from the retained founder or learner transport.
Running selects the handle from its own job and checks current runtime and
membership evidence. The facade uses the existing TLS credentials and leaves
issuance, the voter proof and transport close with their original owners.

A18 adds a default-off Linux local MCP listener on the cluster worker. An
owner-provisioned protected ancestor contains the daemon-owned setgid socket
directory, which allows the dedicated agent group to connect without write
permission on the directory. Both sides check kernel peer identity. The
listener rechecks pinned directories and socket and retains the token
handshake before deriving the immutable principal. The worker's one
Execution and Activity serve untargeted and inbound delegated builds.
The stock foreground and installed service listeners retain their defaults.

B issues independent random operation and lease IDs. Process-owned leases
are bounded to 64 total and eight per source for at most 30 seconds. Each
binds the descriptor, current grant and checked workspace. A persists its
scoped correlation before its single spend attempt. A17 reserves the shared
executor slot and the broker's mirror permit before B creates the dedup
mapping and one started row in Activity. Confirmed commit hands that ID
to the existing worker. Rollback or uncertainty releases both reservations.
The broker binds the descriptor and permit to the authenticated connection
for one post-commit execute. Loss or expiry before execute drops the prepare.

B owns the waiter through terminal cleanup and joins work during shutdown.
The requesting principal can list its correlations and query B without a
repeat spend. Lost replies and unproved terminal evidence produce Unknown.
Activity retains up to 128 mappings in each direction. Pruning chooses only
definite terminal rows and denies admission if the cap is held by pending
or uncertain rows. Leases, command arguments, and output are never stored.

A17 pins the workspace root and slots below the protected A10 hierarchy.
They are actual root-owned directories without group or other write or
extended directory ACLs. The slot and workspace root stay on one mount.
B checks the pinned objects at grant, spend and execute. The build UID
writes to the fixture's fixed output leaf. The bridge passes the slot to
the unchanged Runner and keeps shim cwd empty, so the post-drop no-follow
beneath-open and fchdir select the intended directory.

Trusted root or mount administration could alter the layout after the last
check and before the Runner open. The qualified staging invariant keeps the
layout and mount view fixed through drain. D4 remains unavailable if the
configured layout cannot meet this invariant. Installed-service and physical
paths are **unverified on real hardware**.

C14 reuses the A10 broker profile with exactly one founder, one strict
learner and two admitted B builds. The second build gates its completion
until the original local MCP observer disconnects. Scoped status resolves
that same terminal row.
