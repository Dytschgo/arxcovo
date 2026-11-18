# PR179 corrected-head gates and merge — 2026-11-18

Sol6.1/high coordinator verified exacthead2f888992f82683f54cb9c8170b8d061d6773ea03.
NormalCI44274746575 attempt1SUCCESS537s wall (09:18:53–09:27:50Z).
Windows533s<=feature660s; Ubuntu267s/macOS216s. Allfive requiredchecksSUCCESS,
strictprotectionunchanged. Exacthead independent Sol reviews cover membership
source and one-line historytest correction by verifiedsourceidentity.

Retained perjoblogs D:/Temp/arx-pr179-2f888992-hosted, each bounded<8MiB.
Verified each of7new membership tests PASS once on EACH OS. FullRust nextest:
Windows662passed/36skipped, Ubuntu654passed/22skipped, macOS622passed/8skipped.
All48NodepolicytestsPASS eachOS; formerly failing recorder assertion nowPASS.
No local restoredsource runtime rerun; hosted compiled exactrestoredsource.
Initial log summarizer stopped on Windows cp1252 output of a checkmark; reran
only parser under PythonUTF8, all exactcase counts succeeded. No testrerun.

Logs SHA256:
windows.log287138bytes 12b79ed77c9466d4725ff8cc7f7a3f9c498b32a724b82a84f93652692852e234
ubuntu.log249951bytes 0fd16023fe5f55418bae49748e7269100f63311aa626bdc73576be8b278c1611
macos.log229999bytes 134852c2d8891becc9736309b40fbf8b9862dbaa425774ec159ccd5ab803dcd5

## Optional quarantined Windows sample

Existing sample outcome=diag_fail,nextest100,wall47219ms,orphans0,e_s495.
T1/R6s/R6p pass; T2/T3/T4/U6/R6c fail. T2WriteElapsed(node3),T3WriteElapsed(node1),
T4LeaderElapsed(node1); U6StopOrphaned(node1),owners_left55/propose_held55,
engineJoined,total5001ms. Several diagnostic sections truncated under existingcap;
no expansion/retry. This is retained negative quarantine evidence, not a required
check failure, new enginefix, shared cause, stability/re-enable credit or
productionadmission acceptance. Sol3/3 exhausted/Opus0/2 unchanged. No repair,
timeout/quarantine/settings change or further diagnostic authorized/executed.

## Merge and next-main hold

Immediately before merge: live main6f4618c76256c2ead9dfb501c2c3946fde14a81b,
CI44269052508 attempt1SUCCESS; PRhead2f888992CLEAN/allfiveSUCCESS.
ExpectedSHA guarded normal squash mergedPR179 at09:28:55Z as
6bf8f46bc87a05bed01e342c1a37f68427e4c16b. Live main matches. No bypass.
Resulting exact-main CI pending; featuremerges held until ownfirstattemptGREEN.
LastmergedPR179; M2OPEN/M3UNCHECKED. API is guardedwatermark only; actualadapter
quorum/race/cancellation and production admission remain separate proof gaps.
Hardware **unverified on real hardware**. Owner PC/setup/containerplan local
commits preserved, no native/admin/container action. Existing babysitter untouched.

## Exact-main first-attempt confirmation

Coordinator Sol6.1/high personally checked main6bf8f46bc87a05bed01e342c1a37f68427e4c16b
CI44276313116 attempt1SUCCESS,09:28:57–09:38:22Z565s wall. Jobs Windows561s,
Ubuntu288s/macOS419s. Verified exactly7newtestsPASS eachOS and Node48/48eachOS.
FullRust Windows662PASS/36skip, Ubuntu654PASS/22skip, macOS622PASS/8skip.
Bounded perjoblogs<8MiB retainedD:/Temp/arx-pr179-main-6bf8f46, SHA256:
windows246721bytes a198239bb862b350b172a45ee53f07d0b60bac30e7851da31d299fa877017afd
ubuntu248877bytes e3c81243bdacbaf78d11060a09c708d17be2212e717ae6e2798b52a75e200986
macos229980bytes f192a775823d7b57d6f7d84fbb82f4f4ee911cab019fafaa862f1d1f758a0ca9
Existing node scripts/ci-budget.mjs --record executed with D TMP/TEMP; validated
history565wall/563charged/2idlequeuecredit,longestjob561, no statusmutation.
Automaticpolicy44277813800SUCCESS on sameexactmain. No rerun/dispatch.
OptionalWindows sample all_pass,nextest0,wall29266ms,orphans0,e_s540. This single
passing observation does not erase prior PRsample3pass5fail, diagnosecause or earn
stability/re-enable/repair credit. Counters unchanged, no newfix/diagnostic authority.
M2OPEN/M3UNCHECKED; API quorum/race/cancel and real transport remain unproven.
Hardware **unverified on real hardware**. Native PC waits verified owner setup.
