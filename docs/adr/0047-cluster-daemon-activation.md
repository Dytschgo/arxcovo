# ADR 0047: Linux cluster daemon activation and process recovery

## Decision

Linux foreground mode takes a private installation file with
`--cluster-config FILE --cluster-intent INTENT`. The intent is explicit
and separate from the immutable layout. The native service uses the
same file and intent through optional service settings and supervises a
single dedicated nonroot worker. Service activation requires the local
arxd-cluster account, with matching UID and primary GID and without
administrator membership. Its private cluster layout is checked against
the installed service roots and ordinary execution workspace. The worker uses FD 3 for its inherited
private control socket and inherits standard error. Its standard input
and output are null. No other descriptor is inherited. Real and
effective IDs are checked after setting the UID and GID and clearing
supplementary groups. The worker accepts control only from the root
parent identified by kernel peer credentials.

The retained lifecycle holds the owner-local endpoint, pending Join,
admission and issuer work and its retained cluster job. Cluster
readiness is separate from service manager state. Pending Join is
observable before publication. The learner starts admission only after
its peer listener binds. The fixed join-only peer route is qualified
against local interface prefixes and the authenticated Redeem leader
and anchored key. The existing peer request verifies the actual peer.
Recovery does not use this route.

Recovery moves the role custody validated by authenticated history and
the inspected store pair. Recovery retains the exact outer and custody
locks. It passes the existing clock with its persisted committed-time
floor to the restored peer. Founder recovery uses the original CA
signer through checked sealed custody. Confirmed voter history is prior
installation evidence. The existing guarded live proof still controls
issuer activation and use.

Foreground handles SIGINT and SIGTERM. Service shutdown first waits
for the child to stop and join its owner work, drains the ordinary
service resources, then tells the child to close the retained cluster
job. The root parent checks the final worker result and observes
actual child exit. Missing or nonclean outcomes are failures.

Foreground can explicitly adopt TCP descriptors 3 and 4. It checks the
exec-time descriptor inventory, socket type, protocol, family, listen state
and exact configured address before binding owner control. It marks both
descriptors close-on-exec. Fresh Join requires bound, unconnected sockets with
listening disabled, so only the pending owner endpoint is reachable until
publication. The retained transport starts the peer listener after publication
and before self-admission. Enrollment starts its listener only at the existing
issuer readiness gate. Founder and recovery handoffs require listening sockets.
Native service retains its single private control descriptor. Ordinary address
binding remains the default.

C13 needs the exact persisted peer and enrollment addresses through all
three restarts and the founder's explicit D9 route before Join. That uses
fixed addresses in the private installation rather than replacing them
when a port-zero bind completes. The launcher holds the eight initial
reservations simultaneously. Bound, non-listening successors keep the
same binding across restart without accepting traffic from the original
listener. Close evidence checks kernel listen state separately from lock
release and actual reap.

## Evidence boundary

C13 uses the non-test Linux arxd executable from the existing CI
profile, graceful foreground SIGINT and the fixed Linux test-only
publish helper with a one-use inherited pipe. The launcher selects
each pinned test artifact explicitly. Both selectors are ignored by
ordinary nextest inventory. The C13 launcher is one bounded Ubuntu
Rust step after the existing custody qualification.

Installed service and physical multi-host paths remain
**unverified on real hardware**.
