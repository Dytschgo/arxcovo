# M3 pure enrollment core evidence

Implemented by Sol high on 2026-11-15, from base `9867540` and the corrected
140-line first-slice brief accepted by the independent Sol delta review.

Scope: the five authorized Rust paths, plus this evidence file. `lib.rs` adds only
`pub mod cluster;`; `cluster.rs` contains only `pub mod enrollment;`. No dependency,
Cargo, workflow, ADR or PROGRESS change. The coordinator retains milestone tracking.

The core has no I/O, clock reads or native integration. Randomness is injected through
`fill`; test randomness and SPKI inputs are synthetic. State retains verifiers, not
secrets/proofs. Credentials have redacted Debug and no Display/Serialize implementation.
Grant fields are private, with getters and no public constructor. Applying to a local
clone can produce a grant: this is explicitly **not proof of consensus commit**.
Future trusted adapters must use committed entries, sign only the bound key and freshly
check issuance both before signing and before certificate release/persistence.

## Actual local checks

All Rust commands used existing `D:/Code/arxcovo-a12-delivery-source/target` as
`CARGO_TARGET_DIR`, `TEMP` and `TMP`; the target and its two parent directories were
inspected as ordinary directories without reparse attributes. No install or cleanup ran.

- `cargo fmt --all --check`: passed.
- `cargo nextest run -p arxd --locked --offline cluster::enrollment`: passed,
  13 tests, 454 unrelated tests skipped. Final run ID:
  `61b71faf-c7f2-4f6f-b1cf-965bc7077bb5`; test execution 0.089 seconds,
  preceding incremental compilation 7.77 seconds.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  passed; final incremental run 3.37 seconds.
- `git diff --check`: passed before staging; staged diff checked before commit.

The initial focused run passed 10 tests and failed one test fixture because its
synthetic marker was 33 bytes. The marker was corrected to an explicitly typed
32-byte array. Subsequent 13-test runs passed. Production logic needed no fix.

Tests cover bounded text parsing (including whitespace-padded valid text beyond the
input cap), checksum and independent Python hashlib vectors, exact-byte SPKI hashing
and size limits, injected RNG failure at either draw, raw/hex marker privacy, proof
binding, first-applier key races, identity/request conflicts, node monotonicity and
exhaustion, pending/retained cap ordering, tombstone boundaries, replay without expiry
extension, deterministic replicas, common pruning on rejection, generic joiner replies,
proposal skew/maximum timestamps, and issuance guard overflow/skew/expiry boundaries.
The delayed sole proposal deliberately gets a proposal-time grant, then the injected
fresh-time guard refuses it at one hour. This is not physical-expiry evidence.

No mutation testing was run; no mutation survival/kill claim is made. No broad test
suite or hosted check was run. Existing CI/policy budget definitions were read; no gate
was modified or bypassed. Fresh independent Opus review and green hosted required checks
remain necessary before publication/merge. Nothing was pushed or published.

## Remaining limits

M2 remains open. Real host/native acceptance, physical expiry, signing, CA placement,
consensus commit authority, Raft storage/crash behavior, three-node availability, mTLS,
LAN/Tailscale transport and SPKI/CSR parsing or proof-of-possession are outside this
slice and **unverified on real hardware**. No owner/admin, VM, Lab Manager or new native
grant/retry action was attempted; A7 FINAL NO-GO remains closed and A12 D7l cleanup
remains unconfirmed.
