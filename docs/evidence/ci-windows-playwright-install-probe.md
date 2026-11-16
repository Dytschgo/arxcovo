# C1: Windows Playwright install timing probe

Sol implementation of the independently Sol-reviewed C1 candidate from Opus2e3a388,2026-11-16. The reference PR153 Windows job133123407634 took598s; its Playwright install step took151s. Opus inspected pinned playwright-core1.65.0 installDependenciesWindows: --with-deps runs install_media_pack.ps1 and requests Server-Media-Foundation on Server SKU. The feature/download split is unmeasured, so this is a probe, not a claimed speed improvement.

Keep --with-deps on Ubuntu/macOS. On Windows install the identical Chromium headless shell without that flag. All browser and Electron smokes remain required; no dependency, browser channel, timeout, caching, quarantine or other CI step changes. The workflow contract test pins both platform branches and required smokes.

Measure the PR and next-main Windows install/job duration against the598s/151s reference. At least100s job savings on each supports the later sampling budget. Under60s abandons this candidate;60–100s needs a concrete evidence decision. Do not rerun a head to obtain savings or broaden optimization. No sampler implementation until ownership/outcome/time-budget contract review is closed. Every normal PR gate and exact-main green prerequisite remains required.

Local verification and hosted timing results are appended at their actual checkpoints. No Windows feature provisioning is performed locally. M2 remains open; M3 unchecked, Windows three-node tests quarantined and **unverified on real hardware**.
