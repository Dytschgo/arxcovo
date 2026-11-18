# Owned TLS/TCP Append implementation evidence

2026-11-18. Sol6.1/high core builder and separate Sol6.1/high test builder,
under [ADR0028](../adr/0028-owned-tls-append-transport.md). Coordinator inspected
the actual logs, verified all 29 indexed artifact hashes/sizes (12,799 bytes total)
and all five frozen source hashes. Final independent source
[review passed](../reviews/m3-owned-tls-append-source-sol61-review.md) at b936dd20;
both designated controls produced their intended failures and source was restored
exactly. Hosted checks remain pending.

The new private transport implements actual TLS 1.3/TCP Append exchanges with
explicit frame termination, current registry checks, owned tasks and resumable
close. It is not a production OpenRaft factory or service startup integration.
Only five non-document files changed. Removing the private module declaration
restores the exact canonical base consensus.rs bytes. Existing engine, PKI,
wire/registry modules, settings, workflows and quarantine remain unchanged.

## Baseline source (31 tests)

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

Final review identified this qualification in the original cancelled-close test:
at `CloseAborted` does not assert a nonempty JoinSet. An inbound deadline may
have completed work before the close grace expires. It proves keeper/original-
deadline persistence; aborted-but-unjoined cancellation requires independent
disposition rather than inference from the stage name. The added proof below
resolves this finding; it was not accepted on the original 31-case evidence alone.

## Retained artifacts

All under `D:/Temp/arx-tls-append-build/logs`, each bounded below 8 MiB:

| Artifact | SHA256 |
| --- | --- |
| artifact-index.json | af80dacccd070f0184e5b163e60605d90a0abcab2441abe807c0264994278f1b |
| transport-lib-baseline.log | 87401f9ec364c99204d08499b6f5a40119caf252a2213a4b8fc0663a938a0860 |
| transport-lib-baseline-result.json | 4bec4105e39afea9d26106ad8c2f2f53c2b88f8236c38deeaa3319d3643c3af4 |
| clippy-vote-final.log | 7fc982b8487ccf720e41cceab40241fac24e5c21b48a46510bfdda5d3c0e479f |
| clippy-vote-final-result.json | 2ed1be8d3dbb7887726584c273af683121910334ae9c8e540a621a9517b1e7b3 |

All-OS hosted checks, merge and an owner-testable release remain pending.
M2 OPEN / M3 UNCHECKED. No engine fixes or
counter reset: Sol3/3 exhausted, historical Opus0/2 unchanged. Full RPCs, actual
startup/join/custody/clock and container acceptance remain; real multi-host paths
are **unverified on real hardware**. All scratch/build paths are D:; no owner
admin, Lab Manager, VM/cache/user-data action or babysitter change.

## Added aborted-but-unjoined ownership proof

Final review confirmed the qualified case above did not prove a nonempty unjoined
record. Only a new test was appended; production code and the original 63,943-byte
31-test prefix remain unchanged. The added test holds an actual outbound TLS 1.3
exchange at ClientAuthenticated with an RPC deadline beyond close grace. It
observes `aborting`, a nonempty JoinSet, retained keeper and unchanged original
close deadline before and after another cancelled close. It then releases gates
and explicitly joins transport, caller, remote helper and clock observer before
asserting Unclean/reclamation. Final independent source PASS at b936dd20 resolved
the finding after reviewing this oracle and its first actual execution.

First exact execution, without repeating the original 31:
`cargo test -p arxd --lib --locked --offline cluster::consensus::peer_transport::tests::cancelled_close_retains_aborted_outbound_task_until_join -- --exact --test-threads=1`
passed **1/1**, zero failed/ignored, 701 filtered. 23.679 seconds total, 23.17
seconds compilation and 0.02 seconds test body. Formatting passed in 0.715 seconds,
workspace all-target locked/offline Clippy in 4.444 seconds. Thus 32 distinct
tests have passing evidence across two runs; no single 32-test run is claimed.

The final tests.rs blob is `22ae4a9189e06aca022147b7913110cc48fb7fa9`, SHA256
`672698ed398523cb756648b65f859cf7dfb9d624184e8f10350eae9aad619b9e`.
The builder's earlier a654 hash was pre-format, not the compiled final source.
All other four source blobs remain as in the baseline table.

Separate records under the same D: log root preserve the original 29 artifacts:
`artifact-index-close-retention.json`, `source-freeze-close-retention.json` and
`close-retention-original-evidence-check.json`. New target log SHA256:
`95c903dc5ad7d6b112aea6659ae654f748876f6d7b9c2db4b73600fd7fd7d849`;
result JSON: `fa43fbf34ca3ac5b85389731d3b6013e54abfb4db363b346f6cd63bbd61266b7`.
No failed runtime, engine test, control or policy change occurred for this addition.

## Two designated controls and exact restoration

Sol6.1/high builder ran each control once, sequentially, only after independent
source PASS. Coordinator read both exact patches, stdout/stderr, command/timing/
exit records and restoration manifests, and independently verified all 18 indexed
control artifacts (46,483 bytes) and final five source hashes/Git blobs.

| Control | Compilation | Exact semantic outcome | Total |
| --- | ---: | --- | ---: |
| M1 omit only inbound final registry callback | 18.67 s | Remove test, dispatch-count assertion at tests.rs:744, left 1 / right 0; exit 101 | 19.214 s |
| M2 omit only inbound clean EOF before request decoding | 18.57 s | Extra-byte test, dispatch-count assertion at tests.rs:700, left 1 / right 0; exit 101 | 19.058 s |

Both tests completed gate/helper/transport cleanup before the intended assertion.
Neither failure was a compile error, timeout, setup error or unrelated red. Real
handshake verification remained in M1; outbound reply termination remained in M2.
Each exact reviewed source file was restored after **each** control. Root verified
the final non-document tree equals reviewed b936dd20; independent review's
restoration conditions are satisfied. The two controls were never repeated.

An earlier shell wrapper had a nested here-string parse failure before script
creation, source mutation or Cargo. `setup-command-failure.json` records zero
control execution. It was replaced with a standalone driver; each semantic control
then executed once. No founder/engine diagnostic or fix attempt was made.

Evidence under `D:/Temp/arx-tls-append-build/controls`:

| File | SHA256 |
| --- | --- |
| artifact-index.json | 0df9f470aa9ae7d26dedae2d91f019dd92c6dea054e14fc3d8b8002bfb2bfcae |
| m1-result.json | 5b628f9c8f5a8862cd6b6bad56ecfeb6889c8088f4cb430881fd348ca0a0e748 |
| m2-result.json | 702c034a2e4b3d380186caef1fa0a5a52cc386a3a221796d1c01903022e29014 |
| m1-restored-source.json | 3ceee8542a450184cde72539980d9f7ae78f837c1a0f8c3d3404b85b1364b9d8 |
| m2-restored-source.json | 3ceee8542a450184cde72539980d9f7ae78f837c1a0f8c3d3404b85b1364b9d8 |

No runtime/fmt check was repeated after restoration. The local library test
executable remains the M2 mutant; hosted CI must compile the restored baseline.
Prior passing evidence applies by exact source identity. Engine Sol3/3 exhausted,
Opus0/2 and all owner fences remain unchanged.
