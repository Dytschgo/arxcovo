# ADR0038: Terminal incomplete issuance and retained credential storage

Date: 2026-11-20. Coordinator-adopted D2; storage mechanism only.

Before any future issuer entry, create-new a zero-length identity.json, sync
its file and parent, retaining the original object on the original startup owner.
This reservation is deliberately invalid as a receipt. A surviving reservation
prevents fresh acquisition even when Raft still resembles untouched genesis.

IncompleteIssuance is terminal in v1. Unknown effects are Unconfirmed. There is
no retry, salvage, reset, rebind, orphan deletion or replacement key generation.
The original owner retains the first failure, files and OS lock through ordinary
caught unwind. Process death leaves only disk evidence, not an in-memory latch.

The Linux mechanism copies exact authenticated member ciphertext, writes original
ca.der, and writes bounded PublicIdentity v1 to .identity-publish. Exclusive
directory-relative creation, split writes, file sync and parent sync precede
atomic same-directory replacement of only the original empty reservation.
Final parent sync, original-object rechecks, exact three-entry inventory,
retained positional readback and member unseal/leaf validation precede fixed
guard installation. Failed operations preserve partial effects without cleanup.
Other platforms refuse before publication I/O. No Windows/macOS durability
qualification follows from Linux.

Consistency assembly and store completion confer no actual Found/Redeem,
PublishedMember, attach or restart authority. No issuer/job endpoint is added.
Original E, role and anchors require independently trusted expectations; the
member AEAD binds installation/cluster/root/SPKI, not public issuance history.
Missing expectation refuses. The durable trusted-history source remains deferred.

Generic fresh/recovery acquisition is unchanged. Publication rechecks original
root/config/ancestor/lock objects separately from offline zero-journal rules.
Real live-store ownership composition belongs to later items 5/6. A complete tuple
after refused final acknowledgement cannot prove historical acknowledgement.

4b actual gated founder, 4c same-connection join, 4a-W, custody 3b isolation and
trusted restart-history integration remain open. M3 remains unchecked. Engine Sol
3/3 exhausted and historical Opus 0/2 unchanged. No power-loss evidence is claimed.
All native/multi-host paths are **unverified on real hardware**.
