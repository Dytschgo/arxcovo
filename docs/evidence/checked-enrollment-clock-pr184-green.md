# PR184 corrected-head hosted evidence

Reviewed head `a3cf0cd2dba3888905693e264dbacfe368bb820d`, base
`08c4b999c23f9b516b3a942039ebc3c3f1a6dd04`. First normal corrected-head CI
[44370386768](https://github.com/Dytschgo/arxcovo/actions/runs/44370386768),
attempt 1, completed SUCCESS in 627 seconds by the existing duration policy.
No PR queue credit is claimed and no duration-history main entry is fabricated.

| Job | Seconds | Rust PASS / skip | Rust body | Node tests |
| --- | ---: | ---: | ---: | ---: |
| Windows 135035286242 | 474 | 709 / 36 | 211.892 s | 48 PASS |
| Ubuntu 135035285867 | 221 | 695 / 22 | 50.280 s | 48 PASS |
| macOS 135035285535 | 320 | 663 / 8 | 69.702 s | 48 PASS |

All fourteen new pure clock tests and the new durable anchor refusal test passed
exactly once on each OS. The issuer `i6_actual_post_wait_clock` passed on Ubuntu
in 0.976 s and macOS in 2.355 s, covering its original six cases and the fifteen
new checked-failure subcases in the unchanged 45-second fixture. Windows retains
the existing source quarantine for that fixture; no Windows issuer execution
credit is claimed. The mandatory inventory guard, formatting, all-target Clippy,
UI unit/lint/build, browser and Electron smoke steps passed on all three OSes.
Windows meets the unchanged feature-job limit of 660 seconds.

The automatic optional Windows sample ran once: eight cases PASS, zero failures,
reported orphans zero, nextest exit0, sample wall30.125 s. This is one future-head
sample, not stability, repair, quarantine removal or re-enable evidence. Existing
engine/founder counters and PR182 HOLD/NOFIX remain unchanged.

Independent original source review found only the mandatory inventory omission.
The [delta attestation](https://github.com/Dytschgo/arxcovo/pull/184#issuecomment-6987847673)
closed it at this exact head: Standards PASS / Spec PASS, status59286111811
`agent-review=success`. `ci-time-budget` and all three verify checks were also
SUCCESS, with the PR OPEN/CLEAN at the captured gate read. Coordinator must still
refresh live main, its own latest exact CI and these gates immediately before
merge. Source review is not resulting-main health.

Retained D: evidence under `D:/Temp/arx-pr184-a3-first-ci/` includes exact run and
attempt-1 job metadata, required gates, `execution-proof.json`, `pr-duration.json`
and bounded per-job logs. Each download had an 8 MiB / 45-second bound and
completed untruncated:

- Windows: 258,171 bytes, SHA256
  `498abfce18f162034fc626fdcf0a947a35896f3baa98d16f6db8c0ed2c1941ac`.
- Ubuntu: 258,291 bytes, SHA256
  `cd637cf59d1464f2e693fb37309fa8f5e2f8374c182ae0868cc72d0870a7abcd`.
- macOS: 238,348 bytes, SHA256
  `d4377b410d7c13eebdb79a12e40508d30cba877cc061acd08794ad8c4b0b4d1d`.

The failed original1cd91ef6 head and first CI44368904104 remain separately
preserved. No rerun or dispatch was used. This remains an unwired checked-clock
prerequisite: healthy UTC, startup/restart/resume, real fifteen-minute expiry,
live transport, container and hardware acceptance are not established.
M2 OPEN / M3 UNCHECKED; unexercised paths are **unverified on real hardware**.
