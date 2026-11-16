# C1: Windows Playwright install timing probe

Sol implementation of the independently Sol-reviewed C1 candidate from Opus2e3a388,2026-11-16. The reference PR153 Windows job133123407634 took598s; its Playwright install step took151s. Opus inspected pinned playwright-core1.65.0 installDependenciesWindows: --with-deps runs install_media_pack.ps1 and requests Server-Media-Foundation on Server SKU. The feature/download split is unmeasured, so this is a probe, not a claimed speed improvement.

Keep --with-deps on Ubuntu/macOS. On Windows install the identical Chromium headless shell without that flag. All browser and Electron smokes remain required; no dependency, browser channel, timeout, caching, quarantine or other CI step changes. The workflow contract test pins both platform branches and required smokes.

Measure the PR and next-main Windows install/job duration against the598s/151s reference. At least100s job savings on each supports the later sampling budget. Under60s abandons this candidate;60–100s needs a concrete evidence decision. Do not rerun a head to obtain savings or broaden optimization. No sampler implementation until ownership/outcome/time-budget contract review is closed. Every normal PR gate and exact-main green prerequisite remains required.

Local verification and hosted timing results are appended at their actual checkpoints. No Windows feature provisioning is performed locally. M2 remains open; M3 unchecked, Windows three-node tests quarantined and **unverified on real hardware**.

## Local validation and independent review

Sol coordinator: node --test scripts/ci-workflow.test.mjs passes3/3, diff check passes. Independent Opus high review01c0761 repeats3/3 and diff check and verifies pinned Playwright1.65.0 source: --with-deps adds Windows Server-Media-Foundation setup, but the installed Chromium shell is identical. UI/Electron smokes stay required in the actual workflow; neither validates media playback. The added static smoke-presence assertions do not by themselves prove a future workflow cannot disable a smoke, an admitted advisory.

Use both measured Windows baselines: reference PR153 job598s/install151s and latest main58d06c1 CI43815774741 job542s/install139s. Compare the install step against151/139 and the job against598/542, reporting both deltas rather than attributing56s baseline jitter to this change. Source review PASS for publication only; actual hosted savings and smokes remain pending.

## Hosted PR154 timing: mixed whole-job criterion

Exact head5ba2cf6 run43818081980 Windowsjob133134613345 succeeded06:38:03Z–06:46:16Z =493s. Chromium install06:45:54–06:46:01 =7s; browser smoke4s and Electron smoke7s both success. Nextest step06:40:19–06:45:23 =304s. Allfive PRgates pass and latestexactmain58d06c1 CI43815774741 remainsgreen, freshlychecked.

Install savings are144s versus151s and132s versus139s. Wholejob savings are105s versus598s but49s versus542s. The latter is below the declared60s abandon threshold, so merge and sampler implementation remain held pending an independent evidence disposition. Do not present component savings as meeting the whole-job100s criterion. No manual rerun or new optimization is performed. The7s install and unchangedrequiredsmokes are observed; hosted media support is not claimed.

First postmerge main9074a85 CI43819925277 failed Windows nextest before the changed install: S2b latched readiness gate, unrelated source path untouched by C1. Windows install and smokes skipped, so no postmerge install or fulljob saving is measured. Ubuntu/macOS passed. Sampler held by R2 failure; R3 install>79s was not observed and no C1 revert is inferred. No rerun to replace this first-run evidence.
