# PR187 first hosted CI and merge

2026-11-19, coordinator Sol6.1/high. Published head
`9a48b47452cc30fdb9acf0354525c21af1f81ac3`, base
`eea7af57c944d134d50ab284f1bf9b003246073e`.
[CI44442166258](https://github.com/Dytschgo/arxcovo/actions/runs/44442166258),
attempt1 SUCCESS:580wall/576charged/4validated idle. No rerun/dispatch.
All9jobs PASS; longest Windows Rust567s remains below660s.

| Platform | Rust passed / skipped | Rust job / body | UI job / Node |
| --- | --- | --- | --- |
| Windows | 788 / 36 | 567s / 210.143s | 69s / 60passed |
| Ubuntu | 775 / 22 | 167s / 46.836s | 55s / 60passed |
| macOS | 743 / 8 | 309s / 66.559s | 59s / 60passed |

All20Windows and21eachUnix new offline-preparation cases passed exactly once.
Unix includes actual three-stage identical-byte replacement/refusal and
symlink/hardlink cases, as required by their cfg-specific assertions. Local
Windows evidence remains OS-denied replacement only. The previous67 focused RPC
cases also passed once each OS. All UI install/lint/Node/build/browser/Electron
phases passed. Aggregate verify gates: Windows4s,Ubuntu7s,macOS7s.

Optional Windows sample reported all_pass/orphans0/31.516s. This grants no
stability, repair, re-enable or native qualification. Existing exhausted engine
Sol3/3 and historical Opus0/2 remain unchanged.

All9bounded logs, SHA256 manifest, exact-name counts and actual unchanged-policy
measurement are retained at `D:/Temp/arx-pr187-first-ci`. Fetches capped8MiB/45s,
none truncated. Five earlier completed logs were reused; only four remaining
logs fetched after terminal completion. Earlier partial snapshots are preserved.

Independent final source/delta Sol6.1/high review PASS, Standards0/Spec0;
[attestation6997578360](https://github.com/Dytschgo/arxcovo/pull/187#issuecomment-6997578360)
and exact-head agent-review SUCCESS59324182918 verified. Separate independent
four-path carried-evidence review PASS, Standards0/Spec0, corroborated61 retained
inputs,18 older logs, policy arithmetic and unchanged prior history bytes. Reports
and pins remain at `D:/Temp/arx-pr187-final-review` and
`D:/Temp/arx-pr187-carried-evidence-review`.

At02:21:57.884UTC, immediately before merge, all5required exact-head gates,
strict protection/admin enforcement, six source pins, published head/base and
live main eea/latest first push CI44432988620 SUCCESS were rechecked. Guarded
squash merge completed as `9b4ccbde4362249fe06a775643c3bc8b6369c3c8`.
Entire merged tree equals reviewed9a48. PR187 is unwatched. Own main first push
CI44443788836 attempt1 later qualified SUCCESS585wall/580charged/5idle, all9jobs
and20/21/21newcasesonce. [Main evidence](m3-pr187-main-first-ci.md).

This accepts the offline component, not live startup/custody/admission or
container/hardware acceptance. M2 OPEN / M3 UNCHECKED;
**unverified on real hardware**. OAuth, owner host setup and babysitter untouched.
