# E2 current-parent integration: independent assessment and HOLD disposition advice

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent, no
delegation. Builder of the assessed integration: OpenAI / gpt-6.1-sol / high, so
this is a different-provider review of a Sol change. It is not an
architecture-independence claim over the Opus-authored ADR0016 design; that
credit stays with the prior Sol reviews. Local only: no GitHub write, push, PR,
status, watch, rerun, approval, merge or settings action.

## Pinned target (off-checkout)

| Role | Commit |
| --- | --- |
| Assessed target (docs-only over tested head) | `000835d8537a0946c86211b41634cef457f456e5` |
| Tested implementation head | `2986cad1fe425169b09917fcf6eab70292e915ce` |
| Supplied base (ledger on main) | `fbb1813fd0455f7208e84557ec18c8e9d1bc298e` |
| Main inherited by the base (after PR43) | `b3e1d13888ae5967b095d4151144aa240169ed77` |
| Reviewed E2 source and authority recheck | `1103a660222849f9a6d4ac9d4b2cdf9491703ca4` |
| Prior red current-parent port | `780ca4164494f6c3da00a340f4c4c67e3ac9d305` |
| Public main at assessment time (after PR44) | `0fbf8df0b9dbe204a196bc3422c81d12d1bec94c` |

All objects were read directly with `git show`, `git rev-parse REF:path`,
`git diff` and `git merge-tree`. The prepared checkout (`8687ae8`, the PR45
state-exit head) was used only to host this report; it was not substituted for
the target. Commits `fbb1813..000835d`: `60e0650` (docs), `2986cad` (six crate
files plus scoped docs), `000835d` (docs). Range diff: 19 files.

## Source identity and base assumptions (verified here)

- **Six crate files.** At `000835d`, `2986cad`, `1103a66` and `780ca41` the blobs
  are identical: `mod.rs` `afbecd8c…`, `owner_install_windows.rs` `20b149c3…`,
  `owner_install_windows_tests.rs` `6334aac5…`, `owner_scm.rs` `ef892614…`,
  `owner_windows.rs` `c467ed1f…`, `owner_windows_tests.rs` `d7a1b7d9…`. They match
  the evidence manifest.
- **No dropped main change.** The pre-E2 versions of `mod.rs`, `owner_scm.rs`,
  `owner_windows.rs`, `owner_windows_tests.rs` and `owner_scm_tests.rs` are the
  same blobs at the base `fbb1813`, at merged E1 `884a301` and at the prior port's
  E1 parent `b79786c`. So the E2 delta on the new parent is the reviewed delta,
  and wholesale replacement loses no E1/N3/N2 hunk.
- **Nothing else changed in product.** `git diff fbb1813 000835d` outside `docs/`
  names only the six files. `b3e1d13..fbb1813` is docs-only. The `crates` tree
  (`fc3f9ea5…`) and `Cargo.lock` are identical at `2986cad` and `000835d`, so the
  builder's checks on `2986cad` apply to the target's source bytes.
- **The failing test's source is unchanged.** `development/windows.rs`
  `85f0c0ec…`, `mod.rs` `91ed14c5…` and `types.rs` `754070e8…` are identical at
  `780ca41`, `b3e1d13`, `000835d`, `0fbf8df` and `8687ae8`. The test at
  `windows.rs:953` (`results_count_dropped_output_and_report_nonzero_exit`)
  includes the line-956 `run_inner(...).unwrap()` overflow sub-case.
- **Public main drift.** `b3e1d13..0fbf8df` (PR44) adds one `#[cfg(windows)]`
  test (13 lines) in `service/native.rs` and docs. No production source changed
  and none of the six files was touched. `git merge-tree 0fbf8df 000835d` and
  `git merge-tree 8687ae8 000835d` merge cleanly, and the six blobs are unchanged
  in the merged tree. But see finding R-1: PROGRESS merges cleanly into the wrong
  meaning.
- **Retained reports.** The six retained E2 review and evidence documents are
  byte-identical at `000835d`, `1103a66` and `780ca41`.

## Checks credited, by exact head

| Check | Head | Owner | Credit here |
| --- | --- | --- | --- |
| Full security/spec/standards review, SP-B1/ST-1 blocking, 245/2 run `b2f10f0e…`, mutations | `d28cbd7` vs E1 `4a5919e`, committed `42718df` | Opus high | historical, original head |
| Authority correction recheck PASS, 246/2, M1–M4 mutations | `3ad3c80`, retained `1103a66` | Opus high | historical, original head |
| Junction debug, implementation evidence, builder mutations | their stated heads | Sol / Opus | historical |
| Red uninstrumented run `bdbf032b-658a-4123-a279-303cb40566ed` (116 pass, 1 fail, 132 not run, 2 skip) | `780ca41` | Opus diagnostic | **open failure** |
| Three instrumented runs, 249/2 each | `780ca41` + temporary patch | Sol | diagnostic only, not a waiver |
| fmt, locked all-target Clippy, focused 54 `b7e3c2ba-480e-4ff0-ad41-112396511a15`, one uninstrumented full run 259/2 `d78206ad-1919-4183-8614-0098d379edd2` (9.908 s) | `2986cad` (same crate tree as `000835d`) | Sol builder | builder-reported. I have not seen the logs (`target/e2-integration-evidence/` is in the builder worktree) and do not re-verify them |
| Focused owner tests on a `git archive` of `000835d` (below) | `000835d` | this review | own run |

### This review's own run

From a `git archive` of `000835d` extracted into a session-owned scratch
directory, with a separate target directory:
`cargo nextest run --locked -p arxd --no-fail-fast -E 'test(service::owner_windows::) | test(service::owner_install_windows::) | test(service::owner_scm::)'`.
Result: **54 tests run: 54 passed, 206 skipped**, 0.511 s. This independently
confirms that the exact target compiles on the new parent and that the focused
fake-SCM/owned-temp owner tests pass, including the `i5` owned-temp junction
fixture through its normal parent. I kept only the tail of the output, so the
run ID was not recorded. The skip count differs from the builder's 207 because
of `-p arxd` scoping.

No full suite, instrumentation, mutation, stress, retry or direct ignored
fixture was run. No production OwnerGuard, install, SCM, LSA, account,
privilege, service, pipe, VM or Lab Manager path was run, and no private state
was read.

## Standards

0 blocking.

- Fences unchanged and verified by reading the target. `OwnerGuard { layout,
  pins }` fields stay private. Its only added methods are `pub(super)`:
  `layout(&self)`, `binary_directory(&self) -> Result<&File, OpenError>` (a shared
  borrow, `checked_sub(3)`) and `into_install_pins(self)`. `Local` keeps its
  private field and exposes only `pub(super) fn as_ptr`. `WindowsScm::new` and
  `test_calls` (test-only) are `pub(super)`. These are the SP-B1/ST-1 corrections
  as rechecked at `1103a66`.
- The carried advisories ST-2 to ST-6 and SP-A1 to SP-A5 (SP-A2's `open()` fence
  checks only `OPEN_EXISTING`, SP-A3 fence breadth, SP-A4 no runtime coverage of
  production `binary_check`, SP-A5 lock-last proved by reading only) and E1-R1 are
  unchanged and still open as advisories. Nothing new was introduced, so I add no
  new smell findings for byte-identical code.
- **R-1 (medium, docs/process; blocks any refresh that relies on Git's
  auto-merge).** `git merge-tree 0fbf8df 000835d` succeeds without conflict, but
  the merged `docs/PROGRESS.md` keeps the target's `## Current state` (last merged
  #43, "PR44 … awaits Windows"). It moves PR44's real current-state block (its
  SYSTEM-test provider and outcome row) under `## Historical checkpoint before E2
  current-parent preparation`. A mechanical refresh would leave the ledger false.
  The refresh must rewrite Current state by hand: last merged #44 at `0fbf8df`,
  the current queue, and retained PR44/PR45 rows.
- R-2 (low, docs). The target's status lines are accurate for `b3e1d13` but will
  be stale after refresh. These must name the refreshed base and PR44:
  PROGRESS ("Last merged PR #43", "PR44 … awaits Windows"), the evidence file
  ("Actual main inherited after PR43") and the "inherited on base `fbb1813`"
  sentences in ADR0016 and the brief. ADR0016 and the brief correctly keep E1
  history ("E1 hosted gates subsequently passed in PR40").

## Spec (ADR0016 decisions, E2 brief, task boundary)

0 blocking. I checked every ordering and fence the brief lists against
`owner_install_windows.rs` at `000835d`:

- The source fence test (`owner_windows_tests.rs:1043`, `function_body(..,
  "install_with")`) is present and unchanged.
- **Binary check before SCM observe.** `ns.binary_check(policy)?` is the first
  statement of `install_with`, before `scm.observe()`. The production variant runs
  `structural(f, true)` and `BinaryIntegrity` on the pinned binary directory.
  `install()` constructs `WindowsScm::system` first, which only opens a handle;
  that order was accepted in the recheck. The index is correct by reading:
  `acquire` pins `[root, Program Files, Arxcovo, arxd.exe]` and then the running
  image, so `len-3` is the `Arxcovo` directory. SP-A4 (no runtime test) still
  applies.
- **Pinned ProgramData.** `pin_chain(&parent, None)` pins `state.parent().parent()`
  with ancestor checks only and no `FILE_SHARE_WRITE` on the leaf.
- **Restore after pins, before create.** `enable()` runs after `ns.pin()` and
  before the first `directory(`.
- **Descriptor at creation, no write to existing objects, no repair.** `create`
  uses `CREATE_NEW` with the SDDL descriptor and checks the policy after
  creation. `directory` uses `CreateDirectoryW` with the descriptor, and an
  existing directory is only reopened read-only and checked. `existing` and
  `open` refuse non-read masks on `OPEN_EXISTING`. There is no ACL setter.
- **Exclusive lock before state.** `owner.lock` is created or opened with share
  0, policy-checked and required to be empty before the `state` directory.
- **Classify all before create.** The member loop classifies all three members
  (lock empty, database empty or a valid 20-byte rollback header) before the
  `missing` creation loop.
- **WAL/SHM rechecked.** `sidefiles` runs before classification and again after
  creation. Any outcome other than absent refuses.
- **Register last.** `scm.register()` is the last call, and the evidence says
  registration never starts the service.
- **Lock drops last.** `Held` declares `members`, `pins`, `owner_lock` in that
  order, so the lock drops last. This is proved by reading only (SP-A5).
- **No caller or activation.** `git grep` at `000835d` finds no use of
  `owner_install_windows`/`install` outside its module and the source-fence test.
  There is no CLI, MCP or native wiring. `#![allow(dead_code)]` documents this.
  The `pub mod`/`pub fn install` library surface is the reviewed boundary,
  reachable only through a `cfg(windows)` `OwnerGuard`.
- **Scope docs.** ADR0016 and the brief add only status. `service-install.md`
  step 4 is the reviewed delta: document-only, owner-only, agents must not run
  it, CLI pending, **unverified on real hardware**. The `service-foundation.md`
  E2 section is appended after the current content and labelled HELD. No owner
  command was executed (by the evidence, and none appears in this range's
  history). The six retained reports are byte-identical and keep their original
  head credits.

Spec gaps carried from earlier reviews, not regressions: SP-A1 ordering and
share-mode test gaps, and SP-A4. Native, installed and dedicated behaviour is
**unverified on real hardware**. M2 is incomplete.

## The unresolved `ExecutionFailed`: evidence assessment (debug phase 1 only)

What is established:

1. One uninstrumented full run at `780ca41` failed. The first overflow sub-case
   of `development::windows::tests::results_count_dropped_output_and_report_nonzero_exit`
   (`windows.rs:956:67`, `output_bytes = 100`) returned `ExecutionFailed`
   (116 pass, 1 fail, 132 not run, 2 skip). Phase, Win32 error and cause are
   **UNKNOWN**.
2. The same symptom class (`ExecutionFailed` from the real Windows development
   executor under full-workspace default concurrency) is older than the E2 code.
   It was recorded on 2026-11-11 at the pre-E2 Unix-integration preparation
   (`06c7f6b`: cancel, junction/handle-count loop, bridge positive control; run
   `d9dd5850…`). It is written down in
   `development-execution-failure-observations.md` (first committed `a1246dc`),
   and PR29 later merged to main with the failure retained as open. That source
   contains none of the six E2 files.
3. Reduced loops did not reproduce it: 124 isolated runs, three full runs at
   ≤2 processes, ten instrumented full runs, three instrumented 249/2 runs and one
   uninstrumented 259/2 run. There is no red-capable, deterministic command, so
   debugging is in phase 1. No hypothesis, cause, fix or waiver is supported. I
   add none.
4. The E2 delta touches nothing in `development/`, `executor`, `local*` or
   `native.rs`. Its only runtime interaction with that test is being in the same
   nextest run: in-process tests on owned temp, plus two short `cmd.exe /c mklink
   /J` children in `i5`. The base already has comparable `cmd` spawns in
   `owner_windows_tests.rs`. This does not exonerate E2. It means no evidence ties
   the symptom to E2, and the symptom is recorded on a source without E2.

What the green run does and does not show: one green run shows that the
integrated tree compiles, passes Clippy and can pass the whole suite on this
host. The recurrence rate is unknown and low enough that dozens of runs have
not reproduced it, so one pass says almost nothing about the rate. It does not refute "E2
increases the rate", and it does not close the red. The prior red stays a
recorded, open failure.

## HOLD disposition advice

**Recommendation: conditional release of the HOLD for publication as a
candidate only.** Publish this exact source as a draft/candidate PR so it can get
mandatory independent hosted verification, after the refresh conditions below.
The intermittent `ExecutionFailed` stays an explicitly open issue linked from the
PR, ADR0016 and PROGRESS. No merge disposition is implied. Only the coordinator
decides publication and merge.

Technical reasons publication-for-verification is defensible:

- The source identity is exact. It is the reviewed bytes, with no divergence and
  no lost main hunk. The reviewed security properties (above) are unchanged.
- The open failure is in code E2 does not modify. The same symptom is recorded on
  a pre-E2 tree on the main lineage, and an earlier slice merged with it retained
  as open. Holding E2 locally does not lower the risk on main, because the
  symptom can occur on main without E2.
- Local reruns cannot settle this, and further blind local reruns are
  forbidden. Hosted Windows CI on the required gates is the independent signal
  still missing, and it can only be obtained by publishing.
- Publishing is a two-way door: a draft PR can be closed with no effect on main.
  Merging is the one-way-ish step, and it stays behind every gate.

Conditions (all required, none relaxed):

1. **Refresh and re-pin onto current main** (`0fbf8df`, or whatever main is
   when published). Before publication, show that the six crate blobs still equal
   the manifest and that `development/*` is unchanged. Rewrite PROGRESS Current
   state by hand (R-1, R-2). Do not accept the clean auto-merge as correct.
2. **Exact-head independent status.** A different-provider reviewer confirms the
   published head's tree. If the refresh changes only ancestry and docs, a
   source-identity recheck is enough. Any source change needs a fresh review.
   `agent-review` is posted only for that exact head.
3. **All required hosted gates on that exact head:** CI on Windows, macOS and
   Ubuntu (fmt, Clippy, nextest, UI, Playwright), `ci-time-budget` and
   `agent-review`. Nothing skipped, filtered, labelled away or given a longer
   timeout.
4. **Any hosted failure keeps E2 held.** No blind rerun or retry-until-green. A
   recurrence of `ExecutionFailed` in hosted CI is a natural recurrence to capture
   as evidence (logs and run ID retained) and to feed into phase 1. It is not
   reinterpreted as infrastructure noise, and a later green on another run does
   not cancel it.
5. **Disclosure in the PR body.** Run `bdbf032b…` at `780ca41` stays an open,
   unexplained failure. The single green `d78206ad…` and the three instrumented
   passes are neither a fix nor a waiver. No cause is claimed fixed. Native,
   installed and dedicated paths are **unverified on real hardware**. M2 is
   incomplete.
6. **Merge needs a separate coordinator decision**, made after conditions 1–5
   pass, that explicitly decides how to handle the open intermittent issue. The
   precedent is PR29, which merged with the symptom recorded as open. That
   decision is not made here.

If the coordinator prefers to keep E2 fully held, the defensible reason is
policy, not technical evidence against E2. Holding means the intermittent stays
unresolved anyway, because it lives outside E2, and E2 cannot get hosted
evidence. The way out of the hold would then be a reliable red loop for the
development executor, which is separate work with no E2 dependency.

## Summary

Standards: 0 blocking, 1 medium (R-1: a clean auto-merge leaves PROGRESS with a
false meaning), 1 low (R-2: stale status after refresh); carried advisories
unchanged. Worst: R-1.
Spec: 0 blocking; every ordering and fence requirement verified on the target;
SP-A1/SP-A4 gaps carried. Worst: SP-A4, carried.
Disposition advice: conditional publication as a candidate for hosted
verification under conditions 1–6; the intermittent stays open; merge not
recommended here.
