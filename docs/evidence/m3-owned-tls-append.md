# Owned TLS/TCP Append implementation evidence

2026-11-18. Sol6.1/high core builder and separate Sol6.1/high test builder,
under [ADR0028](../adr/0028-owned-tls-append-transport.md). Coordinator inspected
the actual logs, verified all 29 indexed artifact hashes/sizes (12,799 bytes total)
and all five frozen source hashes. Final independent source review is pending;
no controls or hosted checks have run for this source yet.

The new private transport implements actual TLS 1.3/TCP Append exchanges with
explicit frame termination, current registry checks, owned tasks and resumable
close. It is not a production OpenRaft factory or service startup integration.
Only five non-document files changed. Removing the private module declaration
restores the exact canonical base consensus.rs bytes. Existing engine, PKI,
wire/registry modules, settings, workflows and quarantine remain unchanged.

## Frozen source

| Path | Git blob |
| --- | --- |
| Cargo.lock | 4c487cc3029717022379fdbad5718d9ee15368c4 |
| crates/arxd/Cargo.toml | d195eaa24cf46fc5bf2c9580d6cf5ccf943f011e |
| crates/arxd/src/cluster/consensus.rs | 6dd5396d510b4d5c8eb1dcf7a222041d1d21f349 |
| crates/arxd/src/cluster/consensus/peer_transport.rs | 07a81194a74ae586f680cdb9392a96f381eed013 |
| crates/arxd/src/cluster/consensus/peer_transport/tests.rs | e670ce3f695bdc04c0bcce125a91a3b60dd640a6 |

Core SHA256: d74bbc56d1519cf5f30114d18f0c25151db46ee46a3a915e260af50532b1137f.
Tests SHA256: f8d85319ff9a9411164656c70546d5d5e114bbc497f977cc280308fe1b1d7e07.
Local `D:/Temp/arx-tls-append-build/logs/source-freeze.json` records all five:
SHA256 a1fe0f9b7003846dbbeaeaf33f025c4fadc694f9e5b90bca1eb5210dd9b9be55.

## Checks actually run

- Final `cargo fmt --all -- --check`: builder reports exit 0 after the last test
  case addition, `fmt-vote-check.log`; final duration was not captured separately.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  exit 0, 4.438 seconds on frozen source.
- First actual focused execution:
  `cargo test -p arxd --lib --locked --offline cluster::consensus::peer_transport::tests:: -- --test-threads=1`:
  exit 0, **31 passed, 0 failed, 0 ignored, 670 filtered**. 35.469 seconds total,
  including 33.30 seconds compilation; test body 1.75 seconds.

No runtime test was retried. Earlier `--bin arxd` no-run checks did not compile
the library test module. A mistaken focused `--bin` invocation returned exit 0
but ran **zero tests**, one filtered, in 9.618 seconds. Its original logs remain
and grant no transport-test credit. Workspace all-target Clippy did compile the
library tests. Correcting the target supplied their first actual execution.

Locked/offline feature trees show only the added tokio-rustls 0.26.6 ring adapter,
existing rustls 0.23.46 ring/std and single Tokio 1.48.0. The lock diff adds only
the adapter package and direct arxd edge; no aws-lc/TLS1.2/default expansion.

The 31 cases include real TLS half-close and independent framing, an asserted
232,688-byte canonical membership fixture, actual Found/Remove registry states,
isolated root/cluster/SPKI/expiry refusals, route/request/reply validation,
termination/truncation, deadlines, shared 4/32 limits, caller/close cancellation,
store release and panic-payload disposal outside the ownership mutex. Deliberate
joined panics are fixtures for mutex poison, dispatcher failure and panic payload
destruction. They are not engine/founder executions.

Capacity timing is qualified: the 32-socket fixture uses real TCP/TLS while Tokio
policy time is controlled. A bounded blocking observer suppresses automatic
clock advance, performs no transport/crypto I/O, and is released/joined before
assertions. This supplies no wall-time performance or multi-host evidence.

One test qualification is explicitly pending review: the cancelled-close test
at `CloseAborted` does not assert a nonempty JoinSet. An inbound deadline may
have completed work before the close grace expires. It proves keeper/original-
deadline persistence; aborted-but-unjoined cancellation requires independent
disposition rather than inference from the stage name.

## Retained artifacts

All under `D:/Temp/arx-tls-append-build/logs`, each bounded below 8 MiB:

| Artifact | SHA256 |
| --- | --- |
| artifact-index.json | af80dacccd070f0184e5b163e60605d90a0abcab2441abe807c0264994278f1b |
| transport-lib-baseline.log | 87401f9ec364c99204d08499b6f5a40119caf252a2213a4b8fc0663a938a0860 |
| transport-lib-baseline-result.json | 4bec4105e39afea9d26106ad8c2f2f53c2b88f8236c38deeaa3319d3643c3af4 |
| clippy-vote-final.log | 7fc982b8487ccf720e41cceab40241fac24e5c21b48a46510bfdda5d3c0e479f |
| clippy-vote-final-result.json | 2ed1be8d3dbb7887726584c273af683121910334ae9c8e540a621a9517b1e7b3 |

No final source review, two designated controls, all-OS hosted checks, merge or
owner-testable release is claimed. M2 OPEN / M3 UNCHECKED. No engine fixes or
counter reset: Sol3/3 exhausted, historical Opus0/2 unchanged. Full RPCs, actual
startup/join/custody/clock and container acceptance remain; real multi-host paths
are **unverified on real hardware**. All scratch/build paths are D:; no owner
admin, Lab Manager, VM/cache/user-data action or babysitter change.
