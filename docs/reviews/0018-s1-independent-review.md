# ADR 0018 S1: independent Opus source review

Reviewer: Anthropic / Opus 5.5 / high. This is a single-agent review with no
subagents. It reviews the Sol (OpenAI / gpt-6.1-sol / high) S1 implementation.
The independent credit covers only the new Sol S1 implementation. The original
Opus architecture keeps its Sol review credit. Date: 2026-11-12.

## Pins and scope

- Target: `813dd1e575e1f1cbfe50237c43d115a6134b39bd`
- Base: `b1663060452f8fed25bf64986bc3892bc37c7313`. This is the coordinator's
  docs-only ledger on main `d70e808` after PR47.
- Commits: `dd07995` (S1 source), `b25797d` (Unix-only fixture modes),
  `813dd1e` (evidence and PROGRESS).
- Worktree: `C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-cd232bca-0bbee57d1f039deb`.
  The Git top-level was checked before every test and before this write. It
  differs from the canonical `D:/Code/Arxcovo`, which was not touched.
- Diff: `config.rs` +367, `executor.rs` and its tests, `native.rs` +120,
  `owner_unix.rs` +93, `owner_windows.rs` and its tests, plus the docs. The diff
  does not touch the CLI, `main.rs`, `client_config`, the runner, the bridge,
  workflows, packaging, `Cargo.lock` or any dependency.
- Spec: ADR 0018 decisions 3 and 4, and brief `docs/briefs/owner-cli-wiring.md`
  §3 S1, §7 tests 4–6 and §8. Standards: AGENTS.md, CONTRIBUTING.md and the
  Fowler smell baseline, which is used as judgement only.

## Verdict

- **Standards: PASS with advisories.** Nothing blocks the merge. ST-2 is cheap
  and recommended for this slice.
- **Spec: PASS, conditional on hosted Linux and macOS gates.** The Unix-only
  source and tests in `owner_unix.rs` were never compiled or run locally, by
  the builder or by me. I inspected them by hand and found no compile or logic
  defect. The gap is not waived: the hosted Linux and macOS runs must compile
  and pass `owner_unix::imp::tests` before merge.

This review does not clear the merge gate. Hosted required checks and the
coordinator's disposition are still needed. Native, installed and dedicated
paths remain **unverified on real hardware**, and M2 is incomplete. The open
`ExecutionFailed` bdbf032b remains unknown. Nothing here claims a fix for it,
and nothing here proves a new regression.

## Standards

**ST-1 (judgement: Middle Man / seam strength), `native.rs` `inventory_config`.**
`inventory_config` is a one-line wrapper around `parse_inventory`. The test
`native_read_parser_makes_only_client_observations` pins the wrapper. It does
not pin the call in `native::read_config`. If someone edited `read_config` to
call `ServiceConfig::parse(&bytes, layout, &NativeResolver)` directly, the
change would bypass the seam and no test would catch it. I found this by
inspection; I did not run that mutation, because it would mutate the real
native read caller. The seam is still a useful guard against changes inside
the wrapper. The S3 brief already lists "use `ServiceConfig::parse` in
`read_config`" as a required mutation. Keep it, and apply the mutation at the
`read_config` call site.

**ST-2 (behavior without a test), `owner_unix.rs` `Manager::read_config`.**
The new fixture covers four cases: an absent parent, an absent file, file mode
0640, an oversized file and a symlinked target. It has no negative case for
the parent privacy check `self.fs.check(&parent, true, Some(0o700))`. Deleting
that line would pass the new test. AGENTS.md says "every behavior needs a
meaningful test". The fix is cheap: in the same owned-temp fixture, chmod the
config directory to 0750 and assert `Err(Permissions)`. The same-inode `same`
check also has no negative case. A swap fixture needs a hook, so accept that
one as residual risk unless an existing `Fs::same` test covers it.

**ST-3 (meaningful test), `owner_windows_tests.rs`
`configure_session_seam_has_no_required_native_authority`.** The test only
checks that its own `Refusal` fake returns `Busy`. It never exercises the
production `impl ConfigureSession for WindowsOwnerSession`, and that impl
cannot be exercised without constructing a session, which is forbidden. The
test's real value is a compile-time check of the signatures, and that a fake
can implement the trait without a `BatchObservation` constructor. Rename the
test to say that, for example `configure_session_fake_needs_no_batch_constructor`,
and do not count it as behavior coverage.

**ST-4 (residual coverage), `native.rs` `state_guard`.** No test pins that
`state_guard` calls `require_service_identity()` before `state_members`. The
gap existed before this slice: the code was byte-equivalent before the split.
The split adds risk, because a sibling without the guard now exists. A future
edit that calls `state_members` from the service path would pass the tests.
Add a small `state_guard_with(identity, members)` order seam later, if that is
practical. Do not run it against real callers.

**ST-5 (judgement: Duplicated Code), test resolvers.** The tests add two new
bespoke resolvers, `Counting` in `config` and `InventoryOnly` in `native`. The
`FakeResolver` from `config::tests` already exists. The duplication is
tolerable because `Counting` records the call order the guards need. Prefer
extending one shared counting fake in S3.

Style notes, not findings: `DEFAULT_LIMITS`, `ConfigureRequest` and
`valid_principal` sit between the `use` block and `MAX_CONFIG_BYTES` with no
blank lines and no doc comment on the constant. The `(u8, u64, u32)` tuple is
mandated by the spec, so it is not flagged as Primitive Obsession.

Every unused new item has `#[cfg_attr(not(test), allow(dead_code))]` and a
comment naming S3 (S2/S3 for the shared predicate). None of the new items is
Debug or Clone. No production constructor, guard, batch, store or right was
added. Tests bind the hook signatures without invoking them.

## Spec

All S1 requirements in brief §3 are implemented. Checked item by item:

- **config.** `DEFAULT_LIMITS` is `(1, 8_589_934_592, 512)`. `valid_principal`
  is the extracted rule, identical to the old inline check. `ConfigureRequest`
  borrows its fields, has no Debug or Clone, and its fields match the brief.
  `from_request` runs in this order: `validate_platform_rights` (macOS admin
  returns `Rights` before any resolver call), then the duplicate env check
  (`Config`), then build account `User` (errors pass through), then client
  account `User`, then the private `Wire`, then serde, then the full
  `ServiceConfig::parse`. The build name is kept verbatim, agents keep their
  input order, and `granted_batch_right` is false. Tests cover the ordering
  (macOS admin together with a duplicate env still returns `Rights` with zero
  resolver calls), group and missing accounts for both accounts with exact call
  counts, and the canonical JSON round trip on all three layouts. They also
  check that a stale previous config refuses through both `parse` and
  `revalidate`, that `configure_plan` refuses an old workspace that is no
  longer safe and records the resolver call, and that the compare-and-replace
  `expected` equals the exact previous bytes.
- **executor.** `windows_component_safe` keeps the device and superscript
  rules, and COM0, LPT0 and COM10 remain accepted. The trailing dot or space
  check was folded into the shared predicate, as the brief asks. This does not
  change `absolute_executable`, whose later all-component check already
  refuses trailing dots and spaces. The drive component is excluded from both
  checks.
- **native.** `state_members` is unchanged, and `state_guard` still calls
  `require_service_identity()` first. `state_members` opens existing objects
  only and creates nothing. `owner_start_preflight` is Windows-only and runs
  `observe_layout`, then `read_config` (which uses `parse_inventory` with
  `NativeResolver`), then `state_members`. It does no build lookup, no
  workspace check and no execution preflight. `start_preflight_with` is the
  shared sequence; pins are retained and dropped in reverse order on success
  and on each error.
- **owner_unix.** `require_owner` is the existing four-ID rule, now pure-tested
  through `owner_identity`. `read_config` calls `Manager::new()` (root plus the
  fixed binary), walks the pinned no-follow parent, checks parent 0700, calls
  `open_at` (absent returns None), then `bytes(0o600, MAX_CONFIG_BYTES)` and
  `same`. The fixture fix in `b25797d` (0755 ancestors, 0700 leaf) matches the
  existing manager fixtures, so `directory()`'s exact-mode check holds on both
  layouts.
- **owner_windows.** `ConfigureSession` has exactly the existing `&mut self`
  signatures. The only production impl delegates to `WindowsOwnerSession`.
  No constructor was added.

**SP-1 (residual, not blocking).** The `owner_start_preflight` body binds the
three closures, and no test checks that binding. The generic
`start_preflight_with<L, C, S>` would accept swapped closures, or
`state_guard` in place of `state_members`, and every test would still pass.
Calling the real function would touch fixed system paths, so it cannot be
tested under the current rules. Record it as a known residual risk. The S3
`windows_start` test covers only preflight before SCM.

**SP-2 (interpretation accepted).** `Manager::read_config` returns `Ok(None)`
when the config's parent or an ancestor is missing. The brief says "absent →
None" only for `open_at`. No config can exist without its parent, and the
later atomic config write refuses with `Layout` when the parent is missing.
So this leaks no authority and skips no revalidation.

**SP-3 (note).** Brief test 4 names `FakeResolver`. The implementation uses an
equivalent counting resolver on Windows, Linux and macOS layouts. Coverage is
equal or stronger.

Two S1 test items are not here, as the brief expects. They belong to S3:
`windows_configure` and `unix_configure` (test 5 wiring), and `windows_start`
with fake SCM (test 6, first part).

## Checks run by this reviewer

All checks ran in the verified worktree or in a scratch export of the pinned
head. None ran against production guards, real callers, fixed system paths,
SCM, LSA, accounts, services or VMs.

- Focused run: `cargo nextest run --locked -p arxd -E 'test(service::config::request_tests) | test(service::native::owner_preflight_tests) | test(shared_windows_component) | test(configure_session_seam) | test(service::config::tests)'`.
  arxd was rebuilt fresh at `813dd1e` (53.71s). **14 passed, 258 skipped.**
  Run `5cd316a3-afb2-4f03-82ea-0d6703aa4118`.
- Scratch export: `git archive 813dd1e`, with a separate `CARGO_TARGET_DIR`.
  The unmutated baseline passed 3 of 3 (run `40e7dbc4-a8ce-4f15-bac0-29abadc79997`).
  Every mutation compiled and failed at a runtime assertion with exit 100. Each
  file was restored from saved bytes and checked with `cmp` against the
  worktree:

  | Mutation | Run ID | Failing test |
  | --- | --- | --- |
  | Drop the trailing dot/space check from `windows_component_safe` | `f337c7bb-c87d-4baa-9c30-7c249d36490d` | `shared_windows_component_rule_preserves_device_controls` |
  | `let _ = layout()?` drops the layout pin early | `9933c1d2-83b3-48bf-bdbd-b25cd638f03b` | `owner_preflight_order_errors_and_pins` |
  | Reverse agent order in `from_request` | `d1e2b74a-c293-4cd4-9325-43eef2f34c8c` | `request_canonical_json_and_full_parse_on_all_platforms` |

  These are new reviewer checks. The builder's six earlier mutations stay
  credited to the builder's heads.
- I did not repeat the full suite, and I did not try a Unix cross-compile.
- I confirmed by reading that the diff stat contains no forbidden path and no
  `Cargo.lock` change.

## Limits

- The Unix-only code was never compiled or run locally. That covers
  `owner_unix` `read_config` and `require_owner` and their tests. Hosted Linux
  and macOS are mandatory.
- The builder's focused 69 and 9 tests and six mutations ran in the canonical
  checkout before the source moved. They are builder evidence; I did not
  re-run them.
- Whether an elevated admin can read SYSTEM-private config and state through
  `owner_start_preflight`, and the share modes against a running service,
  remain **unverified on real hardware** (brief §9).
- The S2 work in a separate worktree depends on S1 but does not clear S1's
  gate. No GitHub write, push, PR, status or merge was performed.
