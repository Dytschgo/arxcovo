# 0022: Owner bootstrap for the CI policy buffer repair

Status: executed 2026-11-13 under separate explicit owner approval;
protection restored and exception exhausted.

Trusted policy run 43326387032 failed ENOBUFS reading completed CI runs above
its 1 MiB subprocess buffer. #54 independently passed frozen A1-A6 on head
c5bb191 (567s), but main policy cannot evaluate it. Branch protection pins
ci-time-budget to GitHub Actions app15368, so local status publication would
not satisfy the gate. A repair PR cannot claim a measured speed reduction.

The one-line finite 8 MiB repair and regression tests were independently
reviewed by Opus high after Sol high implementation. Policy semantics,
thresholds, first-attempt selection and required sources remain unchanged.
Tests reproduce the failure and retain fail-closed behavior above the bound.

## Historical approved plan (superseded by actual execution below)

After explanation, the owner approved publishing a standalone repair PR.
Normal platform CI and exact-head different-provider agent-review must pass.
The OWNER then performs the one-time admin merge bypassing ONLY the broken
ci-time-budget gate. The agent does not perform that admin action, change
protection or force a success status. The exception applies to this repair
only. No product PR or performance criterion receives an exception.

Once merged, trusted bot policy evaluates #54 normally, followed by normal
merge checks, a fresh main measurement and dependency-order integration.
No performance-attempt counter is reset. No rerun is authorized to hide a red.
Record actual owner merge and resulting main/policy runs in PROGRESS.

No runtime product, owner account, service, VM or experiment operation is
introduced. M2 remains incomplete; native paths are **unverified on real
hardware**. The existing raw-error-output advisory is separate follow-up work.

## Actual owner-authorized execution

2026-11-13: owner subsequently explicitly approved the agent performing this
merge. GitHub rejected admin merge because enforce_admins=true. Owner then
separately approved removing ONLY ci-time-budget temporarily and immediately
restoring the original required-check configuration. Four other required
checks were verified green on pinned head a5ee45f. Snapshot, temporary PATCH,
pinned squash merge and restoration were performed in one script with finally.
Readback verified the original five check contexts, app pins and strict=true.
PR57 merged at 16:05:11Z as f1340a4; its tree equals the reviewed head. No
other protection was changed. The exception is exhausted and grants no
bypass for #54 or product PRs. Main CI43459116760 remains pending here.
