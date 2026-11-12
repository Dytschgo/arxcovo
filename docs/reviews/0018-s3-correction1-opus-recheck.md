# ADR 0018 S3 correction 1: independent Opus standards and spec recheck

Reviewer: Anthropic / Opus 5.5 / high. Single-agent review, no subagents.
Date: 2026-11-12. It covers two things: the Sol (OpenAI / gpt-6.1-sol / high)
S3 correction attempt 1, and coordinator Sol medium's contract clarification
`c41fbb2`. This review grants no authority to merge, publish, accept or
activate anything.

## Pins and scope

- Target: `5cbb32ac6227576fdd9d26c9522e7b9ceb5e1b37` (evidence and PROGRESS
  only).
- Correction source: `974048e621edc8c468c9f62943a749bfb0218ddb`.
- Base: `c41fbb2cd972f19e3af7af81f0f0b06342f42a0d`. That is the coordinator
  clarification, on top of the retained full review `02499d1`, which reviewed
  `7f70852` with source `edcadff`. All of this sits on actual main
  `c4ef8ad` (#49).
- Worktree: `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-2c5315de-e221d55bcba59c61`.
  - pwd and the Git top-level matched, and HEAD was the clean target.
  - It is not the canonical `D:/Code/Arxcovo`.
  - I created no checkout or worktree and wrote nothing to the canonical root.
- Reviewed diffs:
  - `02499d1..c41fbb2`: ADR 0018, the brief, the guide and PROGRESS.
  - `c41fbb2..5cbb32a`: `owner_cli.rs`, `owner_cli/tests.rs`,
    `owner_scm_tests.rs`, the correction evidence and PROGRESS.
  - `02499d1..5cbb32a` makes no change to Cargo, the lockfile, `.github`,
    `native.rs`, `config.rs`, `executor.rs`, `client_config.rs`, `main.rs` or
    `tests/`. The FIRST safety fixture `5c57208` is preserved:
    `tests/service.rs:32` still spawns `service install extra`.
- Read: AGENTS, KICKOFF, ADR 0018, the complete wiring brief, the prior
  report, the correction evidence and the original S3 evidence. I also read
  the library code the clarification describes: `config.rs` `parse`,
  `revalidate`, `validate`, `execution_preflight` and `from_request`; the
  `native.rs` `NativeResolver`; and the `native_windows`, `native_unix` and
  `native_macos` account mapping.

**Source identity.** The committed blobs of `native.rs`, `config.rs`,
`executor.rs`, `client_config.rs` and `tests/service.rs` equal the evidence
manifest byte for byte. The three changed files do not match as committed
LF blobs. Their manifest hashes equal the CRLF conversion of the committed
blobs at `974048e`:

| File | Manifest hash (CRLF) | Committed LF hash |
| --- | --- | --- |
| `owner_cli.rs` | `4c3d80b9…` | `1723d320…` |
| `tests.rs` | `2362c8b8…` | `3dacc843…` |
| `owner_scm_tests.rs` | `ce98d0b8…` | `5cec6c74…` |

I verified this by converting `git show 974048e:<file>` and hashing it. The
content is equivalent and the source identity is confirmed. The manifest
header says "checkout bytes" but does not mention the line-ending
difference (advisory D-1).

## Verdicts

- **Standards: PASS.** ST-1 is closed. Only judgement-level and advisory
  notes remain.
- **Spec: PASS pending hosted gates.** One Low non-blocking test finding
  (SP-4a) and one advisory wording note on the clarification (C-1).
- **Coordinator clarification `c41fbb2`: ACCEPTED.** This is the
  cross-provider review it required.

PASS means the coordinator may open the PR for hosted verification. Merge
still needs all five hosted gates green on the exact head, including the
mandatory Ubuntu and macOS compile of the `cfg(unix)` paths. Native,
installed and dedicated paths remain **unverified on real hardware**, and M2
is incomplete.

## Coordinator clarification `c41fbb2` (SP-1 contract)

I checked each claim against the library code:

- **"A stale previous account surfaces as `service-identity`, exit 3,
  before workspace checks."** True.
  - `validate` (`config.rs:190-205`) runs `validate_inventory`. The client
    lookup there maps every error to `Identity`.
  - It then runs `execution_preflight`. Its build `AccountMissing` becomes
    `ExecutionUnavailable::AccountMissing`, and `validate` folds that to
    `Identity`.
  - Only then does it call `workspace_safe`.
  - `parse` and `revalidate` are unchanged by `c41fbb2..5cbb32a`.
- **"`service-account-missing` comes from new-account lookup in
  `from_request`, and natively only on Windows."** True.
  - `from_request` uses `res.account(..)?`, which passes the error through
    (`config.rs:116,120`).
  - Only `native_windows.rs:71-72` (`ERROR_NONE_MAPPED`) produces
    `AccountMissing`.
  - `native_unix.rs` and `native_macos.rs` have no `AccountMissing`.
  - The only `owner_unix.rs` hit (`:1477`) is a test fake.
- **"Preserves token set and remedies."** True. `error()`, the exit classes
  and the guide remedies are unchanged.
- **Brief test 5** now states the semantics that are actually implemented.
- **The guide account case** is added (closes prior SP-6). It names the
  token that actually appears, and says the separate token comes only from
  a new-account lookup.
- **Brief §6 `windows_start(preflight, start)` signature** matches
  `owner_cli.rs:285-291` exactly.
- **The ADR history note** follows the ADR's existing history convention.
  The parenthetical in decision 4, "(`service-identity` or
  `service-account-missing`)", stays readable as "the remedies apply to
  either token". The note above it governs which token can actually occur.

**C-1 (advisory, wording only).** On macOS, native `parse` first calls
`NativeResolver::revision` (`native.rs:35-38`). For a missing account,
`native_macos::revision` fails with `Identity` before `validate` runs. So the
mechanism the note gives ("folds execution preflight …") is exact for
Windows and Linux. On macOS the refusal comes one step earlier. The token,
exit code and "before the old workspace" claims hold on all three
platforms. The fake resolvers model the default `revision` of `Ok(None)`,
so tests cannot observe this. If the ADR is touched again, consider:
"surfaces as `service-identity` (revision or execution-preflight lookup)".
This needs no change before merge.

## Spec: disposition of every prior finding

**SP-1: CLOSED.**
- Unix (`tests.rs:536-656`) loops Linux and macOS:
  - `missing` asserts `Err(Identity)` exactly.
  - It asserts `error()` gives `("service-identity", 3)`.
  - The exact resolver log contains `account:old-builder` and no
    `workspace:/old`.
  - `executed == false`.
- Windows (`tests.rs:694-899`):
  - `missing` asserts `Error::Identity` and `("service-identity", 3)`.
  - The exact log is `["account:S-1-5-21-1-2-3-1001", "account:old-builder"]`
    with no `workspace:C:/old`.
  - Session calls equal `["read"]`.
- Library semantics are unchanged.

**SP-2: CLOSED.** The exact Unix resolver order now separates the full
previous parse (old client, old builder, old workspace) from the later
`configure_plan` revalidation. I re-ran the `unix-inventory` mutant (Unix
old `parse` → `parse_inventory`): killed at `tests.rs:631`, with a different
call sequence (`40efe97b-9ccc-4d59-addc-c46b2ab41c79`, exit 100). The
builder's `unix-none` kill is credited. The success path keeps the previous
bytes as the expectation, and the only agent is `test`, so nothing is merged.
As the evidence states honestly, no dedicated previous-agent-merge mutant
was run.

**SP-3: CLOSED for the required wiring cases.**
- **Changed pin.** Unix refuses on both layouts with no execute, and the
  call log includes the full revalidation before the pin comparison. Windows
  calls equal `["read","observe"]` and never configure.
- **New-SID observation.** `observe_batch` asserts the new SID `…-1002`.
  Only the changed-pin case tells it apart from the old SID (`…-1003`).
  - My `win-observe-old-sid` mutant passed the old identity when one exists.
    It was killed at `tests.rs:778` (`9c57aa57-857d-4fce-9eca-79839cc9bdd8`).
  - My `win-observe-before-parse` mutant reordered observe ahead of the full
    parse. It was killed at `tests.rs:867`
    (`a4f19f5e-7d65-4d0f-8abe-40454b63d60b`).
- **Recorded-grant mirror.** With `Store(true)` and `Right(true)` (a Granted
  record, present true), `adds_right == false` and `granted_batch_right ==
  true`. The not-present case gives `adds_right == true`, also with mirror
  true.
- The remaining provenance and status combinations are inherited library
  coverage (`owner_transactions_tests.rs`, `owner_plan.rs:444-447`). That is
  justified, and the evidence does not re-credit them.

**SP-4: CLOSED for the required bounds and forms, with one Low gap (SP-4a).**
- Accepted cases:
  - exactly 170 args (10 + 16×2 + 64×2);
  - exactly 4200 bytes in one argument;
  - exactly 32768 total bytes, and 32769 refused. The arithmetic is right:
    the remainder argument is about 1.3 KB, under the per-argument cap, so
    only the total cap can refuse it.
- The guide's Windows configure line (`service-install.md:126`) is
  reproduced exactly. It has backslashes, `;`, all four `--env` values and
  a filled placeholder.
- `--rights admin` is accepted. `A=b=c` keeps `b=c`.
- Each of the four required configure flags and both client flags is
  refused when missing. The odd tail is refused.
- The builder's three boundary kills are credited.
- My `parser-case-workspace` mutant (an `eq_ignore_ascii_case` arm) was
  killed (`2e203e2d-e151-4058-9a07-6d2be1226c08`). So was my
  `parser-prefix-workspace-v2` mutant (prefix match, length > 3)
  (`70f46cf9-3d6a-4c5d-b730-55ce7118ab41`). Both failed at `tests.rs:292`.

**SP-4a (Low, non-blocking; masked negatives).** `tests.rs:289-293` writes
every case/abbreviation flag into `a[8]`, the `--workspace` slot. For
`--build`, `--client` and `--right`, the argv is then both missing
`--workspace` and either duplicating or missing another flag. So it fails
for reasons unrelated to abbreviation.
- These compiled prefix-match mutants survived the whole
  `service::owner_cli` filter (10/10 passed, exit 0):

  | Mutant | Run |
  | --- | --- |
  | `parser-prefix-build` | `279e7d30-8c52-4e19-9910-12288c15334c` |
  | `parser-prefix-client` | `83928a39-dad1-4f69-80bc-279f374ab7d6` |
  | `parser-prefix-rights` | `969675bd-12b6-41aa-98ec-a402c9edc208` |

- Production is exact-literal matching (`owner_cli.rs:82-112`), so behavior
  is correct by inspection.
- The prior requirement ("`--Workspace` and abbreviated flags") is met by
  the `--Workspace` and `--work` kills. Three of the five listed cases are
  only non-discriminating. The evidence line "wrong case and abbreviated
  flags refuse" is true of production but overstates what the test proves.
- Correction test: replace the abbreviation in its own flag slot, keeping
  every other flag present: `a[2]="--build"`, `a[4]="--client"`,
  `a[6]="--right"`, plus the existing `a[8]` cases. Then re-run the three
  mutants above as kills.
- If this is folded in before merge, the `tests.rs`-only delta needs a
  cross-provider delta check. It is not a merge blocker.

**SP-5 / ST-1: CLOSED.**
- `OwnerOps::uninstall` now returns `Option<RightReport>`.
- The Windows NativeOps adapter returns `Ok(Some(report))` unchanged
  (`owner_cli.rs:372-379`). Unix returns `Ok(None)`.
- One pure `right_output` (`owner_cli.rs:204-206`) uses library `Display`,
  and the shared `dispatch` calls it.
- All five tokens are asserted three ways: against literals, against
  `to_string()`, and through `RecordingOps`/`dispatch`, with exact stdout,
  empty stderr and calls `["uninstall"]`.
- My `right-output-swap` mutant was killed
  (`67f6bfd5-5f74-4754-9505-f83603c5d1c5`). The builder's callsite-literal
  kill is credited.
- The adapter passthrough is still `cfg(not(test))` and source-only. It is
  now trivial.

**SP-6: CLOSED.** Covered by `c41fbb2` (above).

**SP-7: CLOSED.**
- `ConfigureArgs` derives `PartialEq`. `RecordingOps::configure` compares the
  full parse against `marker_args()`: build, client, admin, workspace, two
  `--env` pairs in order, and two principals and hashes.
- `request_forwards_all_marker_fields` compares all six request fields. The
  builder's forward-build, forward-env and forward-agents kills are credited.
- Every service, owner and client error goes through every fake verb (seven
  argv × 26 variants). Each asserts exact stderr and exit, and empty stdout.
- Client args are `test` and `/private/file`, not labelled markers. Exact
  stdout `test=<hash>\n` still proves the out path never leaks, which
  satisfies §7.2's intent.

**SP-8: ACCEPTED (brief §6 clarification ratified).**
- `owner_scm_tests.rs:776-799` asserts that the successful fake start's call
  sequence equals that of a direct `engine(true, STOPPED).start()`.
- A failed preflight makes zero SCM calls.
- The start closure asserts that the preflight ran first.
- Production `NativeOps::start` (`owner_cli.rs:347-359`) builds
  `WindowsScm::system` only inside the second closure, after
  `owner_start_preflight` and `drop(guard)`.
- **Source-only:** native guard release, pin release before SCM
  construction, SCM order and native callback selection.

**SP-9: ACCEPTED, unchanged.** `native.rs` has no delta since `edcadff`, and
its blob equals the manifest. The builder re-ran `native-inventory` and
`native-pins`, both restored kills; these are credited. Prior acceptance
stands.

## Standards

- **ST-1:** closed (SP-5).
- **ST-2:** closed. The service `Error` and owner `Failure` expected tokens
  are also compared with library `Display` (`tests.rs:468,481`).
- **ST-3 and ST-6:** deferred as advisories and stated honestly:
  - stale `dead_code` comments in `executor.rs` and `owner_scm*.rs`;
  - README width;
  - stdout write errors ignored with exit 0.

  These are out of this correction's scope. Fixing them would touch
  library modules or exit semantics.
- **ST-4:** closed. The test is renamed
  `native_entry_signature_compile_check_only`, and the evidence and PROGRESS
  say it supplies no behavioral coverage.
- **ST-5:** honoured. The correction touches only CLI output, tests and fake
  start evidence.
- **Judgement (possible Speculative Generality, minor).**
  `#[derive(PartialEq)]` on production `ConfigureArgs`
  (`owner_cli.rs:13`) exists only for the test comparison.
  `#[cfg_attr(test, derive(PartialEq))]` would keep the production type
  minimal. No change is required.
- No other smell. The test file's large inline `Res`, `Store`, `Right` and
  `Session` fakes are local to the one test that needs them.

## Docs accuracy

- **PROGRESS:**
  - There is exactly one `## Current state` heading. Before this change,
    two older `## Current-user …` headings also began with "Current". They
    are renamed `Historical current-user …`, with their bodies unchanged.
  - The previous current block is preserved verbatim under "Historical
    checkpoint before S3 correction attempt 1".
  - The queue (Opus recheck, then five hosted gates) is right, and so are
    last merged #49 and main run `43221533019`.
  - Per-task provider, effort and outcome are recorded, with
    **unverified on real hardware** and M2 incomplete.
- **Evidence:**
  - The red history is honest: Focused1 failed to compile with no run ID,
    Focused2 ran 9 of 10 with one fixture failure, and there was no
    production fix.
  - The focused 14/285 result matches my run.
  - D-1: the manifest's line endings, above.
  - The SP-4 abbreviation claim is overstated (SP-4a).
- **ADR, brief and guide:** accurate, apart from C-1.

## Checks run by this reviewer

All checks ran in an owned scratch `git archive 5cbb32a` export with its own
`CARGO_TARGET_DIR`. Nothing was edited in the worktree. Only `owner_cli.rs`
was mutated. A script restored the saved bytes in `finally` and asserted
SHA-256 equality after every mutant. The final export hash `1723d320…`
equals the committed blob.

**Never used:** production NativeOps, `create`, `ClientConfig::load`,
OwnerGuard, a session, the native preflight, a store, `NativeResolver`,
fixed system paths, a pipe, SCM, LSA, accounts, privileges, services, a VM,
Lab Manager, private state or an ignored fixture. No valid owner argv ran.
Only fakes and synthetic values were used.

- **Focused run** `c23cbabe-e3b9-4bec-bb88-a6721ba9142e`, exit 0: 14 passed,
  285 skipped. It used the evidence's Focused3 filter. The binary test spawns
  only invalid argv.
- **Mutants**, all compiled, sequential and restored:

  | Mutant | Run | Exit | Result |
  | --- | --- | --- | --- |
  | `unix-inventory` (re-run) | `40efe97b-9ccc-4d59-addc-c46b2ab41c79` | 100 | killed `tests.rs:631` |
  | `win-observe-old-sid` | `9c57aa57-857d-4fce-9eca-79839cc9bdd8` | 100 | killed `tests.rs:778` |
  | `win-observe-before-parse` | `a4f19f5e-7d65-4d0f-8abe-40454b63d60b` | 100 | killed `tests.rs:867` |
  | `right-output-swap` | `67f6bfd5-5f74-4754-9505-f83603c5d1c5` | 100 | killed `tests.rs:330` |
  | `parser-case-workspace` | `2e203e2d-e151-4058-9a07-6d2be1226c08` | 100 | killed `tests.rs:292` |
  | `parser-prefix-workspace-v2` | `70f46cf9-3d6a-4c5d-b730-55ce7118ab41` | 100 | killed `tests.rs:292` |
  | `parser-prefix-build` | `279e7d30-8c52-4e19-9910-12288c15334c` | 0 | **survived** (SP-4a) |
  | `parser-prefix-client` | `83928a39-dad1-4f69-80bc-279f374ab7d6` | 0 | **survived** (SP-4a) |
  | `parser-prefix-rights` | `969675bd-12b6-41aa-98ec-a402c9edc208` | 0 | **survived** (SP-4a) |

- **Not counted:** my first `parser-prefix-workspace` attempt
  (`ad8a8abc-e62c-4c33-94b6-a3d3d97cb7f6`, exit 0). Its guard (length > 6)
  excluded `--work`, which is 6 bytes, so it was an invalid mutant, not a
  survivor. The `-v2` row above replaces it.
- I ran no full suite, stress, retry, instrumentation, `service run`
  fixture, Clippy or fmt.

## Credited, not re-run

- Builder evidence at `974048e`, credited to its pins:
  - fmt, `git diff --check` and locked workspace all-target Clippy;
  - focused `ade72ec9…` (14 passed);
  - 11 restored compiled assertion kills;
  - exactly one normal full run `1dfe22f1-d91c-4062-aa3d-51bae09aa60a`:
    298 passed, 2 skipped, 10.483s.
- The 298 count includes the compile-only check and is not contract
  acceptance.
- Main #49 run `43221533019` (green, 439s) is coordinator evidence. I did
  not check it.
- E2's natural ExecutionFailed (`bdbf032b`) remains open and unexplained,
  with no waiver. A green run neither fixes it nor measures its rate.

## Limits

- **Unix `cfg(unix)` code was not compiled locally.** That covers Unix
  NativeOps, `unix_context` and the Unix non-UTF-8 branch. `unix_configure`
  and `unix_verb` compile under `cfg(any(unix, test))` and were exercised. I
  confirmed by inspection that the unconditional `RightReport` import is
  used on Unix through the trait. Hosted Ubuntu and macOS are mandatory.
- All five hosted gates on the exact published head are outstanding.
- **Source-only review:** the NativeOps compositions. That covers guard
  ordering, the start guard and pin release before SCM construction,
  configure session pins, the uninstall proof, adapter passthrough and
  native client creation.
- Installed, native and dedicated paths are **unverified on real hardware**.
  M2 is incomplete.
- After three failed fixes, the work goes to the other provider with the
  failure evidence.
