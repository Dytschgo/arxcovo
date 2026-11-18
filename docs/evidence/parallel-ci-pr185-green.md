# PR185 corrected source: first attempt qualified, then merged

Reviewed head `7141c1dc81315c6aaf688343741c155d2282f387`, normal first
[CI44396900780](https://github.com/Dytschgo/arxcovo/actions/runs/44396900780)
attempt1 completed/success. **627 seconds wall / 622 charged / 5 validated idle
queue seconds; longest job607 seconds.** Actual prospective exact-nine policy
validation returned applied. This qualifies the reviewed budget, not a charged or
execution speed reduction against the 606-wall/603-charged/602-longest base.

| Phase | Windows | Ubuntu | macOS |
| --- | ---: | ---: | ---: |
| Rust job seconds | 607 | 169 | 276 |
| Rust passed / skipped | 709 / 36 | 695 / 22 | 663 / 8 |
| Rust test body seconds | 255.841 | 52.572 | 72.019 |
| UI job seconds | 77 | 56 | 47 |
| Node script tests passed | 60 | 60 | 60 |
| Verify gate seconds, all Ubuntu runners | 4 | 5 | 10 |

All nine jobs succeeded. Node/UI/browser/Electron passed each OS. Existing14pure
clock cases and anchor case passed once per OS; issuer i6 passed Ubuntu/macOS,
with existing Windows source quarantine unchanged. The automatic Windows sample
reported8PASS/orphans0/33.188s. No stability, repair, re-enable, native or exhausted-
attempt credit follows. Every worker stayed below12minutes; Windows workers met
the660-second feature limit. Gates were created at last worker completion.

All9 job logs, metadata, counts and timestamps are retained at
`D:/Temp/arx-ci-split-7141`, bounded to8MiB/45s each; all fetches exited zero with
no truncation. all-log-manifest.json SHA256:
`77fa4a06e9c8cc36c38b676d389d265e9c4ea31998adc93a2a1f0e2421bd856b`.
execution-proof.json and policy-measurement.json contain the actual qualified
measurement. The earlier949/null HOLD and b0 lint failure remain unchanged.

The early watch notifications counting only1 or2 required statuses were not
accepted as a merge decision: actual strict protection still required all five,
and GitHub remained BLOCKED while Windows Rust and the verify gates were pending.

Fresh premerge proof at2026-11-18T21:13:59.558Z checked exact7141/baseff599a24,
OPEN/non-draft/MERGEABLE/CLEAN, all five required contexts SUCCESS, unchanged
strict protection/enforced admins, independent attestation6991559983, and live
mainff599a24's latestCI44379630740 first attempt SUCCESS606wall/603charged.
Main ref was separately reread immediately before guarded coordinator merge.

Coordinator squash merge produced **e11b0767f1d108313824a5fd2600516561d70391**.
Remote main and merged PR state were independently verified; the entire merged
tree exactly equals reviewed7141. PR185 is unwatched. No rerun/dispatch, bypass,
counter reset or application-source repair occurred. Resulting exact-main first
CI44399330997 is pending; PR182 remains held until it passes and qualifies.

M2 OPEN/M3 UNCHECKED. Hardware remains **unverified on real hardware**. OAuth,
owner setup, Lab Manager/VM/user data and the existing babysitter are untouched.
