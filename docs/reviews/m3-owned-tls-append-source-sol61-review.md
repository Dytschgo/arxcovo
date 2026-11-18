# Independent owned TLS Append source review

Sol6.1/high, 2026-11-18. PASS at
`b936dd20e5baa42e7089e8ffbb69559f9acc7c26` against base
`cd8907500dcd89fddc0c659590f962cbf03c7ed5`.

Standards: PASS, zero remaining findings. Exactly five non-document paths are
within the allowlist. Existing engine behavior, PKI/wire/registry code, timers,
capacities outside this transport, workflows and quarantine remain unchanged.

Spec: PASS, zero remaining findings. The earlier abort-and-join proof gap was
resolved by the added actual-TLS outbound case with a deadline beyond close
grace, explicit nonempty JoinSet/keeper/original-deadline assertions across
cancelled close, and observed cleanup joins before assertions.

Evidence is 31 passing baseline cases plus the added test's first exact 1/1 PASS:
32 distinct tests across two runs. Final fmt and workspace all-target Clippy
passed. The zero-test binary invocation grants no credit. The reviewer verified
all 29 original and nine supplemental indexed artifact hashes/sizes, final five
source hashes, and unchanged original 63,943-byte test prefix.

Security: PASS, zero remaining findings. Actual identity extraction, final
registry authorization, bounded framing, request/reply validation, try-admission
and cancellation ownership satisfy the contract. Panic payloads and store/
resources are destroyed outside the ownership mutex.

| File | Final reviewed Git blob |
| --- | --- |
| Cargo.lock | 4c487cc3029717022379fdbad5718d9ee15368c4 |
| crates/arxd/Cargo.toml | d195eaa24cf46fc5bf2c9580d6cf5ccf943f011e |
| crates/arxd/src/cluster/consensus.rs | 6dd5396d510b4d5c8eb1dcf7a222041d1d21f349 |
| crates/arxd/src/cluster/consensus/peer_transport.rs | 07a81194a74ae586f680cdb9392a96f381eed013 |
| crates/arxd/src/cluster/consensus/peer_transport/tests.rs | 22ae4a9189e06aca022147b7913110cc48fb7fa9 |

Core SHA256: d74bbc56d1519cf5f30114d18f0c25151db46ee46a3a915e260af50532b1137f.
Tests SHA256: 672698ed398523cb756648b65f859cf7dfb9d624184e8f10350eae9aad619b9e.

PASS permits exactly the two designated once-only semantic controls. It remains
applicable after exact restoration of all five blobs. A surviving or unrelated-
red control requires disposition, without retry or additional controls. Required
hosted gates remain pending.

Reviewer performed no builds, tests or mutations. No engine, production startup,
join or container acceptance follows; real multi-host paths remain **unverified
on real hardware**. Exhausted counters and owner fences remain unchanged.
