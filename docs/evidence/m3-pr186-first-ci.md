# PR186 first hosted CI and merge

Coordinator verification, 2026-11-19. Published head
`0a323793986d705785eff7e409cc85a75cce2836`, base
`e2908c98dfcdf97cb8e0aba431bafdb51f328c95`.
[CI44431134508](https://github.com/Dytschgo/arxcovo/actions/runs/44431134508),
attempt1, completed SUCCESS. No rerun or dispatch.

The unchanged policy measured490seconds wall,484charged,6validated idle;
longest worker468seconds. All9jobs passed; Windows workers remain below660s.

| Platform | Rust passed / skipped | Rust job / test body | UI job / Node |
| --- | --- | --- | --- |
| Windows | 768 / 36 | 468s / 232.160s | 77s / 60passed |
| Ubuntu | 754 / 22 | 175s / 51.342s | 136s / 60passed |
| macOS | 722 / 8 | 251s / 72.661s | 61s / 60passed |

Each OS passed all67focused source cases exactly once, including all4real learner
histories. All UI jobs passed install/lint/test/build, browser and Electron smoke.
Aggregate gates completed in Windows15s,Ubuntu10s,macOS7s. All five required gates
were independently rechecked SUCCESS; exact-head review attestation6996145608 and
agent-review status59319264686 remain linked to the reviewed head.

The optional Windows sample reported8PASS,orphans0,31.391s on this observation.
It grants no stability, repair or re-enable credit. Existing source quarantine,
sampler, limits and engine attempt counters remain unchanged.

All9job logs are retained on D:, bounded8MiB/45s per fetch, with SHA256s/counts/
durations and actual policy measurement at `D:/Temp/arx-pr186-first-ci`. The five
earlier completed logs were reused; only the four remaining logs were fetched.
Partial snapshots are preserved separately from terminal metadata. One local
duration parser initially missed Windows CRLF endings; normalizing that parser
did not rerun any CI or change source.

Immediately before merge at00:56:01UTC the coordinator rechecked live main e290,
its latest normal firstCI44418600817 SUCCESS537wall/531charged, all5PR gates,
strict protection/admin enforcement, all7reviewed source pins and PR head/base.
Expected-head guarded squash merge completed at00:56:07UTC as
`eea7af57c944d134d50ab284f1bf9b003246073e`. The entire merged tree equals the
reviewed0a tree. PR186 is unwatched.

Resulting main's own first push CI44432988620 attempt1 later qualified SUCCESS:
560wall/556charged/4idle, WindowsRust537s, all9jobs/67focused cases eachOS PASS.
[Main evidence](m3-pr186-main-first-ci.md). Earlier pending checkpoint history
remains retained. Local unpublished docs checkpoints remain preserved.
M2 OPEN / M3 UNCHECKED; production startup/custody/admission, actual joins,
containers and real multi-host hardware remain unverified. Hardware paths remain
**unverified on real hardware**. Engine Sol3/3/historyOpus0/2 unchanged.
