# M3 three-container acceptance plan

Owner steering, relayed by the existing babysitter on 2026-11-18, authorizes the
kickoff's VM/container route. Prefer three containers on GitHub-hosted Ubuntu;
no local hypervisor installation is needed. Sol 6.1/high inspected actual source;
coordinator Sol 6.1/high reviewed and records this plan. No container, build,
network or native acceptance run was performed for this plan.

## What exists and what is missing

crates/arxd/src/main.rs starts MCP/local/HTTP services, not a cluster node.
config.rs has no cluster startup configuration; its HTTP listener is intentionally
loopback-only. consensus.rs accepts a network factory, but consensus/mem_net.rs
calls another Engine in process. three_node.rs directly initializes its fixture.
Running that fixture inside Docker would not prove real transport or joining.

The arxj2 codec and deterministic expiry/reuse rules exist. Real authenticated
peer/enrollment listeners, sealed joiner receipt/key custody, self-admission,
initial promotion and process lifecycle integration remain prerequisites. Existing
L2 contracts and source slices apply; this plan grants no owner-control authority,
clock-policy shortcut, production acceptance or founder repair permission.

## Build order and observable completion

1. Review the real peer protocol: framed TCP/TLS AppendEntries, Vote and snapshot
   transfer, canonical addresses, bounds before allocation, authenticated sender
   and target binding, connection ownership and bounded shutdown. Enrollment's
   no-client-certificate ingress stays separate, allowing only AnchorQuery/Redeem.
   Positive/refusal tests include truncation, oversize, wrong method/cluster/key
   and sender substitution. Reuse canonical encodings and existing TLS verifier;
   no unbounded deserialization or changed consensus capacities/deadlines.
2. Implement dedicated network adapter/listener modules for the existing OpenRaft
   factory/client interfaces. Prove real socket transfer and denial with loopback
   tests, then three isolated processes and disposable synthetic credentials over
   real sockets. Credit this as networked-engine evidence, not joining acceptance.
3. Complete the real join path under existing reviewed prerequisites: legitimate
   trusted first-node bootstrap; same-connection pre-proof gating, closed query/
   redeem ingress, sealed key/receipt custody, self-admission and guarded initial
   promotion/recovery. Nodes two and three must use join codes and actual issued
   certificates. No test-only assume, seeded state, preinitialized three-voter set
   or mocked success. Public destructive owner control stays blocked on authority.
4. Add narrowly reviewed cluster configuration/startup, preserving the existing
   MCP listener restriction. Require private bounded configuration, separate state
   directories, explicit numeric addresses and credential paths; readiness only
   after stores/listeners are usable. Restart must retain cluster identity. Test
   malformed/reparse paths, duplicate state ownership, incomplete receipts, bind
   failure and shutdown with outstanding work/store-release confirmation.
5. Build the exact binary/image once and launch three nonprivileged containers on
   hosted Ubuntu. Use private disposable state, an internal network, no published
   ports/host networking/Docker socket/owner mounts. A bridge alone is insufficient:
   deny host gateway, metadata and destinations outside the cluster using reviewed
   runner-side fences, and test denies with controlled probe endpoints. Bound CPU,
   memory, disk, logs and every phase. No owner PC admin action is involved.
6. Through authenticated application interfaces, establish three voters through
   the real join path, record readiness/identities/applied membership and uniquely
   identified acknowledged writes. Kill the leader container, continue writes on
   the surviving quorum, restart it, and verify every acknowledged write exactly
   once on all recovered nodes. Save kill/restart timestamps and committed/applied
   positions. No retry-to-green or cleanup masking the original failure.
7. Redeem a code successfully, then reject a distinct request/key reusing that
   consumed code without allocating another member. Distinguish legitimate exact
   same-request idempotent replay. Reject an expired code without a new binding.
   The expiry evidence qualification below must be settled before full acceptance.

Each slice has its own bounded allowlist, meaningful tests, independent Sol review
and normal green checks. Add container CI only after measured runtime fits the
existing under-15-minute budget. No timeout/quarantine/protection/budget bypass,
capacity increase or exhausted-attempt reset is authorized by this plan.

## Expiry evidence and other hard prerequisites

CODE_TTL_MS is 900000 ms. ADR0030 now settles the conditional trusted-host-UTC
model and fallible checked-clock adapter. Runtime recovered-floor acquisition,
caller wiring, TLS time and qualifying restart/suspend/UTC evidence remain open;
that source prerequisite does not prove real-time expiry. Waiting a real fifteen
minutes plus build and acceptance cannot fit the existing CI budget. A narrowly
reviewed controlled-clock test across real transport can prove expiry-policy
rejection, labelled controlled-clock evidence. It cannot establish real-time
fifteen-minute expiry or complete that acceptance gap. Do not shorten TTL, backdate
public requests, fabricate expiration or extend the CI budget. Resolve clock
policy and qualifying evidence as a technical security prerequisite; no owner
interview or approval UI is invented here.

Transport, custody and actual admission are also missing today. Thus this is a
concrete implementation plan, not a runnable container harness or acceptance pass.
Local VMs are a fallback only if containers cannot exercise the path. Before that
fallback, list required owner hypervisor/setup actions; agents perform none on the
PC. Lab Manager, its installed state and agent-windows-test remain untouched.

## Evidence, cleanup and credit

Retain exact source/image digest, bounded per-node logs, acknowledged-write ledger,
membership/readiness records, node-loss/restart events, distinct expired/reuse
outcomes and cleanup report. Do not log join secrets, keys, certificates or owner
data. Cleanup targets only this run's proven disposable containers/network/data,
with ownership/path checks; preserve the first failure and cleanup uncertainty.
No static VM or backup deletion is permitted.

The result can earn three-container acceptance on hosted Ubuntu only after these
paths run. Real multi-host hardware stays **unverified on real hardware**. M2 is
OPEN, M3 UNCHECKED. This PC is authorized for applicable Windows M2 paths only after
the owner completes the standard-account/cap/reserve checklist. Linux A7/A11
sudo/NNP and A12 remain qualifying-Linux-host pending. Sol 3/3 exhausted and
historical Opus 0/2 stay unchanged; no further founder diagnostic/fix follows.
