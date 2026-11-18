# PR182 source-preserving CI refresh

Owner-authorized refresh after PR185 main `e11b0767f1d108313824a5fd2600516561d70391`
firstCI44399330997 qualified920wall/827charged/93idle, all9jobsSUCCESS.
The original PR182 first attempt/cancellations and no-fix disposition remain
retained. This refresh adds no transport repair, runtime diagnostic or counter reset.

Merged that exact green main normally into local48c6022e, preserving published
2e4e0d3f and the three unpublished documentation commits as ancestors. Recovery
ref `refs/arxcovo-recovery/pr182-before-ci-refresh-20261118` retains48c6022e.
The separate work/ci-parallel-validation branch retains006ad7e8 and all unpublished
CI checkpoints. Latest applicable progress/evidence/history37 were carried as
documents, without duplicating merged Rust implementation.

All executable paths auto-merged. Four conflicts were docs only: Progress/history
use the latest verified checkpoint; ADR0028/brief differ only in trailing blank
lines and use main's text. Both versions are retained under
`D:/Temp/arx-pr182-ci-refresh`; old commits remain accessible. Existing PR182
first-CI evidence and Astra disposition files have identical blobs on both sides.

Exactly five non-document paths differ from green main: Cargo.lock, the arxd
manifest, the single peer_transport module declaration, and transport/core tests.
Core Git blob **07a81194a74ae586f680cdb9392a96f381eed013** and tests blob
**22ae4a9189e06aca022147b7913110cc48fb7fa9** match the original reviewed/published
source. The tokio-rustls0.26.6 dependency and its lock entries are unchanged.
No clock/engine/founder, workflow, quarantine, timer or capacity changes were made.

Coordinator Sol6.1/high ran fmt check and Windows all-target workspace Clippy
`--locked --offline -D warnings`: both PASS,42.845s combined. All build/scratch/
cache roots explicitly onD; Cfree689GB. Clippy compiles tests but does not execute
them. No local runtime, semantic-control, native/client, engine or founder test
was run. Evidence/source preservation and command logs are under the D root above.

Fresh independent integrated-source review and normal NEW-head first CI remain
mandatory, including all32 existing transport cases once on every OS and all
required checks, Windowsworker<=660, the existing900charged budget and fresh
exact-main health. No rerun/dispatch/forcepush or additional fix follows.
Production startup/factory/join/full RPC/container/hardware acceptance is not
established; M2 OPEN/M3 UNCHECKED, hardware **unverified on real hardware**.
EngineSol3/3/historicalOpus0/2 unchanged. OAuth/owner setup/babysitter untouched.
