# S3 correction attempt 1 evidence

OpenAI / gpt-6.1-sol / high, 2026-11-12; implementation sub-agent, no subagents.
S3 is unpublished and pending a fresh independent Anthropic / Opus 5.5 / high
recheck, including coordinator Sol medium's c41fbb2 contract clarification,
and all five hosted gates. No acceptance or activation; M2 is incomplete.
Native, installed and dedicated paths are **unverified on real hardware**.

## Worktree and retained pins

Runtime-prepared worktree: `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-784b56b1-bfcc2b1e10a1241f`.
Initial pwd and Git top-level matched, HEAD was clean at
`c41fbb2cd972f19e3af7af81f0f0b06342f42a0d`, and the path differed from
canonical `D:/Code/Arxcovo`. No checkout/worktree creation or canonical write.
The initial verification used the runtime cwd; subsequent shell commands set
explicit workdir. Completed commands' exit results were inspected; long-running
commands used their returned sessions until completion.

Retained candidate source `edcadff70d20c6a5f6847679b9dea3b468ef1802`, evidence
`558b94895de98938ceb649ab73f3feaa54fd1fdd`, full Opus review `91b0cc3`
retained through `02499d1`, and FIRST invalid-install safety fixture `5c57208`.
Read the complete review, AGENTS, KICKOFF, ADR0018, wiring brief and S3 evidence.
Source commit: 974048e621edc8c468c9f62943a749bfb0218ddb. Evidence/progress commit follows; its own SHA is
identified by Git history (no recursive self-pin). No review report was edited.

## Finding dispositions

- SP-1: c41fbb2 docs preserved unchanged. Both Unix layouts and Windows assert
  exact Identity and service-identity/exit3; old-builder lookup is recorded,
  old workspace lookup absent. Windows calls only read; Unix never executes.
  Existing full parse/revalidate semantics remain unchanged.
- SP-2: exact Unix resolver order distinguishes full previous parse from a later
  configure_plan revalidation. Success retains previous bytes as expectation,
  and exactly the new agent, without merging. Compiled Unix inventory and
  old-None mutants fail assertions. No actual previous-agent-merge mutant was
  run; no such kill claimed. Build/environment/agent request-forwarding mutants
  separately kill actual field mapping, not a copied test mapping.
- SP-3: changed build pin refuses on both Unix layouts before execute; Windows
  calls exactly read/observe, never configure. Session observation checks the
  NEW synthetic SID. A Granted-record/present=true case asserts adds_right=false
  and serialized mirror=true. Exhaustive provenance/status combinations remain
  inherited library coverage; not duplicated or recredited as new tests.
- SP-4: parser-only accepted boundaries are exactly170 args, exactly4200 bytes
  for one argument, exactly32768 total bytes; the latter refuses one extra byte.
  The actual Windows guide configure form fills both placeholders and includes
  its backslashes, semicolons and all four env pairs. Admin and A=b=c acceptance
  preserve b=c. Each required flag missing (including client form), odd tail,
  wrong case and abbreviated flags refuse. No valid owner argv is spawned.
- SP-5/ST-1: OwnerOps returns an optional RightReport, so the native adapter
  passes the enum unchanged. One pure right_output uses library Display;
  shared dispatch calls it. All five exact tokens equal to_string and traverse
  RecordingOps/dispatch. Replacing the dispatch callsite with a literal is a
  compiled assertion kill. NativeOps is excluded from test execution; changing
  its enum return/production adapter selection still requires source review.
- SP-6: coordinator account remedies/actual Identity guide text retained.
- SP-7: every ConfigureArgs field has distinct synthetic marker data, including
  two env pairs, two principals/hashes and admin rights. RecordingOps compares
  the full parsed value; request fields are separately compared in full.
  Marker principal/hash grammar and lengths are checked. Every fixed error
  variant traverses every fake verb, asserting exact stderr/exit and no stdout.
- SP-8: deferred closure retained. Successful fake start calls equal a direct
  PR-C engine(true, STOPPED).start sequence; failed preflight calls nothing.
  Native guard drop, pin release before SCM construction and SCM order remain
  source-only, not fake execution evidence of native behavior.
- SP-9: read_config_with unchanged; actual inventory-parser selection and
  retained-pin mutations are rerun and restored, with assertion kills below.
- ST-2 advisory: service Error and owner Failure expected tokens are additionally
  compared with library Display. No broad token refactor.
- ST-3/ST-6 advisory: stale dead-code comments in out-of-scope executor/SCM modules
  and README width remain deferred; no library/executor source delta. Ignored
  stdout write failures assessed and left unchanged: changing exit semantics or
  adding a failure class is outside this correction's contract.
- ST-4: renamed function-pointer test as a compile check only. It is included
  in runner totals but supplies NO behavioral/native coverage.
- ST-5: this correction isolates owner CLI tests/output and fake start evidence;
  prior native-reader source/evidence remains attributed to its original pins.

## Checks and failures

- Initial cargo fmt --all passed. The first focused attempt did not launch:
  target log directory absent; directory created inside this worktree only.
- Focused1: compilation failed, exit101, no Nextest run ID: attempted Copy/Clone
  on CliError was invalid because ClientError lacks those traits. Removed it;
  test loops reconstruct the error matrix per verb. S2 type is unchanged.
- Focused2 ID `22046247-ba3a-49de-8447-47c16250d83b`, exit100: 9 passed/1 failed.
  Total-byte fixture exceeded the separate4200 argument cap. Redistributed
  total bytes across eight3900-byte payloads and a remainder; no parser change.
- Focused3 `cargo nextest run --locked -p arxd -E
  'test(service::owner_cli) | test(cli_start_preflight) | test(native_read_parser)
  | test(u1_uninstall_flow) | (binary(service) & test(service_run_requires_exact_arguments))'`:
  `ade72ec9-ad7f-4186-91ae-495874c9f2c2`, exit0, 14 passed/285 skipped.
  Logs target/s3c1-focused{,2,3}.log. No native owner calls.
- Final cargo fmt --all -- --check, git diff --check, and
  cargo clippy --locked --workspace --all-targets -- -D warnings passed (exit0).
  Clippy finished in27.09s; log target/s3c1-clippy.log.

## Restored compiled assertion mutations

Every command: `cargo nextest run --locked -p arxd -E 'test(FILTER)'`.
Filters: unix_full_previous, parser_exact_bounds, request_forwards,
right_reports or native_read_parser according to the named mutation.
Each listed run compiled, selected real tests, exited100 with a runtime
assertion failure and restored saved bytes in finally, checking byte equality.
No noncompiling mutant or survivor counted. Logs target/s3c1-mut-NAME.log;
local manifest target/s3c1-mutations.json. Mutants ran sequentially; final normal
suite starts only after every restoration. No instrumentation or custom env.

| Mutation | Run ID | Exit / outcome |
| --- | --- | --- |
| unix-inventory | `35ee2447-4114-4c99-992a-952401ac1b22` | 100; compiled assertion; restored |
| unix-none | `6ffccd69-cba0-43c3-973a-8d7ffd73ff0e` | 100; compiled assertion; restored |
| count-inclusive | `b397daed-b4e7-4200-a575-a125f4dc9bba` | 100; compiled assertion; restored |
| arg-inclusive | `21077632-9332-4637-8ccc-f1841774e5ae` | 100; compiled assertion; restored |
| total-30000 | `d45c2b2e-d8ca-44f8-a31d-54be64bc0cc7` | 100; compiled assertion; restored |
| forward-build | `6ec40fc0-1732-413c-b6b5-11b3d825563f` | 100; compiled assertion; restored |
| forward-env | `a093355c-4827-41ee-92cf-5a480808ecba` | 100; compiled assertion; restored |
| forward-agents | `5269091a-88da-4d0e-afc3-1a5a763ec61d` | 100; compiled assertion; restored |
| right-dispatch-literal | `8f7a6db8-8517-46a1-bba6-af2b48028d09` | 100; compiled assertion; restored |
| native-inventory | `7af5f540-17ec-4526-9f13-0150bd0c2de5` | 100; compiled assertion; restored |
| native-pins | `b221543b-6894-4185-8392-df6240016a83` | 100; compiled assertion; restored |

## Historical checkout byte manifest (SHA256)

- `crates/arxd/src/service/owner_cli.rs`: `4c3d80b9b45be7ca539a5fe1c0201f2cac73c5e33f90499253e0f0049159fb39`
- `crates/arxd/src/service/owner_cli/tests.rs`: `2362c8b8dc30acf32f3c832c07e6662dce80ce57e481d67da3c0512ee5ee78fb`
- `crates/arxd/src/service/owner_scm_tests.rs`: `ce98d0b85ffdb11c28e7b4bc7fa82a321400f8f9687611dd0f6032019f9a26c9`
- `crates/arxd/src/service/native.rs`: `7631b2f4b1faf221882c0857f60c90b0412758b3323569a6bd1d9de2dfdee4f8`
- `crates/arxd/src/service/config.rs`: `e5c0df0fa5bad1b85b81c8a360e7fad1166c71a1aa55f0c72f0a0350beab7f03`
- `crates/arxd/src/service/executor.rs`: `0b81f1b6a68f78babc59fe7501a2b0c636f5b989e2df99fc134003b857086949`
- `crates/arxd/src/client_config.rs`: `e0acffe09e606dc05a1a06e1231d08af52d8210cb06930a898f75a1985ebcc8c`
- `crates/arxd/tests/service.rs`: `91bf603533b4219c89e0b12216efc796399afb4eb5a82fb0c59b5871a00b776e`

Mutated CLI restoration hash matches this manifest; native restoration hash
matches the original S3 manifest. Only three source/test files changed.
No S1/S2 library, native, executor, schema, dependency, CI or bridge changes.

## Final normal full suite and limits

Source `974048e621edc8c468c9f62943a749bfb0218ddb`: run `1dfe22f1-d91c-4062-aa3d-51bae09aa60a`, exit0,
298 passed, 2 skipped, 10.483s. No natural ExecutionFailed; no repeat full run.

Exactly one final `cargo nextest run --locked --workspace`, after the last
source change and all mutant restorations, default profile/concurrency,
no retries/stress/instrumentation/custom env. Log target/s3c1-full-final.log.
No local Unix cfg compilation, toolchain installation or stub credit. Hosted
Ubuntu/macOS compilation and all five exact-head gates remain pending.
Historical E2 natural ExecutionFailed bdbf032b remains unexplained; no pass is
its fix, a failure-rate claim or waiver. Any new natural ExecutionFailed would
stay red with its ID/unknown phase and no blind rerun.

All valid commands used RecordingOps or fake composition resolvers/sessions/
stores/rights/SCM. No production NativeOps, credential create, ClientConfig::load,
OwnerGuard, native session/preflight/store/resolver, fixed system path/pipe,
account/privilege/service/SCM/LSA/VM/Lab Manager operation was invoked. No private
state or ignored fixture was read; ordinary benign owned-temp full-suite
fixtures only. No private data sent externally; no GitHub/push/PR/watch/merge/rerun/settings,
babysitter, owner admin, installation or toolchain action. No subagents.

## Publication manifest clarification

The three changed-file hashes above describe the builder's CRLF checkout
copies, not the LF Git blobs. The retained Opus recheck verified that converting
the committed files to CRLF reproduces them. The eight committed LF SHA-256
values at source 974048e are listed below; these are coordinator Git-object
reads, not a fresh test result.

- `crates/arxd/src/service/owner_cli.rs`: `1723d3206d9005fb65a1ffc7f556ec37d0637e6402525de6e7c5639af20cb962`
- `crates/arxd/src/service/owner_cli/tests.rs`: `3dacc84334956ff54075376ba43a286b623f350f8b40f19d95b2e9f214b271af`
- `crates/arxd/src/service/owner_scm_tests.rs`: `5cec6c74ae2c152e0cfca578004f5cf301fb5a201a056ee018a4f96dc5fe8603`
- `crates/arxd/src/service/native.rs`: `7631b2f4b1faf221882c0857f60c90b0412758b3323569a6bd1d9de2dfdee4f8`
- `crates/arxd/src/service/config.rs`: `e5c0df0fa5bad1b85b81c8a360e7fad1166c71a1aa55f0c72f0a0350beab7f03`
- `crates/arxd/src/service/executor.rs`: `0b81f1b6a68f78babc59fe7501a2b0c636f5b989e2df99fc134003b857086949`
- `crates/arxd/src/client_config.rs`: `e0acffe09e606dc05a1a06e1231d08af52d8210cb06930a898f75a1985ebcc8c`
- `crates/arxd/tests/service.rs`: `91bf603533b4219c89e0b12216efc796399afb4eb5a82fb0c59b5871a00b776e`
