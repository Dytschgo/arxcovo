# Independent Sol review: Ubuntu mirror fallback

OpenAI / gpt-6.1-sol / medium coordinator, single-agent cross-provider review of Anthropic Opus high implementation. Base `1361864104fec9df3df8afa72480e6e489fd6f4c`, head `bfa177f` (runtime `821d496`). Both resolved from Git objects before retaining the exact candidate. Review covers all five changed files and surrounding publication/trust code; not an independent review of Sol's earlier concurrency implementation, which has the retained Opus recheck.

## Standards

PASS, no blocking findings. The change is bounded to CI helper/tests/evidence/docs. Existing no-follow, root-owner, regular/single-link, directory policy, byte/identity recheck, staging, atomic replacement and fixed-message error handling remain. The mirror-list target now gets the same publication checks as changed source files. Indirect source stanzas are still parsed and trust-validated even when their bytes stay unchanged. All parent history is kept. Possible test smell: apt_mirror_order models documented selection instead of exercising APT; this is honestly labelled and hosted validation is mandatory.

Advisory: a synthetic original priority:0 becomes1 and ties the Kernel entry. That is harmless randomized equal-priority ordering, but the claim that Kernel is always first applies to the observed1/2/3 fixture, not every accepted synthetic list. Direct Ubuntu URI sources retain the earlier Kernel-only rewrite; this correction targets the observed mirror+file runner layout. Neither is claimed to have native acceptance outside that layout.

## Spec

PASS for publication as a mitigation candidate, no blocking findings. The smallest correction preserves the original Ubuntu entries instead of replacing them with a single-host dependency. The [APT manual](https://manpages.ubuntu.com/manpages/noble/man1/apt-transport-mirror.1.html) documents automatic fallback on failed file acquisition, lowest-number-first priorities and random equal-priority order. No signature verification, check, platform, timeout, action pin, permission or budget gate is removed. No IP address or resolver change, extra retry loop or native experiment is introduced. Before: indirect sources redirected to Kernel only. After: unchanged source URI, preferred Kernel entry with the original Ubuntu fallbacks retained. Reversible by reverting the CI-only commit; nothing affects an installed product or owner machine.

Published-head hosted acceptance remains required: helper updated1 on observed layout, signed indexes and packages fetched successfully (including fallback if Kernel is unavailable), no final signature/insecure/failed-fetch warnings, dependency installation and browser/Electron smokes pass within12min, all required checks and exact-head independent review green. No successful network recovery or speedup is claimed locally. The underlying DNS/connectivity cause remains unknown.

## Personally executed evidence

- `python -B -m unittest discover -s tools/ci -p 'test_*.py'`:29/29 pass, including23 mirror tests.
- `node --test scripts/*.test.mjs`:19/19 pass.
- Five independent actual source mutations each failed an assertion: Kernel last, mirror-list publication omitted, indirect URI returned to Kernel-only, fresh recheck removed, foreign mirror accepted. All restored byte-for-byte.
- `.github/workflows` unchanged between the review base and candidate. `git diff --check` clean.
- Read entire helper and helper-test diff, docs/evidence/progress diff; existing guard and publication code inspected. No tests or evidence credited merely from the builder's report; its nine-mutation record and five-job log comparison remain attributed to it.

Not run: real APT, sudo/root publication, hosted CI, application/native or ignored fixtures. No SCM/LSA/account/privilege/owner/VM/Lab Manager/S4U operations. Native/installed/dedicated M2 remains **unverified on real hardware**. Standards0blocking; Spec0blocking. Sol review does not waive hosted gates or grant native acceptance.
