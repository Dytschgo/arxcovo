# 0068 — Script vault in the TEST VM foreground

The TEST VM foreground mounts the vault tools on its existing authenticated
local MCP listener. Vault authorization uses the caller's principal. TEST
clone, start, and guest execution use the reserved `arxd` principal, which
foreground clients cannot authenticate as. Callers cannot inspect or mutate
the tester's VM. The inventory foreground still opens its vault without a
tester. HTTP and the installed service retain their existing authority.

The adapter selects the pinned, sealed TEST template for the requested OS
and uses the VM foreground's operation admission and worker receipts. Each
job gets a ten-minute TEST lease and separate VM request keys. On every
post-clone path, the adapter brings forward expiry for the daemon-owned VM
and runs a custody turn for that exact clone under the periodic reaper's
lock. The turn uses the same release, disposal and accounting checks.
A waiting vault teardown gives the active reaper item time to finish, with
a separate six-second acquisition bound. A yield reserves the next ordinary
opportunity so successive vault jobs cannot starve periodic cleanup. The
15-second custody deadline starts after acquiring the lock. The adapter can
retry a backed-off claim within that deadline. The ordinary tick and the
vault waiter choose their turns under one scheduler mutex. A second tick
that arrives during custody waits for the owed turn. One eligible ordinary
item (preferring another VM), or a completed check with no eligible item,
discharges the reservation. An interrupted batch leaves admission fenced.
Backoff belongs to the claim across bounded batch selections. Both ordinary
and targeted failures respect its next eligible time. A targeted worker
releases the lane while waiting. An attempt that exhausts its custody
budget without proof rejects the version.

Contention timeout, backoff beyond the custody deadline, shutdown before
custody, and the absence of a guest verdict produce a retryable
`test_inconclusive` result. It preserves the
active version and frees the test slot. A fresh request can test the same
version again. The vault v2 schema stores the retryable marker separately
from permanent rejection and preserves prior v1 outcomes and request receipts.
A confirmed expiry receipt and zero remaining disk claims are required before
a zero exit can record a pass. The vault compares the exact version's digest
again when committing the result. A mismatch permanently rejects that operation's version.

Native VM admission currently returns `vm_admission_closed` at vault test
submission. M4 must supply qualified clone, launch, guest channel and reaper
custody to the foreground before native vault tests can run. The existing
TEST VM fixture exercises the composed lifecycle. The real throwaway-VM
test is **unverified on real hardware**.
