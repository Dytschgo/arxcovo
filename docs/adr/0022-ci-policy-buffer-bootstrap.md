# 0022: Owner bootstrap for the CI policy buffer repair

Status: owner-authorized delivery route, 2026-11-13; admin merge not performed.

Trusted policy run 43326387032 failed ENOBUFS reading completed CI runs above
its 1 MiB subprocess buffer. #54 independently passed frozen A1-A6 on head
c5bb191 (567s), but main policy cannot evaluate it. Branch protection pins
ci-time-budget to GitHub Actions app15368, so local status publication would
not satisfy the gate. A repair PR cannot claim a measured speed reduction.

The one-line finite 8 MiB repair and regression tests were independently
reviewed by Opus high after Sol high implementation. Policy semantics,
thresholds, first-attempt selection and required sources remain unchanged.
Tests reproduce the failure and retain fail-closed behavior above the bound.

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
