# ADR0019 Stage D local implementation evidence

OpenAI / gpt-6.1-sol / high, 2026-11-12. Implementation child; no delegates.
**Ready for independent review, not acceptance.** Fresh Anthropic / Opus 5.5 /
high product/security review is required before publication. No publication,
GitHub write, PR, watch, merge, CI investigation or owner operation occurred.
M2 remains incomplete. Native, installed and dedicated paths remain
**unverified on real hardware**. ExecutionFailed / E_PROTECTION remain
unexplained; no fix, waiver, frequency claim or retry is inferred from passes.

## Pins and scope

- Prepared worktree `<prepared-worktree>`: initial pwd equals Git top-level,
  clean at exact base `7b2cfa1cd00d0f3dc657513dd07bc684739cc1e1`, distinct
  from the canonical checkout. No checkout creation or canonical write.
- Explicit unmerged dependency: reviewed Stage C PR53 source
  `2ce79caf9ded55976e3e83b2c49b41d54160f2da`. Actual merged main remains
  Stage B PR52 `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`.
  Dependency review/hosted credits belong to those heads, not Stage D.
- Final source commit `d3615be3b9820cf72a40b321a3dcf6c746a9e7bd`.
  Evidence is committed separately after validation; PROGRESS records that
  evidence head, preserving this source pin.
- Only new bridge/tests, executor test seams, module declaration, Stage B
  scanner extension and progress/evidence changed. Removing the added
  cfg(test) executor methods yields the exact base executor text. In
  particular configured/native, dispatch, assess and output release are
  unchanged. Existing B/C runtime and assertions, owner CLI/SDK, dependencies,
  schema, workflow/policy, runner architecture and native composition remain.

## Implementation and test coverage

- T1: pure config derivation, exact private directories and environment,
  nonexistent synthetic workspace, account-name and accepted config/runner
  limit mismatches, Linux standard/platform/admin refusals, one factory call,
  repeated check -> stored construction refusal -> readiness ordering.
  Inputs rejected by ServiceConfig cannot be forged through its private fields;
  existing config tests retain those invalid UID/schema cases. The factory is
  injected only in cfg(test); tests never construct/call a real Runner.
- T2: bounded UTF-8 supervisor parser, one v2 line, missing/wrong leaf, root-only,
  v1, extra lines, NUL, dot components and oversized input. No proc file read.
- T3: fixed installed binary, exact path/tool argv and fidelity (-x, --, empty,
  Unicode), empty runner cwd, request timeout/output, fixed 60s/4096 discovery.
  128x512 tail with 4096-byte target/cwd checked through fake port. The real
  phased validator's item/byte assertions remain in Stage B and pass focused
  verification; fake Invalid refusal is separately checked through report mapping.
- T4: exhaustive typed error classes through real dispatch and activity rows,
  quarantine, no capture keys and finished rows; independent exact Report
  assertions distinguish NotInvoked/Unknown/Created without assess as oracle.
  Invalid/Resource are NotInvoked, per accepted brief section 3 and ADR0017:
  no primitive was called, so ProvenNone would invent a qualifying guarantee.
  Other NotStarted classes have their fixed refusal/cancellation reports.
- T5: N/M/L/H/K/I, timeout's incidental Some(0) withheld, exited-without-code
  quarantined, cleanup copied unchanged, mismatched synthetic IDs withheld,
  process-group boundary never promoted to MacUidSweep. Captures move unchanged;
  dispatcher alone controls release, including release on matching cleanup failure.
  Dropped-byte counts remain a lower bound: unread final-drain bytes are not counted.
- T6: JSON parsed by keys rather than serialized key order (shim serde keys sort),
  exact mandatory name/found/version fields, ordered complete TOOLS, UTF-8,
  dropped bytes, extra/missing/reordered rows, unknown/duplicate/missing fields,
  empty or path-bearing versions, found=false/version inconsistency, nonzero exit,
  timeout and identity mismatch. Invalid discovery is empty tools -> row K;
  stderr stays unread. Tools only leave on dispatcher row N.
- T7: caller cancellation reaches a blocking fake, dropped request cancels,
  drain passes shutdown and joins, pre-spawn fake cancellation is E, per-run
  drop never cancels caller shutdown. Real runner owns its independent shutdown
  token (unchanged Stage B); no real Runner is built or dropped by these tests.
- T8: synthetic marker account/workspace/environment/cgroup/argv, all phases;
  response, activity and owned database-file byte checks. Matched/mismatched
  synthetic UID/GID 3141592653 plus marker output are checked without printing.
- T9: test-only native composition with valid synthetic Linux/macOS/Windows
  pins and successful fake preflight closure remains MechanismUnavailable,
  including direct preflight assertion and dispatch. Source guard preserves
  configured -> native; no production configured constructor/NativeResolver runs.
- T4b/P7: run catches spec derivation, port call, mapping and discovery branch
  together. Test-only fixed mapping/discovery seams cause finished G rows.
  Payload is dropped unread, with no service hook, formatting, rethrow or prints.
- T10: only normal parent reexecutes purpose-built ignored fixture, exact name,
  --ignored --nocapture, cleared environment plus fixture flag and owned temp
  variables. Runs with backtrace unset/set, checks both output streams, all typed
  classes, matched/mismatched synthetic IDs, fixed runner/mapping/discovery panics
  exactly once, and visible negative-control payload. Child exits successfully.
  No installed/service/operational panic data or host-identity test is added.

## Direct F2 audit and scanner limits

Audited bridge plus exec mod/unix/cgroup/output and the unchanged shim directly.
No indirect/UFCS/function-value/raw unwrap, diagnostic import alias, RunFailure
From/type/use alias, stderr writer alias, hidden diagnostic macro expansion or
service hook alias in the actual reachable production source. Existing shim's
one hook is separate-process only. Its JSON macro serializes fixed string/bool/
Option<String> fields; its stdout write is the intended probe protocol. Existing
cgroup formatting is numeric internal names/limits, never a private panic payload.
The bridge has no formatting, stream writes, string range indexing or implicit
From conversion. Its only source panic literals are cfg(test) mapping/discovery
seams, inventoried explicitly. Constructor/preflight remain outside the per-run
catch; no child can exist there.

P-SCAN includes the bridge. It rejects diagnostic import aliases, RunFailure use
aliases, macro_rules definitions, qualified/aliased stderr writers and hook aliases,
in addition to the prior UFCS/raw/function-value/type-alias controls. The inventory
is nonempty and exact. It is a lexical scan, **not semantic privacy proof**. Macro
expansion/dependencies, implicit or aborting panic payloads, backtrace contents
outside these fakes and actual service logs remain unproven. G-P1/P2/P3 remain gates.

## Commands and outcomes

All commands run in `<prepared-worktree>`, with no instrumentation/stress/retry flags.

| Command | Result and pin |
| --- | --- |
| `cargo fmt --all --check` | Passed final source after all edits/restores. |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | Passed final source, 3.16s; earlier passes 28.52s and 3.01s retained. |
| `cargo nextest run --locked -p arxd -E 'test(service::unix_bridge) | test(exec::stage_b_tests)'` | Final focused 24/24, run `8256aee4-0d4a-4ceb-99f1-02b961ee9fff`, 1.857s. |
| `cargo check --locked --workspace --all-targets --target x86_64-unknown-linux-gnu` | Failed before arxd at libsqlite3-sys: missing x86_64-linux-gnu-gcc. No install/stub, no Unix compilation or execution credit. Compiler emitted local environment paths in tool output; those were not copied into repository evidence. |
| `cargo nextest run --locked --workspace` | Exactly ONE final normal full workspace run, after final source/restores, source d3615be: **323 passed, 3 skipped**, run `8195e507-b3d3-4306-9fe6-72b89fce5a3f`, 11.307s. Additional skip is the new parent-only fixture. |
| `git diff --check` | Passed before source commit and final evidence. |

No ExecutionFailed recurrence in this final run; that does not resolve the historic
natural failure. Linux/macOS real compilation and execution are pending. Windows
fake/normal fixture passes are not Unix or native acceptance.

### Development failures and intermediate checks (not erased)

- Initial `cargo test --locked -p arxd --lib service::unix_bridge --no-run`
  failed compilation: two wrong Activity::list calls and ambiguous PathBuf conversion;
  one unused import. Corrected test helpers/types. No mutation kill credit.
- First focused cargo test reached runtime: 5 passed, 4 failed, 1 ignored; invalid
  activity-list page limit 100 caused downstream failures. Next: 6 passed, 3 failed,
  1 ignored; refusal dispatch was incorrectly unwrapped, missing-version replacement
  did not alter sorted JSON, and cleared child temp env caused fixture failure.
  Corrected expected refusal handling, JSON edit and owned child temp variables.
- Corrected cargo test: 9 passed/1 ignored. Focused nextest `4555e942-71f7-4991-b0a6-bc5898d49570`:
  14 passed/1 failed; scanner mistook captured stderr field names for std stderr
  writers. Restricted to qualified/aliased writer tokens; no production change.
- Intermediate focused passes: `dbfb9307-579d-4fec-95e6-96958d87656b` 15/15;
  `9968f164-a1ec-45c6-9914-a25fc601d274` 17/17.
- Expanded focused `17e223d6-ef5c-4305-8732-501ac7e078bc`: 23 passed/1 failed;
  T9 evaluated dispatch outside its runtime. Moved evaluation into async runtime.
  T9 passed `67f5d160-f0ba-4e19-b9f7-072cf451a6bd` and later
  `75c0fb61-ca54-430b-afa8-c4910a3b6c7d` before their mutations.
- One PowerShell source-search command had a brace-list parser error, no mutation.
  Initial mutation driver stopped before mutation on CRLF matching; normalized its
  comparison text, preserved original bytes. No compile/infrastructure failure
  is credited as a mutation kill.

## Compiled mutations and restoration

Each row used `cargo nextest run --locked -p arxd -E 'test(<filter>)'` with the
listed filter. All listed mutation runs compiled. Both survivors are retained;
only changed assertions justified their subsequent rechecks. No full-suite retry.
The early batch used bridge bytes identical to final d3615be; later test additions
strengthened T2/T6/T9. The payload-rethrow mutation is distinct from the actual
runner-only catch mutation (both listed). Production-selection coverage uses the
compiled native-refusal mutation; no real production bridge constructor exists to
select. ExecError text cannot cross the typed phase interface; direct mapping,
T8/T10 and source audit cover this rather than claiming an unperformed text mutant.
AfterSpawn -> Unknown has equivalent dispatcher row G; exact Created is nevertheless
asserted directly. Stage B/C mutations keep their own historical credits only.

| Mutation | Filter | Outcome | Run |
| --- | --- | --- | --- |
| spawn-notinvoked | `t4_exact_reports` | compiled assertion kill | `ed4d283a-3b4b-461c-9737-6d73ba669402` |
| cancel-stopping | `t4_exact_reports` | compiled assertion kill | `6c614d9b-8b5b-44b5-b091-721066a30231` |
| timeout-code | `t5_result_mapping` | compiled assertion kill | `4576a7eb-1232-4824-8063-44a0a33471b6` |
| mac-sweep | `t4_exact_reports` | compiled assertion kill | `a8b6c575-aae5-44bc-84a8-8996e9347264` |
| cleanup-true | `t5_result_mapping` | compiled assertion kill | `7b0212b1-75b1-410d-988d-f37c87f020a2` |
| discovery-dropped | `t6_discovery` | compiled assertion kill | `ac1dc0ca-9154-47a5-b1e2-1bf3c91408eb` |
| request-cwd | `t3_exact_specs` | compiled assertion kill | `f73aae56-e412-4798-9dc5-5c45f1fc2717` |
| catch-removed | `t4_phase_matrix` | compiled assertion kill | `124e4b3c-2fdb-4cf1-9bca-664cf019a0f1` |
| missing-private-state | `t1_derivation` | compiled assertion kill | `fceed554-5691-4002-890d-25bded636177` |
| missing-supervisor | `t2_bounded` | survived | `2c4e8392-de06-41d2-a865-c7e5095ab9f1` |
| payload-rethrow | `t4_phase_matrix` | compiled assertion kill | `c57b5134-c09b-4d40-8736-549045f2ca49` |
| panic-formatted | `p_scan_checked` | compiled assertion kill | `e01cbce5-18aa-4e62-9280-b3118b1eeedc` |
| service-hook | `p_scan_checked` | compiled assertion kill | `7c8cae9e-b129-42d8-800b-67a053df46bb` |
| missing-supervisor | `t2_bounded` | compiled assertion kill | `c4666b14-01f5-4c98-870f-24e8bc0dbb76` |
| catch-runner-only | `t4_phase_matrix` | compiled assertion kill | `f06bd103-dbc6-4aa2-b4c8-36b6dd82082f` |
| payload-reemit | `t10_reexec` | compiled assertion kill | `db2f5e0a-cd11-4bfd-ab40-d65a19e23e98` |
| native-fake-preflight-activates | `t9_production_selection` | NOT KILLED | `ca1c96d8-1526-4a00-a466-838b8476e664` |
| native-fake-preflight-activates | `t9_production_selection` | compiled assertion kill | `919d90cb-c74e-4b89-9b08-e045ae72a000` |

Bridge restoration after each driver compared byte SHA-256
`0b4a142139ed57c8da9cc1cf066852aa1145549a5fbe211af1c4a4fc3eac347b`;
final executor restoration SHA-256
`2cde815fc733fdbba75ab098457b6acbb3ac70e3e6c2e05c8c69c58a563a3b3c`.
Both match final working bytes and the source-manifest credit below (Git blobs
normalize line endings). Temporary mutation drivers/results stay ignored in target/.

## Final source manifest

| File | Git blob at d3615be | Working-byte SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/service/unix_bridge.rs` | `00f8f2c0aae63e83f58f81158eb42fc88c41dff7` | `0b4a142139ed57c8da9cc1cf066852aa1145549a5fbe211af1c4a4fc3eac347b` |
| `crates/arxd/src/service/unix_bridge/tests.rs` | `6eb2b7c951162544fe5e3a92335d46591c2beb53` | `552c63b2cfbe28c942148d3cbaf3ad1c21c12255ac2fb79876197f4a04ccee9e` |
| `crates/arxd/src/service/executor.rs` | `9c538ee25bf06fb1d0a101f72cf4f3fba742cb30` | `2cde815fc733fdbba75ab098457b6acbb3ac70e3e6c2e05c8c69c58a563a3b3c` |
| `crates/arxd/src/service/mod.rs` | `c0488d356ca7b133d7c056c2273da2af2517a824` | `f5c4fd5417043833fb35754fc275dd7445c3a9ece1e2913dffdce4c514123d72` |
| `crates/arxd/src/exec/stage_b_tests.rs` | `c875ccab1d97848f6b1f3e8466f4fe39a936449e` | `9b2f7883e8f0b5732afa4210ce0393987ec095b402f1574d262e699b64e6860a` |

No architectural deviation or activation is proposed. Pure owner-policy checks
are confined to the inactive bridge; only validated ServiceConfig is accepted,
and the injected once-only factory is responsible for port construction. A future
production factory/readiness/preflight integration remains a separately reviewed
change. No stage E/F/G work, hierarchy read/setup/janitor, installed binary spawn,
service account lookup, privilege/root path, SCM/LSA/service/VM/Lab Manager,
private-state or old ignored fixture invocation occurred in the new tests. The
existing normal workspace fixture baseline ran as required; only T10's dedicated
new fixture was reexecuted through its normal parent. No owner/admin step is
authorized by product priority. CI investigations remain parked; all publication,
review and merge gates and the dependency's unmerged status remain intact.
