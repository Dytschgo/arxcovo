# PR178 first-attempt observation and merge — 2026-11-18

Sol6.1/high coordinator verified exact published head11a34312afb6027b82478686fc1e728c01cd393b.
Normal all-OS CI44267088243 attempt1 SUCCESS, wall633s (08:27:09–08:37:42Z).
Jobs: Windows629s, Ubuntu311s, macOS397s. Windows meets feature<=660s.
Allfive required gates SUCCESS; independent Astra6.2 verdict posted6973198836,
previous identity verdict retained by source identity. No formal GitHub review.

Bounded per-job log reads <8MiB in D:/Temp/arx-pr178-11a34312-hosted.
Verified exactly one PASS for each of the three peer_identity tests and three
postshutdown observer tests on every OS. Founder anchored_preproof_single_voter
PASS on Windows1.009s/Ubuntu0.129s/macOS0.209s. Full workspace nextest:
Windows655passed/36skipped; Ubuntu647passed/22skipped; macOS615passed/8skipped.
No FOUNDER-POSTSHUTDOWN runtime rows: green means no reproduction, not a repair.
Narrow eventual-owner-success prediction unresolved. No diagnostic rerun or fix
credit. Windows optional three-node sample skipped_budget e_s637, no case executed;
its guard60passed. No stability/re-enable/three-node Windows credit.

Local logs SHA256:
windows.log406636bytes 2ae35cd0600d82130ed361e616b3c97f7373cec04839f712546a445d2bc503eb
ubuntu.log422099bytes 95270ed9272fcc6e98328d154b05c437e0642f1b8441f3bcd8826249bc5e4693
macos.log383347bytes 2499a7ac3d5d8d8f423a90972cc252c1e487dd730430c3648d5e42eef8a0d14c
First summarizer did not remove rendered ANSI caret escapes and its exact-name
assertion failed; corrected escape normalization passed all exact-name counts.
No runtime source or test rerun resulted from this parser correction.

Immediately before merge: live main exacte3e79089, CI44229539357 attempt1SUCCESS;
PR exacthead11a34312 CLEAN/allfiveSUCCESS. Expected-sha guarded merge-method=merge
returned HTTP405 'Merge commits are not allowed on this repository'; no change.
Normal expected-sha guarded squash succeeded, PR178 MERGED at08:40:36Z as
6f4618c76256c2ead9dfb501c2c3946fde14a81b. Live main matches merge SHA.
No bypass/admin/forcepush/protection changes. Resulting exact-main CI still pending.
Do not claim new main GREEN until its own first attempt completes. LastmergedPR178;
M2OPEN/M3UNCHECKED, Sol3/3 exhausted, historicalOpus0/2 unchanged.
Hardware **unverified on real hardware**. Existing babysitter untouched.

## Exact-main verification — 2026-11-18

Sol6.1/high coordinator verified CI44269052508 attempt1 completedSUCCESS on exact
live main6f4618c76256c2ead9dfb501c2c3946fde14a81b. Started08:40:38Z, completed08:50:41Z:
603s wall; jobs Windows598s, Ubuntu224s, macOS427s. All job conclusionsSUCCESS.
No queue-credit claim made from these timestamps. Bound each job log read to8MiB;
actual sizes Windows404158, Ubuntu419658, macOS383265 bytes. Logs retained only
at D:/Temp/arx-pr178-main-6f4618c (no raw logs committed).
Verified exact PASS counts: identity3 and observer3 eachOS, founder proof1eachOS.
Full nextest Windows655passed/36skipped, Ubuntu647passed/22skipped, macOS615passed/8skipped.
Founder Windows0.905s/Ubuntu0.119s/macOS0.287s. No postshutdown refusal rows; cause
still unreproduced/unfixed, late prediction unresolved, no stability credit.
OptionalWindows sample push skipped_budget e_s606; guard60PASS, zero sample cases.
No rerun/dispatch/repair. Main green removes its merge hold only; M2OPEN/M3UNCHECKED.
Sol3/3 exhausted, historicalOpus0/2 unchanged. Hardware **unverified on real hardware**.

Main logSHA256:
windows.log41bb88ad67f99276a263922ce5aec565c2718a294dce3f48460398f40e8cee50
ubuntu.log05ac16bb5cbb4d3249031397328e9e6869e77cf3c928f925a1939166084b3178
macos.log07bb07134436fb9c6c5b7da92701de148738409ae76c6fb7fab3bc9ccb8c874b
