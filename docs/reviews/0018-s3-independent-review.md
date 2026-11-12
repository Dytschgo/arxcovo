# ADR 0018 S3: independent Opus standards and spec review

Reviewer: Anthropic / Opus 5.5 / high. This is a single-agent review with no
subagents. It covers the Sol (OpenAI / gpt-6.1-sol / high) S3 owner CLI wiring
candidate. Date: 2026-11-12. The review grants no authority to merge, publish,
accept or activate anything.

## Pins and scope

- Target: `7f708529cd076dc51486f581d96145e0a534f8f4`. That commit is a
  PROGRESS checkpoint only and has no source change.
- Source: `edcadff70d20c6a5f6847679b9dea3b468ef1802`.
- Evidence: `558b94895de98938ceb649ab73f3feaa54fd1fdd`.
- Implementation base: `46f3a9ffc42983216ae55a4be27350a40f43580a`. It sits on
  actual main `c4ef8ada086f6595a6a83c38cc77b34f87ebbd9f` (#49).
- Worktree: `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-7899ca8c-3a70bd233515edb0`.
  pwd and the Git top-level matched, HEAD was the target, the tree was clean,
  and it was not the canonical `D:/Code/Arxcovo`. I created no checkout or
  worktree and wrote nothing to the canonical root.
- I reviewed the whole diff `c4ef8ad...7f70852`: 19 files and two new modules.
  It includes the FIRST safety commit `5c57208`, which changes only
  `tests/service.rs` from `install` to `install extra` and keeps the old
  expectation, before any routing exists. It touches no Cargo, lockfile or
  workflow.
- In the export, the SHA-256 of `owner_cli.rs`, `owner_cli/tests.rs` and
  `native.rs` equals the evidence manifest.
- Spec sources: ADR 0018, `docs/briefs/owner-cli-wiring.md` (§2–§8), and
  ADR 0009/0012/0014/0016 as that brief cites them. I also read the S1 review
  (ST-1, SP-1 and the test-5 deferral to S3).
- Standards sources: AGENTS.md, CONTRIBUTING.md, KICKOFF rules, and the Fowler
  smell baseline, which I treat as judgement only.

## Verdicts

- **Standards: CHANGES REQUIRED (small).** One documented-rule breach
  (ST-1: behavior without a test) plus advisories.
- **Spec: CHANGES REQUIRED.** SP-1 needs an architectural (contract)
  disposition. SP-2, SP-3 and SP-4 are required-test gaps. I proved SP-2 and
  SP-4 with compiled mutants that survived. SP-5 is an untested stdout mapping.

The authority boundaries hold by inspection. The guard comes first on all six
Windows verbs. Unix runs `require_owner` before layout and context. Configure
parses the previous config in full and does no inventory bypass. Start uses an
inventory-only preflight, then drops the guard, then creates the SCM.
Uninstall consumes the proof. NativeOps is `cfg(not(test))`. Invalid argv
exits 2 before NativeOps. No test reaches a valid owner argv.

## Architectural disposition: old build account missing (SP-1)

**Actual failure path.** For a previous config whose build account no longer
resolves, the steps are:

1. `unix_configure` or `windows_configure` calls `ServiceConfig::parse(old)`.
2. `revision(name)` returns the default `Ok(None)` on Windows and Linux.
3. `revalidate` calls `validate`, which calls `validate_inventory`. The client
   lookup there maps every error to `Identity`.
4. `execution_preflight` then calls `account(build name)`. It returns
   `AccountMissing`, which becomes `ExecutionUnavailable::AccountMissing`.
5. `validate` maps that to `Error::Identity` (`config.rs:190-197`). This
   happens before `workspace_safe`.

So `ServiceConfig::parse` never returns `AccountMissing`, for either account.
The native resolvers also differ by platform. Only Windows produces
`AccountMissing`, from `ERROR_NONE_MAPPED` (`native_windows.rs:69-75`). The
Linux and macOS resolvers report absence as `Identity` (`native_unix.rs:230`,
`native_macos.rs` `observe`). `from_request` still passes `AccountMissing`
through for the NEW `--build-account` and `--client-account` lookups (brief
§3 step 3), so `service-account-missing` is reachable only there, and natively
only on Windows. The builder's fixture checks exactly this path:
`account:old-builder` fails in `execution_preflight`. The observed result,
`Identity` and exit 3, is correct library behavior.

**Recommendation: correct the contract, not the implementation.** This is the
smallest justified correction.

- ADR 0018 decision 4, the governing text, already says the remedies apply when
  an old account fact fails with "`service-identity` or
  `service-account-missing`". It also says the CLI "does not weaken
  `configure_plan`, `parse` or `revalidate`".
- Brief §4 (A-SP3) requires "the library's error (exit 3)" and no change to
  `parse` or `revalidate`.
- Only brief §7 test 5's last bullet ("returns `AccountMissing`")
  contradicts these, so that bullet is the error.
- Making `validate` propagate `AccountMissing` would change shared S1 library
  semantics. That would affect every `parse`/`revalidate` caller
  (`configure_plan`, `uninstall_plan`, `Configure::from_plan`). It would need
  its own accepted API decision, and Linux and macOS still could not produce
  the token.

The exact semantic scope of the clarification:

- Brief §7 test 5, last bullet becomes: "a previous config whose build account
  is now missing returns the library's `Identity` (`service-identity`, exit 3):
  `validate` folds execution-preflight `AccountMissing` into `Identity`. The
  same zero-mutation assertions apply."
- Add one ADR 0018 history note. A stale previous account surfaces as
  `service-identity`. `service-account-missing` comes only from the new
  account lookup in `from_request`, and natively only on Windows. The fixed
  token set, exit classes and remedies are unchanged.
- The clarification needs cross-provider review: if Opus authors it, Sol
  reviews it, and if Sol authors it, Opus reviews it. It adds no authority.

The tests it needs:

- Unix and Windows compositions both assert exactly `Err(Identity)`. The token
  and exit come through `error()`.
- The call log contains `account:old-builder` and contains no `workspace:/old`
  entry, which proves the failure happened before `workspace_safe`.
- Windows: session calls equal `["read"]`. Unix: zero `execute` calls.
- The guide adds the account case (SP-6).

Both fixed tokens matter even though both exit 3, so the guide must name the
token that actually appears. Nothing is waived. No code or docs were changed by
this review.

## Spec

**SP-1 (Blocking, contract).** Disposition above. The Windows composition also
lacks the missing-old-account case entirely. Brief §7.5 requires it for both
`windows_configure` and `unix_configure`.

**SP-2 (Blocking, required mutation survives).**
`owner_cli.rs:307-310` (`unix_configure`). Brief §7 requires that the mutation
"replace [previous-config parsing] with `parse_inventory`" fails a test. I
replaced the Unix `ServiceConfig::parse` with `parse_inventory`. The mutant
compiled, and `unix_full_previous_validation_and_no_agent_merge` still passed
(run `5b514c18-3427-489b-9fa3-111e7bc579ad`, exit 0).

The cause: `configure_plan` calls `old.revalidate` again. With fake revisions
of `None`, that call reproduces both the stale-workspace error and the
missing-account error, and Unix has no intermediate session call to observe.
The behavior stays fail-closed, so this is not an authority hole. It is still
a mandated kill.

The builder's `old-inventory` kill applies only to the Windows composition,
where the extra `observe` call reveals the mutant. For contrast, my
`unix-old-none` mutant was killed (`fc7d4855-e07c-4bb7-a020-b876762d1f3b`,
exit 100).

Correction: in the Unix test, assert the exact resolver call sequence. Full
parse records `workspace:/old` before `configure_plan`'s revalidation calls.
Alternatively, add a counting resolver that returns `Some` revisions, so the
inventory-built config fails the revision comparison with a different error.
Re-run the mutant and record the kill.

**SP-3 (Blocking, wiring matrix).** `owner_cli/tests.rs:338-417, 453-576`.
These brief §7.5 items have no new CLI-level case:

- A changed build pin refused before `configure`. On Windows the calls should
  be `["read","observe"]` with no configure. On Unix, no `execute`.
- The missing old account on Windows (SP-1).
- The `observe_batch` argument. The fake ignores `sid`, so "observe(new SID)"
  is never asserted.
- One recorded-grant mirror case: `present` true with a Granted record, so
  `adds_right` is false and the mirror is true.

The exhaustive mirror and pin combinations are justified inherited library
coverage (`owner_transactions_tests.rs` ~670-700, `configure_plan`
`owner_plan.rs:444-447`). One wiring case each is cheap and is required.

The dispatch success matrix (only `running` and one RightReport) is justified
inherited coverage, because the status tokens come from library `token()`.
That does not hold for uninstall (see SP-5).

**SP-4 (Medium, parser boundaries and guide forms).**
`owner_cli.rs:48-50`, `tests.rs:33-159`. Brief §2 refuses only "more than"
each bound, and §7.1 says "accept every service-install.md example form".
Three compiled mutants survived `parser_matrix_and_fixture_fence`:

| Mutant | Run |
| --- | --- |
| `args.len() >= 170` | `7c3fa6de-7396-45fb-a30f-24ad3ceaf254` |
| `a.len() >= 4200` | `81f82116-2906-4208-ab9d-48a571838d8d` |
| total `> 30000` | `e53e304d-7d87-40ae-8a28-600732a86dd1` |

The first mutant would reject the maximal valid configure: 4 single flags,
16 `--env` and 64 `--agent` give exactly 170 arguments.

Add accept cases:

- exactly 170 arguments;
- one 4200-byte argument;
- exactly 32768 total bytes;
- the guide's Windows configure line, with backslashes, `;`, four `--env`
  values and placeholders filled;
- `--rights admin`;
- `--env A=b=c`, which must keep `b=c`.

Add negative cases:

- each single flag missing;
- an odd-length tail;
- `--Workspace` and abbreviated flags.

**SP-5 (Medium, untested stdout mapping).** `owner_cli.rs:373-383`. The Windows
uninstall `RightReport` to token mapping is new code under
`cfg(not(test))`, so no test compiles it. Swapping two tokens, for example
removed and retained-unproven, would mislead the owner about right provenance
and pass every test by construction. It also duplicates the existing
`RightReport` Display (`owner_transactions.rs:310-318`). Correction: use the
library Display through one pure mapper that tests can reach. Assert all five
tokens against `to_string()`.

**SP-6 (Low, guide).** `docs/service-install.md`. The stale-previous-config
text covers only `service-workspace`. Brief §8 S3 requires the refusal and its
two remedies. ADR 0018 decision 4 applies them to stale account facts too. Add
the account case with the token that actually appears (`service-identity`,
per SP-1).

**SP-7 (Low, dispatch fixture).** `tests.rs:160-309`. Brief §7.2 asks for
marker strings in every argument and exactly the parsed values.
`RecordingOps::configure` checks five fields with real-looking values, not the
environment, agent hashes or count. Errors run through `client-config` only.
The tokens are static, so leakage is structurally impossible. Use marker
values and compare the full `ConfigureArgs`.

**SP-8 (Accept, record).** The `windows_start` deferred-closure seam
(`owner_cli.rs:279-286, 342-355`). The production order is:

1. guard
2. layout clone
3. `owner_start_preflight`
4. `drop(guard)` inside the preflight closure
5. `WindowsScm::system` and then `start`, inside the second closure

On a preflight error, the closure that holds the guard is dropped on return,
and no SCM handle is ever created. That is stricter than brief §6's
`scm: &mut WindowsScm` shape, which would require constructing the SCM first,
and it matches the §4 table. The seam is private, adds no authority and
creates no parallel production logic. Record it as a brief §6 clarification.

The test asserts only that the calls contain `start`. Tighten it to equality
with the call sequence of a direct `engine(true, STOPPED).start()`, which is
§7.6 "exactly the PR-C start sequence". The guard drop, native SCM ordering
and elevation-lock release remain **source-only**.

**SP-9 (Accept).** `native::read_config` now goes through `read_config_with`.
The platform check, the pinned parents and handle, the bounded read and the
recheck of protection on the same handle are byte-equivalent to the old code.
`_pins` is a named binding, so the pins stay alive through `parse_inventory`
and drop at return, as before. TOCTOU is unchanged and nothing gains new
authority. The test proves pin retention and release, and that only the client
is looked up. The S1 ST-1 call-site mutation is killed (builder runs
`0ae602bb…` and `5ad535cd…`, credited). Native callback selection
(S1 SP-1), guard, install, session pins and SYSTEM access remain source-only.

These requirements are implemented correctly, checked by inspection:

- **Routing (main.rs).**
  - It uses bounded `args_os`.
  - Both reserved prefixes route before Tokio.
  - Exactly `service run` reaches `service::run()` unchanged.
  - The foreground path is unchanged.
  - Non-UTF-8, size and count bounds are checked before conversion.
  - Invalid argv exits 2 before NativeOps.
  - Owner routes read no environment and accept no path or layout override.
- **Grammar.**
  - Duplicate single flags are refused.
  - Flag order is free.
  - `--env` and `--agent` values are kept in order, duplicates included, and
    capped at 16 and 64.
  - `client-config new` takes exactly six arguments.
- **Error mapping.**
  - The `error()` matches are exhaustive with no wildcard.
  - The exit classes are 2/3/4/7/1 as the brief tables them.
  - `Permissions` and `OwnerRequired` exit 4.
  - Every `Failure` exits 1.
  - `OpenError` maps as specified.
- **Output.**
  - Stdout carries only the status token, the right token, or the
    principal/hash line.
  - Partial prints no hash and nothing on stdout.
  - OS details never reach stderr.
- **Unix.**
  - Every verb runs `require_owner`, then `Layout::system`, then the context.
  - Uninstall uses `owner_plan(Uninstall)` with no config.
  - `unix_verb` cannot read a config by signature.
  - The Unix `uninstall_plan` mutation applies only to `cfg(not(test))`
    NativeOps. It is untestable by construction, so this is source-only
    review.
- **Previous-agent merge.** The CLI has no merge path. My
  `swap-build-client` mutant on the `request()` field mapping was killed
  (`19eaa817-90af-45a8-bf73-583ded81aef7`).
- **Fixtures.** The no-original-valid-install fixture remains in place, and
  the parser fence test rejects every argv that `tests/service.rs` spawns.
  No other integration test spawns an owner prefix.

## Standards

**ST-1 (Hard: AGENTS.md "Every behavior needs a meaningful test").** The
uninstall RightReport output mapping has no test (SP-5).

**ST-2 (judgement: Duplicated Code).** `owner_cli.rs:154-191` re-types the
`Error` and `Failure` Display tokens. The brief says "its Display token". Keep
the exhaustive match for exit codes, and add `token == e.to_string()` to the
exhaustive test so the owner and service tokens cannot drift.

**ST-3 (Low, stale allowances).** `executor.rs:80-81` keeps
`#[cfg_attr(not(test), allow(dead_code))]` with "S2/S3 consume this hook",
but production now uses the hook through `client_config`. Brief §3 says S3
removes these. `owner_scm.rs:2` (`#![allow(dead_code)] // PR-D wiring is
deliberately absent.`) and `owner_scm_windows.rs:2` are now false. Narrow
them or correct the comments.

**ST-4 (Low, meaningful test).** `native_entry_signatures_are_referenced_never_invoked`
only binds a function pointer to silence dead code. It is fine as a compile
check. Name it as one, or use `cfg_attr(test, allow(dead_code))` on `create`.
Do not count it in behavior coverage. It is one of the 295.

**ST-5 (Low, one concern).** The `native.rs` S1 ST-1 follow-up touches the
service startup reader inside the `feat(cli)` commit. It is
behavior-equivalent (SP-9). Prefer its own commit, or name it in the PR body.

**ST-6 (advisory).** `dispatch` ignores stdout write errors and exits 0. A
closed stdout would lose the `client-config` line after a successful create.
That is recoverable, so this is a note only. In `crates/arxd/README.md` one
line exceeds the file's width.

Smell baseline: `parse` carries six `Option` slots (possible Data Clumps). The
grammar is small and fixed, so I would not restructure it. I found no other
smell.

## Checks run by this reviewer

All of these ran in an owned scratch `git archive 7f70852` export with its own
target directory. I made no worktree source edits. Nothing used production
NativeOps, `create`, OwnerGuard, a session, the native preflight, a store,
`NativeResolver`, fixed system paths, SCM, LSA, accounts, the pipe, a VM or
Lab Manager. No valid owner argv ran. I did not run the
`service run` fixture or the full suite.

- **Focused run** `e16ad614-1ddf-416a-989a-72c8b8a465a1`: 11/11 passed, 285
  skipped. Filter:
  `test(service::owner_cli) | test(cli_start_preflight) | test(native_read_parser) | test(u1_uninstall_flow) | (binary(service) & test(service_run_requires_exact_arguments))`.
  The binary test spawns only the seven invalid argv.
- **Mutations.** `owner_cli.rs` was the only file mutated. It was restored from
  saved bytes, and byte equality was asserted after each run. The final SHA
  `3ea27f27…` equals the manifest.

  | Mutant | Run | Exit | Result |
  | --- | --- | --- | --- |
  | Unix old parse → `parse_inventory` | `5b514c18-3427-489b-9fa3-111e7bc579ad` | 0 | **survived** (SP-2) |
  | Unix old → `None` | `fc7d4855-e07c-4bb7-a020-b876762d1f3b` | 100 | killed |
  | `>= 170` | `7c3fa6de-7396-45fb-a30f-24ad3ceaf254` | 0 | **survived** (SP-4) |
  | `>= 4200` | `81f82116-2906-4208-ab9d-48a571838d8d` | 0 | **survived** (SP-4) |
  | total `> 30000` | `e53e304d-7d87-40ae-8a28-600732a86dd1` | 0 | **survived** (SP-4) |
  | swap build/client in `request()` | `19eaa817-90af-45a8-bf73-583ded81aef7` | 100 | killed |

- I deliberately did not run the RightReport swap. It is `cfg(not(test))`, so
  it survives by construction (SP-5).
- I deliberately did not run the old-versus-new SID mutant. It is equivalent
  on every success path, because `configure_plan` refuses a pin change after
  observing. The SID assertion is requested for the record only.

## Credited, not re-run

- The builder's evidence is credited to its own pins and was not repeated:
  - focused runs `61072229…` (red), `5dbdd530…` (red), `4c627d51…` and
    `53bf5075…`;
  - 13 restored mutations;
  - fmt and locked all-target Clippy;
  - the single full run `e2e5f587-88a4-4103-a8b6-74e41f98a857` at `edcadff`
    (295 passed, 2 skipped, 10.243s).
- The red history is attributed correctly: it was fixture injection, then the
  Identity/AccountMissing discrepancy, with no production fix.
- The 295 passes are not full contract acceptance.
- Main #49 run `43221533019` (green, 439s) is coordinator evidence. I did not
  check it.
- E2's natural ExecutionFailed (`bdbf032b`) remains open and unexplained,
  with no waiver.

## Limits

- Unix code that is gated by `cfg(unix)` was not compiled locally. That
  includes Unix NativeOps, `unix_context` and the non-UTF-8 Unix branch. Hosted
  Ubuntu and macOS are mandatory.
- All five hosted gates on the published head are still outstanding.
- NativeOps compositions are source-only review. That covers guard ordering,
  the start guard drop before SCM creation, configure session pins, the
  uninstall proof and native client creation.
- Installed, native and dedicated paths are **unverified on real hardware**.
  M2 is incomplete.
- After three failed fixes, the work goes to the other provider with the
  failure evidence.
