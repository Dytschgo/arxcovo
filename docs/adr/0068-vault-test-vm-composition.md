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
and invokes the normal reaper. The reaper must confirm process release and
artifact disposal. The adapter requires a confirmed expiry receipt and
zero remaining disk claims before a zero exit can record a pass. The vault
compares the exact version's digest again when committing the result.

Native VM admission currently returns `vm_admission_closed` at vault test
submission. M4 must supply qualified clone, launch, guest channel and reaper
custody to the foreground before native vault tests can run. The existing
TEST VM fixture exercises the composed lifecycle. The real throwaway-VM
test is **unverified on real hardware**.
