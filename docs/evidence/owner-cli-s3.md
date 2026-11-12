# Owner CLI S3 local candidate evidence

OpenAI / gpt-6.1-sol / high, 2026-11-12. No subagents. This is an implementation
candidate, not acceptance. Independent Opus high security/source review and all
five hosted gates are REQUIRED before publication. No public GitHub operation,
push, PR, watch, rerun, merge, settings or babysitter change occurred.

## Isolation and pins

Authoritative runtime-prepared worktree:
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-9f96802c-2cec207100e6cdfe`.
Initial pwd and Git top-level matched, HEAD equaled exact clean base
`46f3a9ffc42983216ae55a4be27350a40f43580a`, and it differed from canonical
D:/Code/Arxcovo. Every shell call used explicit workdir. No checkout/worktree
creation or canonical-root write. Base inherits merged #49
`c4ef8ada086f6595a6a83c38cc77b34f87ebbd9f` and FIRST S3 safety commit
`5c5720882d9b6ce5749aa14df5e3e2fc4a215230`, which was preserved. Its prior
1/1 f703573a fixture pass is coordinator evidence, not re-executed here.
Source/evidence pin: `edcadff70d20c6a5f6847679b9dea3b468ef1802`. Documentation-only evidence commit follows.

## Implementation and authority boundaries

The private non-Debug parser bounds OS argument counts/lengths before conversion,
rejects non-UTF-8 and malformed grammar, preserves repeated values, and routes
reserved prefixes before Tokio. Exact service run and foreground paths remain.
The existing invalid install fixture now expects fixed `arxd: usage`; added
client-prefix fixtures are invalid too. No valid owner argv is spawned by tests.
OwnerOps dispatch uses static error tokens, exact exit classes, status/right
output and principal/hash success; Partial has no stdout/hash.

NativeOps exists only outside cfg(test). Production credential create is wired
but never called in tests; a function-pointer reference avoids test-build dead
code without invocation. client_config remains private in lib.rs; its sibling
items already had sufficient crate visibility. Only the stale unused allowance
and module comment changed there. S2 platform code/tests remain unchanged.

Windows install/configure/start/stop/status/uninstall acquire OwnerGuard first.
Configure uses ConfigureRequest/from_request, the pinned session and FULL prior
parse, then opaque batch observation, configure_plan/from_plan and configure.
Uninstall calls only proof-consuming uninstall_flow/session uninstall after the
SCM phase. Unix owner checks precede layout/context and use owner_plan for verbs,
including uninstall without config. macOS Unknown is still a native refusal.

Private start-seam refinement: windows_start takes a deferred start closure
instead of an already-created SCM reference. The production closure constructs
WindowsScm::system only AFTER native preflight returns and drops the image guard;
the fake closure calls the existing fake SCM. This preserves the required native
order without parallel production logic or added authority. It is a private
composition signature difference from brief section 6, requiring review, not a
new service/guard/session architecture.

S1 ST-1: native read_config now shares its ACTUAL acquisition-and-parser path
through read_config_with, rather than an inventory-only wrapper disconnected
from the caller. The native acquisition returns bytes AND held pins; the pins
remain live through parsing. The fake reader/counting resolver proves no build,
revision or workspace lookup and asserts pin lifetime. Mutating this real parser
selection to full parse fails a compiled assertion. No native reader ran.
Native callback selection, guard acquisition, installer registration order,
Windows session pin behavior, SYSTEM access and state sharing remain source-only
at this boundary; no synthetic test is represented as native execution proof.

## Blocking discrepancy and residual coverage

The accepted brief expects AccountMissing for a previously missing build account.
Existing ServiceConfig::validate maps execution_preflight AccountMissing to
Identity. The new missing-old-build fake therefore observes Identity (exit 3),
with no execute; changing this library was explicitly outside S3's preserve-S1
scope. This discrepancy MUST be resolved by the coordinator/reviewer before
claiming full contract conformance. Missing old workspace is Workspace (exit 3),
with full previous workspace observation and no mutation on Windows or Unix.
No inventory bypass, skip-old-config or merge workaround was added.

Coverage is not exhaustive contract acceptance: the new Windows recording session
covers a missing grant and stale/unparseable prior config; recorded-grant mirror
combinations and changed pins retain library coverage rather than a new full
CLI wiring matrix. New dispatch success coverage uses running and one right
report; other status mappings retain existing SCM/query tests. Unix native paths
were neither compiled nor executed locally. Hosted Ubuntu/macOS are mandatory.
Unix uninstall_plan substitution and previous-agent merge mutants were not run
in this slice; no kill is claimed. Inherited S1/S2 evidence/mutations remain
credited only to their recorded source pins; S2 native Mode and intermediate-pin
limitations remain unchanged. Additional review/coverage may still be required.

## Commands, failures and checks

- Initial cargo fmt --all completed; the first combined Clippy failed on an
  unused Windows Plan import and missing test references. A later Clippy failed
  on an incorrect test import path. Corrected; locked workspace all-target
  Clippy subsequently passed. No install/toolchain/compiler change.
- One Python documentation edit failed decoding a UTF-8 file using the Windows
  default codec. Later edits use explicit UTF-8 byte writes. Diff review found
  and corrected legacy-character mojibake in native device predicates and guide
  text before final validation; original predicates are preserved.
- Focused command: cargo nextest run --locked -p arxd -E
  'test(service::owner_cli) | test(cli_start_preflight) | test(native_read_parser)
  | binary(service)'. Run `61072229-928b-4fdf-9fdd-97cee2df8847`: 10 pass,
  1 assertion failure (missing-old-build fixture did not inject at the name
  lookup). Log target/s3-focused.log. No ExecutionFailed.
- Focused command adding test(u1_uninstall_flow), same other filters:
  `5dbdd530-0b76-4776-85cd-05807eac9759`: 11 pass, 1 assertion failure:
  actual Identity versus expected AccountMissing. Log target/s3-focused2.log.
  Inspection established the existing library mapping; no production fix made.
- Corrected expectation: `4c627d51-663d-4fa3-99ce-c3e9119428a2`: 12/12.
- After invalid client fixtures and retained-reader-pin assertions:
  `53bf5075-d183-45e4-86f2-398e2b48a447`: 12/12, 284 filtered.
  Logs target/s3-focused3.log and target/s3-focused-final.log respectively.
- Final source fmt, diff whitespace check and
  cargo clippy --locked --workspace --all-targets -- -D warnings pass.
  Clippy log target/s3-clippy-final.log. Final full nextest result is below.

## Restored mutations

Each command was cargo nextest run --locked -p arxd -E 'test(FILTER)'.
Filters: exhaustive_errors for permissions/error-output; parser_matrix for
syntax/dedup; windows_configure_fully for old config; cli_start_preflight for
start ordering; native_read_parser for inventory/pins; u1_uninstall_flow for
deregistration. Every listed run compiled, selected a real test, failed a runtime
assertion (exit100), and restored captured bytes in finally with byte equality.
No noncompile attempt is counted. Logs are ignored target/s3-mut-NAME.log;
JSON manifests target/s3-mutations.json and target/s3-mutations-final.json.
The first nine precede final comment/fixture and retained-pin/start-seam edits;
the four final runs target the committed source. No owner operation was invoked.

| Mutation | Run ID | Result |
| --- | --- | --- |
| permissions | `ac2451f2-0ebd-42b9-b52f-4ddcc0f52283` | 100; compiled assertion; restored |
| equals | `fcecd24d-ae41-4103-ab3c-e2c595cdcdf2` | 100; compiled assertion; restored |
| dedup | `a8261f40-0892-4d17-984b-7b1861bca374` | 100; compiled assertion; restored |
| error-output | `977d71bd-7b8c-4748-a36e-8de07937290a` | 100; compiled assertion; restored |
| old-inventory | `5a960a4c-049b-4e08-873b-451b64eb093c` | 100; compiled assertion; restored |
| old-none | `93376525-f007-4f78-a67c-1fc06f97fc4d` | 100; compiled assertion; restored |
| skip-preflight | `9e66f82c-a7b1-43cf-984d-808d6002655e` | 100; compiled assertion; restored |
| native-inventory | `0ae602bb-467e-4c14-bc9b-74b7d7db693d` | 100; compiled assertion; restored |
| deregister | `432e6ab5-b911-4faf-8cb8-49867afb67dc` | 100; compiled assertion; restored |
| final-skip-start | `d26f8494-3770-4f08-bac5-ebbf21ae0eb0` | 100; compiled assertion; restored |
| final-start-order | `a262c074-8237-46ee-974f-dca79d72215b` | 100; compiled assertion; restored |
| final-native-inventory | `5ad535cd-5670-4030-99ae-261ed8533c3f` | 100; compiled assertion; restored |
| final-read-pin-release | `783df268-d1d9-4aa0-9069-079b9cae0881` | 100; compiled assertion; restored |

Final restored owner_cli SHA256:
`3ea27f2727276e0fc357c07c6de03569ac8fe258381d5e02b5615604b9df5c48`.
Final restored native SHA256:
`7631b2f4b1faf221882c0857f60c90b0412758b3323569a6bd1d9de2dfdee4f8`.

## Source manifest (checkout bytes, SHA256)

- `crates/arxd/README.md`: `d9ca313496f0de2fb7044e68ddeb814a6afb04db2a30e1089a643efa41a60f0f`
- `crates/arxd/src/client_config.rs`: `e0acffe09e606dc05a1a06e1231d08af52d8210cb06930a898f75a1985ebcc8c`
- `crates/arxd/src/main.rs`: `8fe8dae820eec3113684c793d6de149b234ff4e898f291a06f0fd21fcf17aa5f`
- `crates/arxd/src/service/config.rs`: `e5c0df0fa5bad1b85b81c8a360e7fad1166c71a1aa55f0c72f0a0350beab7f03`
- `crates/arxd/src/service/mod.rs`: `c79aa3cf841354726d79e8bd330772429dbace3887c577f1aaa99429f838ef47`
- `crates/arxd/src/service/native.rs`: `7631b2f4b1faf221882c0857f60c90b0412758b3323569a6bd1d9de2dfdee4f8`
- `crates/arxd/src/service/owner_cli.rs`: `3ea27f2727276e0fc357c07c6de03569ac8fe258381d5e02b5615604b9df5c48`
- `crates/arxd/src/service/owner_cli/tests.rs`: `d2fc58e75ff5bef1a8e4ce87c3152c592daa7b9253e241d321adc743da122fb4`
- `crates/arxd/src/service/owner_install_windows.rs`: `6b7711a1d835503d7517bb80abb60ab041ba9520dc17969912b3e7828ca93df5`
- `crates/arxd/src/service/owner_scm.rs`: `277696c1aa15264654bc6f77ef7ac27222eceeec2f302b247b007345b3987ed1`
- `crates/arxd/src/service/owner_scm_tests.rs`: `d12fbd81c87b343ccd81be2d5e13eeecb7866a31d61b853a123bdaf3da3aacad`
- `crates/arxd/src/service/owner_unix.rs`: `7d3edcc42d04fddff8011cc395c885873369bf0d56b62d9e95c68fc0b6754b53`
- `crates/arxd/src/service/owner_windows.rs`: `399ac58e1952f20d95fea09f7174eb29f7757148f15f1b70dacb6bbfe3560bf5`
- `crates/arxd/tests/service.rs`: `91bf603533b4219c89e0b12216efc796399afb4eb5a82fb0c59b5871a00b776e`

## Final full run and remaining gates

Source `edcadff70d20c6a5f6847679b9dea3b468ef1802`: run `e2e5f587-88a4-4103-a8b6-74e41f98a857`, exit 0.
Summary [  10.243s] 295 tests run: 295 passed, 2 skipped. Log target/s3-full-final.log.
All 295 runnable tests passed; two existing ignored tests were skipped.
No natural ExecutionFailed occurred; no repeat full run was made.

Exactly one final normal cargo nextest run --locked --workspace, default profile
and concurrency, no instrumentation/custom environment/stress/retry. A natural
ExecutionFailed would remain red with its run ID and unknown phase retained;
there is no blind rerun. Prior bdbf032b remains unexplained: no pass is a fix,
rate claim or waiver. Main CI after #49 remains pending in supplied evidence.
No local Unix success or hosted S3 result is claimed. Native, installed and
dedicated paths remain **unverified on real hardware**; M2 is incomplete.
No owner/admin/account/SCM/LSA/service/native-preflight/session/store/resolver,
fixed-system-path/fixed-pipe/VM/Lab Manager or ignored-fixture operation was added
or executed. Existing ordinary workspace fixtures alone remain authorized.
No private-state file was read. Rollback is a reviewed source revert; there is
no installed state to undo. Fresh Opus high review must address the discrepancy,
private start seam and residual coverage before any publication, followed by
all five hosted gates. The coordinator owns that next step.
