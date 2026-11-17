# ADR0025 activation evidence

Root Sol coordinator, 2026-11-17. Owner-authorized pinned PR163 bootstrap
merged reviewed22a09eae as10aa891f. Original five checks and protection fences
restored and read back; reviewed tree equality passed. Approval consumed.

Main CI44177880021 attempt1 succeeded on all three OSes:632s wall,629s charged,
3s verified idle queue credit, longest job628s. Automatic trusted policy44179754154
succeeded. Root pure policy.mjs measurement matched, without invoking the live
status publisher or history recorder. No history edit or further exception.
The first evidence-writing wrapper used the wrong artifact directory and stopped;
this corrected record hashes the executor bootstrap subdirectory artifacts.
M2 open/M3 unchecked; Windows fixture coverage unverified, hardware
**unverified on real hardware**.

Artifacts under D:/Temp/arx-ci-queue-root-refresh:

- owner-authorization.json: b17d1e4f0463ff08a0204b305d6e8945c42b6edf4983b68ab32638ab840c8a10
- bootstrap/protection-before.json: 41e2ed73ab5516bb5df0c7cfd409b9e41000bf4158e7aa6639fd3bac6c78ef41
- bootstrap/protection-after.json: 41e2ed73ab5516bb5df0c7cfd409b9e41000bf4158e7aa6639fd3bac6c78ef41
- bootstrap/merge-readback.json: fd7155be1d4ec05f02a679e159f15c23f038b535aae53b872ad1466736100e0f
- main-first.json: 8137d0aff8326c691ba7c4141a6225f50011f2a6f390ecb94100ed5ecf8418bb
- main-jobs.json: cf8d98969cc59e453c3a698d4fee179962beb568b1c19c7777c2e1ca96668a41
- main-charged-measurement.json: faf7aef4fcaa71062d80c953a5e875400939d9c50c1403fc28576ee8715dbd99
- trusted-policy.log: bdb48c00ce5719a51f3b13c44f20b5bf09db5e205570adae05b97a445cc90eff
