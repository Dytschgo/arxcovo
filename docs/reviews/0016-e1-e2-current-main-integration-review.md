# ADR0016 owner E1/E2 current-main integration review

Reviewer: Anthropic / Claude Opus 5.5 / high. Independent review sub-agent, no
subagents, 2026-11-12. Single-agent review on two separate axes (Standards,
Spec) per concern. This is a review of an integration port. It is not a new
full review of the roughly 2000 unchanged lines. ADR0016 is Opus-authored, so
this report claims no independent architecture credit. Independent source
credit stays with the retained Sol reports listed below.

Read-only scope: no Cargo, native, process or test runs (an other-provider
diagnosis was running concurrently), no code or PROGRESS edits, no GitHub
writes. Production constructors, SCM, LSA, privileges, accounts, system paths,
owner admin, services, VMs, Lab Manager, S4U, ignored fixtures and private
state were not touched. This report is the only file committed.

## Pins

| Item | SHA |
| --- | --- |
| Actual main (#39) | `79b7879e7330e2f8de408723f15ddfac399dd75f` (local `main` ref is stale; `origin/main` was used) |
| E1 refreshed boundary | `b79786c27ad231578325e7cd72399e1941fa4eaa` (merge `8448b6b`) |
| E1 service bytes | `c4c010afac0fae085fa3f870f86a00b614c6a477` |
| E1 originally reviewed source | `4a5919e61542ee38507a9ba461549a61e4b3a67d` |
| E2 current code | `6e86285585c3a284cc59b0d7bf5becebbcf21117` |
| E2 prepared head (code plus docs) | `780ca4164494f6c3da00a340f4c4c67e3ac9d305` |
| E2 service bytes | `c73e5c7d2cc7df1f6084c25ebccb6f4434bca245` |
| E2 originally reviewed source (includes authority fix `3ad3c80`) | `1103a660222849f9a6d4ac9d4b2cdf9491703ca4` |
| Client source `local_windows.rs` | blob `b685e90` at main, `bd22e05`, E1 and E2 |

Concern diffs reviewed: E1 = `79b7879..b79786c`. E2 = `b79786c..780ca41`, and
within it `b79786c..6e86285` (code) plus `6e86285..780ca41` (docs only:
PROGRESS and the refresh evidence). Ancestry holds: 79b7879 is an ancestor of
b79786c, and b79786c is an ancestor of 780ca41.

## Equivalence evidence (git object checks)

- The `crates/arxd/src/service` tree is `802acd7` at 4a5919e, c4c010a and
  b79786c. It is `c28059e` at 1103a66, c73e5c7, 6e86285 and 780ca41. Every
  owner/SCM file is therefore byte-identical to its originally reviewed pin.
- Outside `crates/arxd/src/service` and `docs/`, `git diff 79b7879 b79786c` and
  `git diff 79b7879 780ca41` are both empty. `Cargo.lock`,
  `crates/arxd/Cargo.toml`, `.github/`, `development/windows.rs`,
  `local_windows.rs` and `local_execution_tests.rs` equal main.
- The service-base drift since the original pins is nil. `executor.rs`,
  `application.rs` and `service/mod.rs` at 4a5919e equal main. The only crate
  delta between the reviewed sources and the integrated heads is main's own
  `local_windows.rs` (#39) and main's `local_execution_tests.rs`. No owner,
  SCM or install code references either file.
- E2's only change to another file in the module tree is the declaration
  `#[cfg(windows)] pub mod owner_install_windows;` in `service/mod.rs`.
  `native::rollback_database_header` is already `pub(super)` at main 79b7879.
- `git diff --check` is clean for both concerns. Every changed docs file
  decodes as UTF-8. Every relative Markdown link in the changed docs resolves.
  The one exception is the `t3-thread://` babysitter link, which predates both
  concerns: it is already on main and was introduced in 176af7a.

## Consequential security spot checks on integration assumptions

- **No new wiring.** Outside the defining modules, `git grep` finds no
  references to `OwnerGuard`, `WindowsOwnerSession`, `owner_install_windows`,
  `uninstall_flow`, `into_install_pins` or `binary_directory()`. Nothing in
  `main.rs`, `mcp`, `exec` or `local*` references `owner_windows`,
  `owner_scm` or `owner_install`. `owner_install_windows` carries
  `#![allow(dead_code)]`, and `uninstall_flow` is
  `cfg_attr(not(test), allow(dead_code))`. There is no MCP, CLI,
  `Plan::execute` or native-backend wiring.
- **Library visibility.** `service` is `pub` in `lib.rs`. As a result,
  `OwnerGuard::acquire`, `WindowsOwnerSession::{open, uninstall, ...}` and
  `owner_install_windows::install` are public library API, although no
  caller exists. The inner store, right, `Local`, policy and pin helpers stay
  private or `pub(super)`. This matches the docs' "library only" claim and
  is inherited from the reviewed source.
- **Proof.** `Deregistered(())` is `#[must_use]`, is not `Clone` or `Copy`,
  has a private constructor that only `deregister()` uses, and is consumed by
  value in `uninstall`. The test helper `fixture_deregistered` is
  `#[cfg(test)]`.
- **Privilege, pin and lock ordering** (inherited unchanged).
  - `acquire` pins only the binary chain and the image. It never enables a
    privilege or opens a store.
  - `open` and `uninstall` pin the private directories first, then call
    `enable_restore`, then require the existing `owner.lock`.
  - `install` runs in this order: binary-directory check, then SCM observe,
    then namespace pins, then `enable` immediately before directory creation,
    then `owner.lock` (share 0) before state, then classification of all
    members before any creation, then WAL/SHM refusal and recheck, then
    `register()` last.
  - `WindowsScm::system` and `new` are pure constructors and issue no SCM
    call before `observe()`.
  - `Held` declares its fields in the order members, then pins, then
    owner_lock, so the lock is released last.
  - Reparse points are rejected because `ProtectionPolicy::check` calls
    `structural` first. Directory and lock handles omit `FILE_SHARE_DELETE`.
- **Positional coupling (advisory, inherited).**
  `OwnerGuard::binary_directory()` takes `pins[len-3]`, which depends on
  `acquire` pushing exactly the image chain followed by the image. This was
  reviewed before and is fenced by tests. It is noted only as a fragility.

## Declared verification limits

- Development-host and hardware limits are declared consistently. Only owned
  temporary, fake or synthetic paths ran. Production constructors, the fixed
  namespace, real SCM, LSA and restore, SYSTEM creation, known folders and
  binary ACLs are **unverified on real hardware**. No M2 acceptance.
- The local evidence for E1 refreshed boundary b79786c is 41/41 focused tests
  (run `23277874-8835-4ddd-961f-34a4fd36e376`). No full workspace run is
  recorded at b79786c.
- The local evidence for E2 head 6e86285/780ca41 is red. fmt and locked
  Clippy passed. The one full nextest invocation, run
  `bdbf032b-658a-4123-a279-303cb40566ed`, **FAILED**: 116 passed, 1 failed,
  132 not run, 2 skipped. The failing test is
  `development::windows::tests::results_count_dropped_output_and_report_nonzero_exit`,
  `windows.rs:956`, `Err(ExecutionFailed)`. It was not retried. The earlier
  247-pass result belongs to c73e5c7 on 0937cac. **It is not treated as a
  full pass for this head.**
- Diagnostic facts (no cause claimed):
  - The failing test and its code under test are byte-identical across main,
    E1, E2 and the original pins.
  - Main's own PROGRESS already records the unrelated ExecutionFailed
    intermittency as unresolved.
  - E1 adds one test-only `cmd /c mklink /J` child process
    (`owner_windows_tests.rs:702`) to the same test binary. E2 adds another
    (`owner_install_windows_tests.rs:659`).
  - `run_inner` creates inheritable NUL and pipe handles and calls
    `CreateProcessW(bInheritHandles=1)` with a handle list.
  - Concurrent test-process spawning is therefore a candidate interaction
    surface for the diagnosis to confirm or exclude. It is **not** a finding
    of cause.

## E1: one concern against main 79b7879

### Standards

Verdict: **PASS with advisories** (no blocking findings).

- S-E1-1 (smell, judgement call: Duplicated Code). `WindowsOwnerSession::open`
  and `::uninstall` repeat the same eight-line prologue: destructure the
  guard, call `pin_private_dirs`, call `enable_restore`, open the lock, then
  `with_lock`. A shared private helper would keep the ordering fence in one
  place. The S1 order fences in the tests currently cover both copies.
  Non-blocking.
- S-E1-2 (docs staleness). The ADR0016 status, the brief, and the
  checkpoints in `service-foundation.md` and `service-install.md` say "this
  port onto main 0937cac". The integrated base is 79b7879, which
  `docs/evidence/windows-owner-current-main-refresh.md` explains.
  Non-blocking. Refresh the wording at publication if convenient.
- S-E1-3 (docs staleness). The E1 head's PROGRESS Current line says "New
  main39 CI43182044209 is pending, not claimed green". That run has since
  succeeded: 559s, 08:19:51–08:29:10 UTC. E2's head corrects the line. If E1
  publishes standalone, refresh it, or accept it as truthful at the time it
  was written.
- PROGRESS retention: `git diff 79b7879 780ca41 -- docs/PROGRESS.md` deletes
  0 lines. Main's #39 rows and the table counters are retained under dated
  historical headings.

### Spec (ADR0016 E1, briefs/windows-install-provisioning.md)

Verdict: **PASS**.

- Implemented as specified: a binary-only private `OwnerGuard`; `open(guard)`
  pins private chains before privilege; session wrappers (`read_config`,
  `observe_batch`, `configure`) replace raw `parts()`; `WindowsOwnerStore`
  and `WindowsBatchRight` are private; uninstall is gated by the linear
  `Deregistered` proof, and `uninstall_flow` reaches no store phase without
  that proof.
- No behaviour outside the spec: no CLI, MCP, `Plan::execute` or activation.
- The docs say library only and no CLI, and they state that production and
  native-constructor operation is **unverified on real hardware**.

### E1 publication readiness

Ready for publication as one PR from this integration-review standpoint. The
source is identical to the reviewed 4a5919e, focused evidence passed on the
identical boundary, and there are no new base assumptions. Hosted required
gates, including the hosted Windows full suite, remain mandatory. They will be
the first full-suite signal at b79786c. Merge also needs an exact-head
independent review on the published PR plus green checks. This is not merge
approval and not hardware or M2 acceptance.

## E2: one concern against E1 b79786c

### Standards

Verdict: **PASS with advisories** (no blocking source findings).

- S-E2-1. The same "port onto 0937cac" wording appears in the ADR, the
  brief and `service-foundation.md` (see S-E1-2).
- S-E2-2. PROGRESS now has two identical headings, "Historical checkpoint
  before current-main M2 port — 2026-11-12", which collide as anchors.
  Cosmetic.
- S-E2-3 (one-concern hygiene, judgement call). The E2 head's PROGRESS also
  records the ADR0018/0019 Sol design-review disposition (report 6a6faae).
  This is acceptable as a shared ledger entry, but it is outside the E2
  concern.
- S-E2-4. `service-install.md` step 4 leaves a double blank line before the
  future-invocation code fence. Cosmetic.
- The failed-verification docs are accurate. The refresh evidence and
  PROGRESS report the red run with exact counts and run ID, say there was no
  retry, attribute the prior 247-pass result to c73e5c7 on 0937cac, and claim
  no cause.

### Spec (ADR0016 E2)

Verdict: **PASS for source correctness.** The source is byte-identical to
1103a66, which passed the authority recheck that resolved SP-B1 and ST-1. All
integration assumptions above hold.

- The docs say library only, no CLI, no account provisioning and no
  activation.
- The `service-install.md` icacls recipe is marked owner-only, document-only
  and not executed by agents.
- Native-constructor operation is **unverified on real hardware**.

### E2 publication readiness

**HELD (red).** The only full workspace run at this head failed, and no waiver
is granted. Publication waits for the ExecutionFailed diagnosis and then a
green full verification at the exact head, plus hosted gates. Whether this
integration contributes to the failure is undetermined. The source-correctness
verdict above stands separately and does not lift the hold.

## Summary

- **E1:** Standards has 3 advisories; the worst is the duplicated prologue
  between `open` and `uninstall`. Spec has 0 findings. E1 is ready to
  publish, pending hosted gates.
- **E2:** Standards has 4 advisories; the worst is the stale "0937cac" base
  wording. Spec has 0 source findings. E2 publication is held on the red full
  run `bdbf032b`.
