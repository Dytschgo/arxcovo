# PR190 first hosted observation

Published head14ace1ccee4834f2b8030c31e22b55ed1b668e63, first normal CI44476535131
attempt1 is terminal FAILURE414wall. Earlier partial snapshots are preserved;
terminal-run/jobs and execution-proof supersede their pending Windows status.

All nine worker logs are retained under `D:/Temp/arx-pr190-first-ci`, each bounded
8MiB/45s, none truncated. Five initial logs were reused; only four remaining logs
were fetched. Full metadata, merged log manifest, execution proof, policy measure
and22-file evidence manifest retain exact IDs, times, hashes and test names.

- Ubuntu Rust135398336006 passed786 tests,22 skipped, body52.300s; all nine binding
  cases passed once, including actual Unix swap/private-mode and ABI-header/alias
  refusal. This component execution does not close remaining native/platform gaps.
- macOS Rust135398335908 failed at Verify pinned SQLite binding build: build.rs43
  rejected DYLD_FALLBACK_LIBRARY_PATH. No macOS Rust tests ran. Its value/provenance
  was not logged; a narrow Cargo-environment contract correction is under design.
- All three UI workers ran60 Node cases:58 passed/two failed. Both are exact
  workflow expectations that omit the added verification step and Cargo path
  variables. Browser/Electron stages were not reached.
- Windows Rust135398335545 failed after393s:673 passed/two failed,36 skipped,
  125 unrun out of800 selected, body61.592s. All10 binding cases passed once.
  service::owner_windows::tests::n1_create_new_and_existing_refusal and
  n2_exact_hash_replace_and_conflict_cleanup panic at publish().unwrap with Io
  (owner_windows_tests.rs104/132). Underlying cause remains unknown; these are
  distinct from the earlier PR189 founder failures.
- Windows/macOS/Ubuntu aggregate gates failed closed in14/6/10s.

Independent exact-head source attestation7002303126 and agent-review59339314382
remain source evidence, not CI acceptance. The two stale Node expectations were
corrected locally in076f156a, retaining exact command/order/variable assertions.
Six focused workflow tests pass; the initial focused run reproduced the two red
assertions. Before/after logs are under `D:/Temp/arx-pr190-corrections`.

Policy computes409rawcharged/5potentialidle, withheld:not-green. No history entry,
source push, dispatch or rerun followed this first observation. A fresh independent
Astra6.2/high evidence-only Windows disposition completed HOLD/UNKNOWN at
`D:/Temp/arx-pr190-windows-disposition`, reportSHA256
`023988f58086f6c779dcad0adb3cd06feaa089d707eb7bc5bc07ef9fab19b969`.
Root verified22 retained file/manifest pins and nine raw logs. Both failures are
first create-new publication; N2 never reaches hash conflict or replacement.
Publish's Io collapses multiple pre/post-rename failures, leaving the native error,
failed operation and final file state unknown. The new exported temp root reaches
default-temp fixtures, but no resolved fixture paths were recorded and no causal
attribution is established. Both PR189 founder cases passed once in this run;
their earlier hold remains separate. No native retry/fix allowance follows.
macOS guard correction is design-only, output
`D:/Temp/arx-pr190-macos-guard-design`. No broad DYLD exemption or policy relaxation
has been adopted. Existing founder/Engine holds and exhausted counters remain.
M2 OPEN / M3 UNCHECKED; **unverified on real hardware**; operational NO-GO.
