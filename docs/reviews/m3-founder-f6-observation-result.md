# F6 single natural observation result

OpenAI / Sol 6.1 / high; isolated worktree; no children. Root authorized
compilation plus exactly ONE invocation of the reviewed patch. Grant consumed.
Baseline HEAD: 8661313b4540ab5ed9be1756f1da56fc13a8d37e.
Failed main: 09134085dce93742d8812821dbe11834dcee7a9c.
Reused independent safety PASS: docs/reviews/m3-founder-f6-observation-sol-review.md
(review retained from b6a28f1a); plan: docs/briefs/m3-founder-f6-observation.md.
Debug phases 2-5 remain unsatisfied; bounded grant superseded loop/fix advice.

## Actual execution and admission

Actually verified all 130 tracked crate bytes against failed main before/after.
Source/restoration ordered path-NUL,bytes-NUL aggregate SHA256:
026f868a360d527e742e6f84dbfb00b9931ce485455a0681ddbabeff02f342ac
Reviewed patch SHA256:
6fb49eba004936204ec45fb113f0f89b640fb2625686546222a6c67549b7d3cd
Private cargo-home, target and evidence paths and ancestors checked non-reparse;
cached registry tree checked before copying and private copy checked afterward.
Existing registry dependencies copied; no shared writable cache or compiled
artifact reused. Fresh target compiled offline; no installation/profile override.
No ancestor/home Cargo config, local nextest config or inherited CARGO/RUST/
NEXTEST environment override. No competing cargo/rustc/nextest/arxd at admission.
Actual tools: cargo 1.95.0 f2d3ce0bd; rustc 1.95.0 59807616e;
cargo-nextest 0.9.151 6b3f994577a98874a2e0ea5bef77fbbab3434044.
Unchanged budgets/assertions/clock calls/Owners/permits/task scope/drop semantics
are reused static review evidence, not independently measured runtime identities.

Exact command (once; includes compilation):
```text
cargo nextest run -p arxd --lib --locked --offline -E 'test(=cluster::consensus::issue::found::found_tests::f6_adapters)' --test-threads 1 --retries 0 --fail-fast --success-output final --failure-output final
```
UTC start: 2026-11-17T23:45:33.555370+00:00
UTC exit: 2026-11-17T23:46:45.295360+00:00
Actual command exit code: 0 (captured directly from cargo process, not a pipe).
Compilation: Finished test profile [unoptimized + debuginfo], 1m 08s.
Nextest default profile; 1 test passed, 632 skipped; F6 0.119s; summary 0.120s.
One post-return emission: overflow=false, count=14; missing trace: false.
All preflight/initialize/leaderwait/proposal/guard1/guard2 exits were Ok.
Finaldeadline exit: Ok; elapsed_us=35415; remaining_us=4964585; pending=none.
Complete scalar phase output retained privately in evidence/trace.txt.

## Artifacts and restoration

Private retained artifact root: local .t3/f6-8661313b (outside repository).
No raw log, target or cache committed or uploaded. Artifact paths below are
relative to that private root. SHA256 unless stated otherwise.
evidence/nextest.log: 087ef749ef9367b40e6d4e6c630284c0f5cd9c7c495a91f26af6bf479afdd1d7
evidence/trace.txt: cf634e13e8faf7228f6ecccc0ca9d41c4720704cb0bdbf72ee8dcecb33626977
evidence/admission.json: cf4db528cbdb389c9ee05e924c7a85201af015ebaa464d442441b5c40f3a2c16
evidence/patched-hashes.json: 3611552c550b120fd58a082d8f05d3fedffadf688928b1fdf90e325c6758950b
evidence/run-outcome.json: 73f823155f5db8470c917c0d4c4f418dc72ce2c731268d69666fa59ba812c452
evidence/restoration.json: 9539fbcc0e79f7b3eab55a462def1b2019230fbe21b013e406abb1cddb440ede
evidence/artifact-hashes.json: 8cc04d327d9231f689234b47fdb1ff41446e1b3c3a62e994a340c3c8189e6642
Compiled binary: target/debug/deps/arxd-648de2072896f9ec.exe
bca2f82c53a8fbc4f1ce3859d3a50abe1daf5bd2a48f85a8fb1c61ef640e5e9e
Applied found.rs: d6596f0b2371bd4a854d2d7213d87d20d4c03f63d994575217cbe22749f73013
Applied found_tests.rs: 6c601f548eed3e02c43adc48a0bdec18f958d28e70d6a334a39ff2a4874408a5
Restored found.rs: b23e9e330ba67da3115df49fb4da708bdf02de57a6b3b395a7e0c79c131542a7
Restored found_tests.rs: 3433f70210ca5aff167f7d0b8418915ec4c2bf3f8f9cfd5ca92a9a1793a3457b
Cargo.lock: c899c5d793c4449e53992f70423936dcc80bdc464b97e828145cd3e9825081b6
rust-toolchain.toml: 97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe
Full source/tool/control hashes retained in admission.json; full patched source
hashes in patched-hashes.json; harness and artifact hashes in artifact-hashes.json.
Before reversal all 130 crate hashes equaled captured expected patched hashes.
Only reviewed patch reversed. All 130 crate bytes equal failed main afterward;
DEBUG-F6-OBS absent from active crate source. Restoration UTC: 2026-11-17T23:46:49.833857+00:00
Post-outcome relevant process inventory empty; no owned survivors observed.
No kill, cache deletion, VM/admin work, network/publication or post-restore test.
No fixes, repeats, F7/F8, stress, delay, tuning or attempt-counter changes.

## Interpretation and next bounded proposal (not executed)

GREEN is one instrumented natural local observation only. Hosted exact red
(initial issuance Unavailable) was NOT reproduced; no causal localization, fix,
hosted stability or repair-budget credit. Observer monotonic reads can perturb
timing. No incomplete expression observed in this invocation. On an exact red,
unmatched entry would localize only a caller expression, not worker completion;
proposal inner ? can bypass exit; final refusal after six Ok exits would not
identify a specific cause. Overflow invalidates a trace; missing output cannot
be recovered by rerunning. Original final predicate sample remains unobserved.
Next proposal: coordinator reviews this preserved one-run evidence and designs
a separately reviewed bounded hosted observation if still useful. No execution,
publication or additional run is authorized by this proposal.
PROGRESS proposal only: Sol6.1/high compiled exact reviewed probe and ran F6 once;
exit0, complete14-event/no-overflow green local trace; source restored exactly;
hosted red unreproduced; grant consumed; no follow-on execution. PROGRESS unchanged.
Sol3/3 exhausted; historical Opus0/2 unchanged; M2 open; M3 unchecked; PR176 held.
Hardware paths **unverified on real hardware**.
