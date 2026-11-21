# ADR 0043: Exact initial three-voter promotion

The owner adopted D10 on 2026-11-21 for M3 step 11. ADR 0005 keeps the
second host as a learner until two bound learners can promote together.

The retained leader watches bounded membership and replication progress.
The proposal unit and membership intent are shared with learner self-admission.
Before promotion, a fresh read checks the applied membership and the exact
registry keys of the bounded candidates. Effective, committed and applied
membership must agree. Both learners must match a fresh committed target
that covers the applied AddNodes membership. The controller preserves every
node's address and any excluded learner.

The earlier recipe chained the two steps through OpenRaft's
`change_membership_if`. Its internal second call changes the expected
membership ID and removes the full-tail precondition. D10 uses two public
`append_membership` calls with Blank payload and all three preconditions
on each call: the exact membership ID, committed leader ID and full tail ID.
The first entry is the exact joint configuration J with old voters {1} and
target voters {1, a, b}. The second entry F flattens only J's recorded target
and keeps the same full node map.

After possible J effects, interruption leaves the disposition unknown.
Intent release schedules a fresh observation. An established leader can
reconcile only the exact committed and applied J shape. An unchanged or
unavailable view waits for meaningful progress or a new leader epoch.
Revocation after J leaves the original target and metadata intact. The
controller rechecks the applied uniform membership and reports revoked
targets. Issuer transfer remains a separate step. The founder still
participates in J's old quorum.

C11 interrupts the owned intent after J applies on the established leader.
It releases the intent and witnesses the ordinary reconciliation wake and
one flatten. Lost leadership is covered by deterministic fencing and the
existing membership API contract. Service and multi-host paths are
**unverified on real hardware**.
