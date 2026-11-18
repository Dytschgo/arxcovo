# PR179 history-count failure and bounded test correction

Sol6.1/high coordinator,2026-11-18. PR179 publishedhead
d4bd38b446899a380c16814af745c04cd1a3bb64, normalCI44272864519 attempt1.
Ubuntu job134693562006 failed at npm test after full Rust nextest654/654PASS,
22skipped,49.422s. All7new membership observation tests passed. No founder red
is present in this failure. Later UI test/build/smoke did not run after npm failure.
Windows/macOS outcomes pending at first capture; do not infer them from Ubuntu.

Failure at scripts/ci-budget.test.mjs85: synthetic recorder test expected historical
array length30 but saw31. PR added verifiedmain44269052508 to committed duration
history through the unchanged recorder. The test reads current real history in
its synthetic fixture. Thus the new history entry exposed a stale test literal;
it is not evidence of membership adapter regression or an engine failure.
Main exact6f4618c CI44269052508 remainsGREEN, not rerun.

Bounded Ubuntu log225666bytes retainedD:/Temp/arx-pr179-d4bd38b-hosted/ubuntu.log,
SHA256434064492224663860379ace192304cca7ca24259f2a387bc9da9adb9ea82cca.

## Exact local red/green evidence

D:/Code/Arxcovo, TMP/TEMP D:/Temp/arx-pr179-d4bd38b-hosted; no new worktree/build,
install/cachedelete/VM/admin action. Node24.18.0 existing toolchain.
Before source edit: node --test --test-name-pattern='synthetic record'
scripts/ci-budget.test.mjs ->1test/1FAIL, same31!=30 at85,272.384ms, exit1.
Applied one-line assertion correction: after.length == before.length+1.
Existing old-byte-prefix preservation, old-record deep equality, charged fields,
threejobrecords and duplicate no-write assertions remain byte-identical.
After: node --test scripts/*.test.mjs ->48/48PASS5690.843ms, exit0.
Root read both exact outputs. No Rust/runtime/engine rerun or CI dispatch.
Independent Sol6.1/high delta review pending before publication.

This is a routine pure-test regression correction within authorized PR recovery;
new precise allowlist is scripts/ci-budget.test.mjs oneassertion plus docs.
No production policy/workflow/budget/status logic or history entry is altered.
Original observation source allowlist remains frozen; no admission/founder source
changes, timeout/quarantine changes, fix-counter reset or hidden diagnostic.
SharedSol3/3 exhausted, historicalOpus0/2 unchanged. M2OPEN/M3UNCHECKED;
hardware **unverified on real hardware**. PR held pending reviewed newsource and
allfive normalgates/freshgreenmain/Windows<=660s. No same-head rerun authorized.

## Local evidence SHA256
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  history-after.stderr.txt
8fbd0ef5501bdb2d36322f1f38a764aff7c71d7871926aacbf5d4396fc20b880  history-after.stdout.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  history-before.stderr.txt
be542aceb1648a63a2cfd89ea88d64ef2899a4a83584fbaf2c68f0c35d6ab87c  history-before.stdout.txt

macOS job134693561979 now completedFAIL on the same history assertion31!=30,
Rust622/622PASS/8skipped64.770s. Local boundedlog221413bytes SHA256
8300f820161db1756d4d52458ee3953e9ea03aa01791a7606cd28cf41ae8cd52.
Windows stillinprogress at09:13:44Z; no inferred result. Correction committed
bfb1fc98 locally after independent review, not yetpushed so original evidence
completes without cancellation. ExistingPRwatch remains active for nextchange.

Original CI44272864519 attempt1 is now completedFAIL. Windowsjob134693561560
completed09:16:42Z (590s job), failed the same31!=30 assertion during npm tests,
after Rust nextest passed. Windows boundedlog231916bytes SHA256
b04efe5a787e7c10fe5261fa1abc44769b0891458c7f7b66b4b3b5211091e8a4.
Allthreeplatforms failed this history test; no run cancelled/retried. Later
UI/build/smoke/sample steps were not reached. Coordinator retained all3logs before
publishing reviewed test correction. Membership source remains unchanged.
