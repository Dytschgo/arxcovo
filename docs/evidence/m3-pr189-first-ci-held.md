# PR189 first CI held

Published head `d4c89fd043d035d805cdbb21faab491f6214ef86`, source
`92eade4fbf866709aabb464bb0e8facb2f4afe11`, base
`8bd12b7f4954f4622b4487fad054e4a32e5d551a`.
Normal CI44456026784 attempt1 failed after306s wall. Independent source review
Standards0/Spec0 and exact-head agent-review59330212021 remain valid source
evidence, but required CI failed and merge is blocked.

| Job | Seconds | Observed result |
| --- | --- | --- |
| Windows Rust | 296 | 48 passed, 2 failed, 36 skipped, 747 unrun; body16.544s |
| Ubuntu Rust | 200 | 784 passed, 22 skipped; body52.684s |
| macOS Rust | 165 | 752 passed, 8 skipped; body46.848s |
| Ubuntu UI | 48 | Node60, browser and Electron passed |
| Windows UI | 66 | Node60, browser and Electron passed |
| macOS UI | 56 | Node60, browser and Electron passed |
| macOS / Ubuntu / Windows verify | 2 / 3 / 4 | All failed closed |

Windows job135331121754 failed
`cluster::consensus::anchored_join_tests::anchored_preproof_single_voter`
(anchored_join_tests.rs:199) and
`cluster::consensus::issue::found::found_tests::f10_drop_bounded`
(found_tests.rs:568). Both report original founder refusal: founder unavailable.
Retained traces reach finaldeadline with pending proposal. Their underlying cause
remains unknown. These test files are unchanged by the candidate; that fact alone
does not establish failure origin, independence, stability or a harmless flake.
The run executed real test steps; zero-step runner-acquisition recovery does not
apply. This is neither a job timeout nor a runner allocation failure.

All seven new handoff tests and all24 selected startup component cases passed
exactly once on each Unix OS. Reviewed Unix branches require actual mutations;
their execution qualifies these component assertions. Windows reached none of
the new cases before nextest stopped; previous23 local Windows passes remain
separate evidence. No optional Windows diagnostic sample executed after failure.

Nine raw logs were downloaded once under independent8MiB/45s per-job bounds;
all downloads completed without truncation. Metadata, SHA256/size manifests,
exact passing names, failure summaries, parser and timing calculation are retained
at `D:/Temp/arx-pr189-ci`. Policy computed300charged/6potentialidle and withheld
credit as not-green. No history entry was appended and no run was retried or
dispatched.

PR189 HOLD. Fresh independent Astra6.2/high evidence-only disposition completed
under the standing unknown-failure rule, report/pins
`D:/Temp/arx-pr189-astra-disposition`. Standards0/Spec0, cause UNKNOWN, report
SHA256 `72c0ba684949f5c51aabc5de0b37c7eb4b8256cf054f20348ef4633a10769605`.
Coordinator verified32 retained input hashes, all10 integrated blobs, unchanged
failure-path blobs and report identity. Both failing calls bypass the new offline
handoff. The anchored post-shutdown cut records EngineReply Ok and OwnerSend
Pending, but neither caller receipt nor success/commit/apply timing relative to
the deadline follows. F10 has no equivalent later observation. Scope boundaries
and the base's prior passes do not establish this failure's origin.

No further founder diagnostic or fix,
reset, bypass, timeout, quarantine, settings, SDK or capacity change follows.
Engine Sol3/3 exhausted / historical Opus0/2 unchanged. M2 OPEN / M3 UNCHECKED;
operational startup remains unavailable and **unverified on real hardware**.
