# M3 R5 Windows quarantine restoration: implementation contract

Opus 5.5 high architecture/security brief. Sol implements; Opus reviews (different provider).
This is a narrow quarantine *exception* to restore green main. It is **not a fix**. Attempt
counts are unchanged: Sol shared fixes 3/3 exhausted, Opus fixes 0/2 (none spent here).

## Source base and identity

- Exact base: main `c6121523417089bc0afaf307470183dd4a7f47ba`, read with
  `git show c612152:<path>`, not from the policy branch.
- This brief's branch also carries 16 unmerged policy commits (PR163/ADR0025). Cut the
  implementation branch from `c612152` and cherry-pick only this brief commit.
- Red under repair: CI43880904887, Windows only. `r5_shutdown_read_owners` hit `Elapsed(())`
  at `three_node.rs:1195`, in the 2 s post-release wait after `gate.open()`. It is outside
  the eight existing quarantines. The cause is unknown: Opus disposition `9075daa`
  reproduced nothing locally and proposed no repair.
- PR162 is diagnostics only, held unless the owner decides; never call it a fix.

## Coupling: why the sampler must change

- The guard pins `EXPECTED` (8 names) and one `REASON`. Every
  `cfg_attr(target_os="windows", ignore=REASON)` gate must equal `EXPECTED`, and
  `source.count('ignore') == len(EXPECTED)`.
- The sampler execs the guard's bytes. `check_expected` requires `EXPECTED == IDS` (8). Its
  `--run-ignored only` filter `test(/^cluster::consensus::three_node::/)` matches every
  ignored test in the module.
- An R5 ignore alone therefore yields 9 matches. `check_inventory` raises
  `Guard("inventory")` and the step exits 1, so `verify (windows-latest)` goes red.
  `test_nonignored_a0_cases_mismatch_and_unexpected_match_gates` already pins that an R5
  match gates. A source-only quarantine fails by construction.

## Decision: exclude R5 from the sample; keep the sampled 8 byte-identical

Rejected: growing the sample to 9. It changes `IDS`/`ID_ORDER`, the released `id=` enum and
the step-line math (`8*16-125`). It adds a third 4-thread wave under the unchanged
`global-timeout = "50s"`, which risks `diag_incomplete` for the existing eight (a silent
coverage loss). It adds Windows wall time while `ci-time-budget` is already red, and it needs
a fresh Sol review of the sample contract. A green sampled R5 is not re-enable evidence
anyway.

Chosen: separate guard inventories plus one fixed sampler-filter exclusion; `EXPECTED`,
`IDS`, `ID_ORDER`, budgets, grammar and profile unchanged.

**Explicit coverage change:** R5 no longer runs on Windows, neither in the gate nor in the
sample. Ubuntu and macOS keep R5 required. Windows R5 observation needs a future,
separately reviewed contract (Opus C1–C5 or the held PR162).

## Allowed files (nothing else; production bytes unchanged)

1. `crates/arxd/src/cluster/consensus/three_node.rs`: add one rustfmt-shaped attribute
   between `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]` and
   `async fn r5_shutdown_read_owners()`:
   `#[cfg_attr(target_os = "windows", ignore = "<REASON_R5>")]`, where
   `REASON_R5 = "Windows R5 post-release quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"`.
   `git diff c612152 -- crates` shows only these added lines. The body, assertions, 2 s
   deadline, retries, helpers and the other eight gates are byte-unchanged.
2. `tools/ci/test_windows_cluster_quarantine.py`:
   - Keep the `EXPECTED` and `REASON` lines byte-identical; the sampler's guard-mutation test
     edits `EXPECTED` bytes.
   - Add `WINDOWS_UNSAMPLED = ['r5_shutdown_read_owners']` and `REASON_R5`.
   - Move the assertions into a pure `assert_windows_quarantine(source)` that requires: gates
     using `REASON` == `EXPECTED`; gates using `REASON_R5` == `WINDOWS_UNSAMPLED`; the sets
     disjoint; `count('ignore') == len(EXPECTED) + len(WINDOWS_UNSAMPLED)`; each named
     `async fn` occurring exactly once; the existing `#[cfg(target_os` and
     `#[cfg(not(target_os` bans.
3. `tools/ci/windows_diag_sample.py`: change only the `FILTER` constant, to
   `package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/) & not test(=cluster::consensus::three_node::r5_shutdown_read_owners)`.
   Argv stays built from module constants only; nothing is derived from the guard at runtime.
4. `tools/ci/test_windows_diag_sample.py`: update the `test_fixed_argv_matches_brief`
   expression and add the tests below.
5. Docs: an R5 addendum to `docs/adr/0024-windows-three-node-test-quarantine.md` (amend, not
   a new ADR: 0025 is claimed by unmerged PR163); amend the inventory/filter lines of
   `docs/briefs/m3-windows-hosted-diagnostic-sample.md` (8 sampled, R5 excluded); and in
   `docs/PROGRESS.md` the health line below plus checkpoint and provider entries.
Out of scope: workflows, `ci-workflow.test.mjs`, nextest profile, `policy.mjs`, `ci-budget.mjs`,
protection/labels/statuses, any timeout/retry/helper/production change, PR162, C1–C5.

## Meaningful tests (each must fail on its mutant)

- **Guard.** The real source passes. Each mutant raises `AssertionError`: R5 gate removed;
  R5 `target_os = "windows"` changed to `target_family = "unix"` or `all()`; R5 gated with
  `REASON`; one of the eight gated with `REASON_R5`; a tenth `ignore`; R5 appended to
  `EXPECTED`; `r5_shutdown_read_owners` renamed or removed. Issuance tests stay unchanged.
- **Sampler.** The `FILTER` exclusion equals the guard's `WINDOWS_UNSAMPLED` (loaded in the
  test only, via the verified-bytes exec path). A `listing()` with R5 `ignored: true` and
  mismatch reason `expression` passes `check_inventory`; the same R5 with `matches` gates
  `inventory`. All existing tests pass unchanged, including 7/9-name rejections and step-cap math.
- **Local evidence:** both unittest files; `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets --locked -- -D warnings`; Windows
  `cargo nextest list -T json` with the sampler's exact argv (exactly 8 matched and ignored,
  R5 mismatched); Windows `cargo nextest run -p arxd --lib -E 'test(=cluster::consensus::three_node::r5_shutdown_read_owners)'`
  reports R5 skipped; the scope diff. Linux/macOS R5 is hosted-only (no local Linux runner).

## Accurate main-health line (same PR, replaces the stale `688f1ad` green at base)

> **Current main RED:** exact `c612152` after PR161, CI43880904887 failed. Windows
> `r5_shutdown_read_owners` timed out at `three_node.rs:1195` in its 2 s post-release wait,
> outside the eight quarantines. The cause is unknown. This PR adds a reviewed Windows-only
> R5 quarantine: an exception, not a fix. PR162 diagnostics remain held unless the owner
> decides. The whole run took 1475 s, which also fails `ci-time-budget` under the active wall
> policy. ADR0025 is accepted but inactive, and its bootstrap is not authorized. Main stays
> RED until a merged recovery's next exact-main CI is green.

It conflicts with PR163's line 5; whichever merges second rebases, keeping newer facts.

## Re-enable criteria for R5 (separate from cause proof)

R5's gate can be removed only when all of the following hold:

1. **Observation path.** A separately reviewed path, merged under normal gates, runs R5 on
   natural hosted Windows CI and emits per-predicate (`sm`/`log`/`permits`) post-release
   clear times on pass and fail.
2. **Margin evidence.** After that path lands, at least 10 natural hosted Windows R5
   executions pass, with no reruns, dispatches or loops, zero failures, and a recorded
   maximum clear time at or below 1000 ms (50% of the unchanged 2 s wait).
3. **No tuning.** Deadline, assertion, retry and helpers are unchanged. Any repair is a
   separately reviewed fix attempt, counted against Opus 0/2, and criterion 2 restarts on
   the repaired source.
4. **Re-enable PR scope.** It removes only the R5 gate, empties `WINDOWS_UNSAMPLED` and
   restores `FILTER`, with all five gates and a different-provider review. If the next
   exact-main CI is red, the quarantine is restored through a reviewed recovery PR, the failure counts as an
   attempt, and there is no rerun.

Cause attribution alone neither re-enables R5 nor is required: proof of cause gates a
repair, not a re-enable. R5 and the eight (recovery plan §6) are re-enabled independently.
Revisit before M3 acceptance or Windows cluster use in M4.

## Gating deadlock (honest merge-blocking condition)

Required: `verify (ubuntu|windows|macos)`, `ci-time-budget`, `agent-review`. The three
`verify` contexts and `agent-review` are attainable. `ci-time-budget` is not: `budget()` in
`scripts/policy.mjs` reads the latest completed main push run, CI43880904887 at 1475 s
(over 900 s). It passes only with the `ci-speed` label plus a faster successful candidate,
and steering forbids that label. No new main run can exist without a merge; reruns are
forbidden. ADR0025 queue credit is inactive, its bootstrap is unauthorized, and it sits on
PR163, blocked by the same gate. The #152 restore-green exception covered a red-main base,
not the budget, and may not be extended. **This PR cannot satisfy all five gates as things
stand.** No label, manual status, protection change or waiver is permitted. An explicit owner
decision is required, recorded before use: (a) authorize the ADR0025 bootstrap (main has 738 s charged, but credit is withheld
because Windows failed, so activation alone does not clear this hold); (b) authorize a one-time budget
exception for this PR; or (c) authorize the existing `ci-speed` measured-reduction path.

## Next action

Root assigns Sol to implement exactly the allowed files from `c612152`, then an Opus high
source review. Publication follows root and steering. Once four gates are green, root puts
the `ci-time-budget` choice (a), (b) or (c) to the owner. Nothing merges before that
decision.

M2 is open, M3 is unchecked, and Windows three-node is **unverified on real hardware**.

Root Sol contract check: accepted for local implementation only. The known main
measurement is 1475 s wall / 738 s charged, uncredited because red. ADR0025 alone
cannot restore this gate. No merge exception is granted by this contract check.
