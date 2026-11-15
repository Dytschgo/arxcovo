# A4 hosted baseline feedback

2026-11-15. Coordinator OpenAI gpt-6.1-sol medium. Evidence collection only;
independent assessment pending. No further run is authorized.

PR109 merged as `a328060000b8cb7bb389d39f8785f96f6bcb0c25` at02:05:45Z
after independent Muse exact-head PASS and all five gates. Its tree
`48b92d548fb6ef5373f7f477ac8bda2f0cb04717` equals reviewed `bcff99c`.
Before dispatch the coordinator checked its direct parent is accepted baseline
`8e4bf824`, all113 baseline source/assets match, and only the six allowed
non-doc paths differ. Driver is the one TEST change to A4; the workflow
builds/compares the fixed helper without privilege before its existing driver.
GitHub main/workflow/driver and actor were read back; the variable was absent
and no prior dispatch existed for this candidate.

One accepted dispatch consumed D7e. [Fixed record](linux-a4-baseline-run-43666413693.json):
run43666413693 attempt1, job132666674807, workflow_dispatch/main, actor and
triggering actor Dytschgo, exact source/workflow SHA equals the candidate.
Job02:09:20Z-02:10:33Z,73seconds, success; every step succeeded, including
helper compile/compare and guard/stage/run/finally-clean. One fixed-schema
feedback object was extracted from bounded job logs; no raw harness output,
helper bytes, identity, token or private payload is retained.

Feedback first/final are null, mutation lower bound cgroup, cleanup confirmed;
service/fixture-check/harness absent, carried/refusal null. The existing pure
record validator accepts it. The background finally operation deleted the
review variable; a separate API read independently returned HTTP404.

The source join needs independent assessment: exact TEST selects only the
ignored A4 fixture; its fixed helper hash/size and Standard profile, exclusive
bounded seeder, one-task placement, more-than-six-second margin, one real setup,
S7-only success kill, owned SIGKILL reap and postconditions/READY hold; driver
requires exact test summary and real systemd success before confirmed cleanup.
No component was rerun locally for this note.

This is positive baseline feedback, not completed A4 acceptance. Native
omitted-kill remains **unrun and NOT granted**. A4, A7-A12, full Stage F,
Stage G, real-Runner PA6 and M2 remain open. Installed/dedicated paths are
**unverified on real hardware**. Historical failure causes are unchanged.
D7e is consumed regardless of assessment; no retry or further native authority.

## Candidate source pins

| Path | Git blob at candidate |
| --- | --- |
| `tools/unix-exec-validation/driver.py` | `eb085b3e416f9bd8ca08c87c95c84fcd27cafd18` |
| `.github/workflows/unix-execution-experiment.yml` | `dbcca2276307357301719ed68a09631b8ab7a10a` |
| `tools/unix-exec-validation/a4-helper/helper.rs` | `5a02cf43d8abaa29e221be4ac54221e2ae05354f` |
| `tools/unix-exec-validation/a4-helper/build.py` | `5412b0cf345d3bd5dd0106e972a6b10e12827b7b` |
| `tools/unix-exec-validation/a4-helper/linux-image-pin.json` | `43e23c1b8597eead804e78cdf69c58139a788bc0` |
| `crates/arxd/src/exec/disposable/a4.rs` | `dfa36d53f0176a8c88b52303a44502ab7f4c82b3` |
| `crates/arxd/src/exec/hierarchy/a4.rs` | `64e1ddb679ed3517e776735933dc59b2442256d6` |
| `crates/arxd/src/exec/hierarchy/linux.rs` | `4ad78a672679226fc11c7899a6ef6b033f833a7d` |
| `packaging/linux/arxd.service` | `0b337773c60e49fe84de76db018d2235ae076254` |
| `packaging/linux/50-arxcovo-standard-rights.conf` | `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74` |
