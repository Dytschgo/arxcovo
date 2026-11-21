# ADR 0045: Installation-authenticated restart history

M3 step 13a adds a private Linux history lane for process-exit recovery.
It uses the original installation configuration and outer Owner. Fresh
or recovery intent selects the existing Plan mode in memory. The
configuration object and legacy Plan encoding retain their identities.
The lane checks a configured, daemon-private history directory whose
components are opened without following links. It checks state,
credentials, workspace, configuration, wrap, role custody and the
reserved voter path for overlap and object aliases. A fresh history
directory and reserved voter path must be empty and absent respectively
before custody or issuance work. Existing callers retain their explicit
legacy entry points.

## Authenticator and records

Use HKDF-SHA-256 inside checked custody. The salt is the exact ASCII
string 'arxcovo/restart-history/subkey/v1'. Input key material is the
original 32-byte seal key. The HKDF expansion info is the concatenation
of the independent 16-byte installation ID, one role byte (founder 1,
learner 0) and one record kind byte. The 32-byte subkey authenticates the
entire frame header and payload with HMAC-SHA-256. Ring performs
constant-time verification. The seal-key read is bounded and wiped on
all outcomes. Expanded keys stay inside Ring and custody. Neither the
key nor member or CA private material enters the history or its errors.

The frame contains the six ASCII bytes 'ARXHST', a version byte (1),
one kind byte (issuance 1, Strict 2, voter 3), a big-endian 32-bit
payload length, that many payload bytes and the 32-byte tag. Maximum
total sizes are 8192 bytes for issuance, 2048 for Strict and 4096 for
voter. The payload is the exact compact serde JSON encoding of the
typed Rust envelope. Parse only after MAC verification, reject unknown
fields and require byte-equal canonical reserialization. The envelope
contains the final record's Linux device and inode as two unsigned
integers and the record-specific payload. Digests and IDs use arrays of
unsigned bytes. Positions use term, leader and index in that order.
Endpoints use canonical numeric socket address strings.

- Issuance pins installation, role, cluster, node, member SPKI, root
  and leaf digests, full original issuance position E, and the verified
  canonical Redeem anchors and TLS peer when present. It pins the
  configured peer and enrollment routes and bounded configuration hash.
  Route expectations confer no listener or peer authority. Owner
  identities, in order, are config, state, credentials, workspace,
  outer lock, published identity, CA and member object. Founder then
  includes the state and log locks and state and log database identities.
  Custody identities are wrap and role directories, install context, seal
  key, custody lock and member key, followed by the CA object for a
  founder. The record also pins the history directory. The file objects
  are identified by device and inode, and mutable SQLite bytes carry no
  immutable content digest.
- Strict pins the authenticated issuance frame digest, the full healthy
  Strict applied position and the original state/log lock and database
  identities in the same order. A fresh learner creates those stores
  after issuance. The write uses a private witness of the existing
  positive guarded observation after the applied and phase guards
  release.
- Voter pins the authenticated issuance and Strict frame digests, the
  original cluster and CA, both node/SPKI pairs, both verified routes,
  full leader epoch and promotion position, the voter directory,
  reservation and envelope identities, and the exact ciphertext digest.
  It records historical acceptance by the local guarded proof.

Only the retained founder success and verified same-connection Redeem
continuation can attest issuance after durable publication and original
member possession succeed. Recovery verifies the record with the
independently configured wrapping installation and role. The reader
derives its expected identity from authenticated history and compares
retained artifacts. Reading a receipt or validating its public fields
supplies no issuance authority.

## Commit and process recovery

Each kind has a fixed temporary name ('.issuance.tmp', '.strict.tmp',
'.voter.tmp') and final name ('issuance', 'strict', 'voter') at the
checked directory. The writer checks the prior exact inventory, creates
a new temporary file without following links, writes and syncs it,
checks the original object, and uses renameat2 with RENAME_NOREPLACE.
It syncs the directory and authenticates exact-handle readback. Live
failure is terminal for that attempt. Under the process-exit and
no-power-loss premise, the sole intact final name can qualify even if
the previous process never observed the final directory sync or
readback. Nothing salvages, replaces or removes partial effects.
Temporary, conflicting, missing required and substituted objects
refuse. This boundary gives no portability, power-loss, same-account
rollback or clean-close guarantee.

The typed read-only phase retains the original outer and custody locks
and authenticates issuance and linked learner Strict history before
opening the writable pair. The reader checks the canonical state image,
actual applied position, membership, binding and required local route,
and compatible retained log positions and purge marker. A qualifying
canonical state snapshot can cover a purged range. At a historical
index, the full term, leader and index must match. At a later index,
the position must also sort after the witness in the project's pinned
OpenRaft 0.10.0 advanced-mode LogId order (term, leader node ID, index).
Within a term, a higher leader ID can advance the position and a lower
leader ID refuses. The reader repeats the checked
state and log comparison and revalidates the original objects on the
writable pair. Retained credentials use the HostClock based on recovered
committed time. A typed failure retains acquired ownership and provides
no ready token. The sidecar envelope admits only the qualified no/zero
journal or WAL inventory with no SHM. Real process-loss qualification of
that envelope belongs to the later authorized campaign.

## Voter completion

The history lane spends its single receiver attempt before channel open
and holds the same installation mutex until completion. The first strict
applied proof covers channel open and sealing. The receiver writes and
syncs the voter objects outside the applied guard. The fresh second
proof authenticates readback and validates the CA while the two local
completion indicators stay pending. Outside the guard, the candidate
rechecks the exact ciphertext and commits and authenticates voter
history. A final binding check precedes the paired custody and installed
membership updates under the retained Job. A post-install eligibility
failure or lost Ack preserves historical local completion. This history
does not attest an Ack or give current signer authority. Current signer
use still requires the installed membership, fresh leader epoch, healthy
clock and applied voter proof. A history-enabled learner can recover
without issuer custody only when both voter objects and history are
absent.

The ordinary legacy inventory retains its prior grants. This change
adds zero Engine starts, owned Engine closes, Found calls, live Mints,
AnchorQueries, Redeems, enrollment proposals or membership proposals.
Actual activation, live issuance and distinct-UID deployment
qualification are deferred. Service and multi-host are **unverified on
real hardware**.
