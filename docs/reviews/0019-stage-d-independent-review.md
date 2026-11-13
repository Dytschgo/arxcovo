# ADR 0019 Stage D independent review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-12. Review sub-agent,
no subagents. This review covers only the Sol Stage D implementation: the
inactive `UnixBridge`, its fake-port tests, the `cfg(test)` executor seams and
the P-SCAN bridge extension. It can count as the different-provider review of
that work. Opus wrote the ADR 0019 architecture, so this review claims no
independence for the contract.

**Standards: CHANGES REQUESTED.** S1 and S2 are tests that cannot fail. They
break the AGENTS rule "every behavior needs a meaningful test". Both fixes
touch tests only. **Spec: CHANGES REQUESTED.** All production behavior matches
the accepted contract. Requirements T1 (refusal rows) and T3 (exact spec at the
port) are only partly met, for the same reasons. T10 misses one required
absence check. This review grants no Stage D publication, activation, cgroup
setup, experiment or M2 acceptance. Native, installed and dedicated paths
remain **unverified on real hardware**. M2 remains incomplete.

## Pins

- Worktree: the runtime-prepared T3 worktree, branch `t3/08a330a5`. Its top
  level matched the working directory and is not the canonical checkout.
  Status was clean at exact HEAD `3b95a1bf8d130c18cbb74482609ad164c6bcac6c`.
  No checkout or worktree was created. Nothing was written to the canonical
  checkout.
- Target `3b95a1b` (docs only). Source `d3615be3b9820cf72a40b321a3dcf6c746a9e7bd`.
  Base `7b2cfa1cd00d0f3dc657513dd07bc684739cc1e1`, whose parent is the
  **unmerged** Stage C PR53 source `2ce79caf9ded55976e3e83b2c49b41d54160f2da`.
  Actual main is Stage B `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`
  (`origin/main`), an ancestor of the base. This review does not claim that
  Stage C's merge gate has cleared.
- `git diff 7b2cfa1 d3615be` outside `docs/` touches exactly five files (1,353+/1−):
  `service/unix_bridge.rs`, `service/unix_bridge/tests.rs`,
  `service/executor.rs` (+36, three `#[cfg(test)]` methods in one hunk),
  `service/mod.rs` (+2, private `mod unix_bridge;`) and
  `exec/stage_b_tests.rs` (scanner, +41/−1). `d3615be..3b95a1b` touches only
  PROGRESS and the evidence report. All five Git blobs at the target match the
  evidence manifest. Remove the three `cfg(test)` methods and `executor.rs` is
  the base text again. `configured`, `native`, `dispatch`, `complete`,
  `assess` and `output_value` are byte-unchanged. Cargo, the lockfile, CI,
  config, the owner CLI, SDK use and `exec/` runtime are unchanged.

## Standards

Sources: AGENTS.md, KICKOFF, ADR 0019 C2 (P1–P7), and the smell baseline.

### S1 (medium, required): fake-port assertions run inside the bridge's own catch

- **Where:** `unix_bridge/tests.rs:751-775` (`t3_fake_port_receives_exact_request`)
  and `:679` (the T8 port assertion).
- **Defect:** the fake port runs inside `UnixBridge::run`'s
  `catch_unwind` (`unix_bridge.rs:119`). A failed `assert_eq!` in the fake is
  caught and becomes `Report::Failed { Unknown }`. The test then drops that
  report (`let _ = b.run(..)`). The test cannot fail. The same holds for the
  T8 `s.arguments … contains(MARKER)` assertion, because T8 checks only that
  markers are absent, not the report shape.
- **Proof (reviewer runs, scratch export, bytes restored):** mutation
  `spec.output_bytes += 1` in `body` survived all 15 bridge tests (run
  `72fa75e4-cfd2-49a7-954e-f0761d6f4607`). Mutation
  `spec.timeout = 1s` in `body` also survived all 15 (run
  `a917dda9-4955-4d8c-ae99-f05092584be6`). `t3_exact_specs` tests `spec()`
  only, not what reaches the port. `t3_max_agent_tail…` catches argv changes
  only.
- **Correction:** after each `b.run`, assert the report shape
  (`Report::Completed { .. }` for path/tool, `Report::Discovered { .. }` for
  the probe). Or record the received spec in a `Mutex` and compare it outside
  the bridge. In T8, assert each class's expected report or reason. Then rerun
  both mutations and record the kills.

### S2 (medium, required): T1 refusal rows skip silently

- **Where:** `unix_bridge/tests.rs:143-160`, the `if let Ok(c) = parse(..)`
  loop over `name: "bad name"`, `name: 129×a`, `gid: 0` and `gid: null`.
- **Defect:** `config::tests::parse` runs the full `ServiceConfig::parse` with
  `FakeResolver`. `FakeResolver` resolves only `builder` to `1002`, always
  with gid `1002`. So all four inputs are rejected before `derive` runs, and
  the `if let` skips every assertion.
- **Proof:** mutation `|| gid == 0` → `|| false` survived the T1 tests (run
  `9f9a19de-7f5b-4c93-9ab5-e581d5685748`). So did removing both the account
  length check and the charset check (run
  `688b3d13-e4d6-40ab-96cf-38fb24a49b98`). The bridge's own account and gid
  refusals have no meaningful test. In production `Runner::new` repeats those
  checks, and the bridge is inactive, so this is a test defect, not a live
  exposure.
- **Correction:** build these cases with `ServiceConfig::parse_inventory`, as
  T8 already does at `:655`, and `unwrap()` the result so that no case can
  skip. Rerun both mutations.

### Lower-severity, non-blocking

- **S3 (low):** `unix_bridge.rs:2` uses a blanket `#![allow(dead_code)]`. The
  repo's pattern is `cfg_attr(not(test), allow(dead_code))` (`executor.rs:81`).
  The blanket form also hides genuinely dead test-build helpers. Narrow it.
- **S4 (low):** `executor.rs:244`. If `Arc::get_mut` fails, the seam silently
  keeps the `Native` backend. It is test-only. Make it fail loudly, for
  example with `assert!(Arc::get_mut(..).is_some())` in the seam.
- **S5 (judgement, Duplicated Code):** `derive` (`unix_bridge.rs:144-157`)
  re-implements part of `exec::validate_config` (UID/GID, account charset,
  workspace, memory and pid ranges). It does not cover the environment
  whitelist or the protected-root overlap. Brief §1 assigns those refusals to
  `Runner::new` through the factory. So the future production factory must
  map every `Runner::new` error to `PreflightFailed`, and that needs its own
  test at activation. This is acceptable for an inactive stage.
- **S6 (advisory, scanner limits):** the new P-SCAN rules are lexical. They
  miss glob imports (`use std::io::*; stderr()`), stdout writers
  (`write!(std::io::stdout(), …)`), `include!`, `libc::write`, and payloads
  produced by dependency macros. The evidence report correctly says the scan
  is "not semantic privacy proof". I read the bridge source directly. It has
  no formatting, no stream writes, no `str` range indexing, no hook, no
  `From` conversion, and the payload is dropped unread. G-P1 must keep these
  limits in scope.

Checked and clean: the constructor is `#[cfg(test)]` only, and the fields
and module are private. The production build can't construct the bridge, and
the dead-code allow is only cosmetic. No type in the bridge derives or
implements `Debug`. No `set_hook`/`take_hook`, `eprintln!`, `format!` or
logging. The exhaustive `failure` match has no wildcard. Preflight stays
outside the catch. Inventory is exact and non-empty, and the two seam literals
sit under `cfg(test)`.

## Spec

Sources: ADR 0019 D1–D7, C1–C3, the brief §1–§10, and the ADR 0017 row table
(lines 433–446) and output-release text.

### Production behavior: matches the contract

- **§1 derivation:** pure, with no I/O. Linux-only, standard rights only, with
  macOS, Windows and admin → `MechanismUnavailable`. The uid is parsed; gid
  `None` or `0`, account, workspace, memory and pid limits → `PreflightFailed`.
  Protected dirs come from `layout.private_dirs()`, and `admin: false`. The
  factory is called once at construction. Preflight order is check, then the
  stored result, then readiness, as §1 specifies. A nonexistent workspace
  still constructs (T1).
- **§1 parser:** bounded at 4,096 bytes. Strict UTF-8, one `0::` line, a
  `supervisor` leaf, no dot or empty components, no NUL. Returns
  `/sys/fs/cgroup/<unit>/runs`. Pure; nothing reads `/proc`.
- **§2 spec:** fixed `layout.binary()`. Argv is
  `exec-child path|tool <target> <cwd> <args…>` or `tool-probe`. Runner cwd is
  empty. Request timeout and output for runs; 60 s and 4,096 for discovery.
- **§3 mapping (`unix_bridge.rs:226-251`)** matches ADR 0017 exactly:
  - `Invalid` and `Resource` → `Failed { NotInvoked }` (row F, never
    `ProvenNone`).
  - `Cancelled` → `CancelledBeforeSpawn` (E).
  - `Shutdown` → `Stopping`; `Unsupported` → `PlatformUnsupported`.
  - `ServiceIdentity` and `Containment` → `MechanismUnavailable`.
  - `AccountChanged` → `AccountIdentityChanged`; `AccountPolicy` and
    `PrivateRoots` → `PreflightFailed` (D).
  - `SpawnFailed` → `Unknown`; `AfterSpawn` → `Created` (G, quarantine). No
    cleanup, boundary, observation or exit fact is invented.
- **§4 evidence:** UID/GID come from the child. The point is `PreExec`. The
  exit code is withheld unless the outcome is `Exited`, which covers the
  incidental timeout code. `MacProcessGroupBestEffort` → `ProcessGroup`,
  never `MacUidSweep`. Cleanup is copied unchanged. Captures move unread. The
  dispatcher alone releases output, so identity mismatch withholds both
  streams and a cleanup failure on a full match still releases (row H). The
  evidence report documents `dropped_bytes` as a lower bound.
- **§4 discovery:** requires `Exited`, code 0 and no dropped stdout. Then
  strict UTF-8 and exactly `TOOLS.len()` lines in table order. Fields are
  checked by key (shim key order is sorted), so an object must have exactly
  `{name, found, version}`. `deny_unknown_fields` rejects duplicate fields
  (T6 case 16). The version must be 1–128 printable ASCII bytes without `/`
  or `\`, and `found: false` with a version is refused. stderr is never read.
  Invalid discovery → empty tools → row K. Tools leave only on row N.
- **P7 catch:** the body covers spec derivation, the port call, mapping and
  discovery. `Err(_payload)` → `Failed { Unknown }` with the payload dropped
  unread: no downcast, no formatting, no re-panic.
- **D3/§6:** the executor's `cancel` and `shutdown` pass straight through.
  The bridge never builds or drops a `Runner` per run.
- **D7/T9:** inactive. `configured` still builds only `Native`. Tests never
  construct a real `Runner`.

### Requirements partly met

- **T1 (medium, required):** "every row … including refusals". The account
  and gid refusal rows are vacuous; see Standards S2 for the proof and fix.
- **T3 (medium, required):** the port must receive the exact `RunSpec`. The
  port-level check is vacuous for timeout and output; see Standards S1.
- **T10 (low, fix in the same pass):** brief (b) uses the UID/GID marker
  `3141592653`, and "every marker except (e)'s is absent". The parent at
  `unix_bridge/tests.rs:580` checks only `MARKER`. Add
  `assert!(!text.contains("3141592653"))`. The negative control (`:605`, a
  formatted payload, counted exactly once) and the fixed-literal counts are
  correct. The parent re-executes only the new purpose-built ignored fixture.

### Accepted deviations (documented, non-blocking)

- **T9:** the test uses a `cfg(test)` copy of `configured`'s composition plus a
  source guard, because `configured` hard-codes `NativeResolver`. Inertness
  rests on the constructor being `#[cfg(test)]` only, which the compiler
  enforces.
- **T7:** the bridge-level "drop never cancels shutdown" check is trivially
  true with a fake. The real `Runner::drop` independence test is in Stage B
  (`shared_cancellation_and_runner_drop_are_independent`).
- **§4:** `text.lines()` and `serde_json` accept CRLF, a missing final newline
  and surrounding whitespace. The fixed shim never emits these, and every field
  is revalidated.

No unrequested behavior was found. Nothing in this diff covers stages E, F or
G, an owner operation, CI or policy work.

## Coverage and credits

Reviewer runs (Windows host, scratch `git archive` export of `3b95a1b`, own
target directory; no worktree or canonical writes):

| Command | Result |
| --- | --- |
| `cargo nextest run --locked -p arxd -E 'test(service::unix_bridge) \| test(exec::stage_b_tests)'` | 24/24 pass, run `e1ce8b6b-fc88-4a41-ad17-8d8645eff87f` (baseline) |
| mutation `output_bytes += 1` in `body`, filter `test(service::unix_bridge)` | **survived** 15/15, `72fa75e4-cfd2-49a7-954e-f0761d6f4607` |
| mutation `timeout = 1s` in `body`, same filter | **survived** 15/15, `a917dda9-4955-4d8c-ae99-f05092584be6` |
| mutation `gid == 0` removed, filter `test(service::unix_bridge::tests::t1)` | **survived** 3/3, `9f9a19de-7f5b-4c93-9ab5-e581d5685748` |
| mutation account length and charset removed, same filter | **survived** 3/3, `688b3d13-e4d6-40ab-96cf-38fb24a49b98` |
| `cargo fmt --all --check` (export) | pass |

After every mutation, the bridge's SHA-256 was checked against the pristine
export copy (`3acf3d23…`). Limits of these runs: no full workspace run, no
Clippy, no Unix compilation, no stress, retry or instrumentation. They
constructed no real `Runner`, ran no native or old ignored fixtures, and made
no GitHub, CI, owner or VM action.

Inherited, credited by exact pin, not rerun: the builder's final fmt and
locked all-target Clippy, the focused run `8256aee4` (24/24), and the one full
normal run `8195e507-b3d3-4306-9fe6-72b89fce5a3f` (323 passed, 3 skipped) at
`d3615be`. I read the builder's mutation ledger, including the survivors
`missing-supervisor` and `native-fake-preflight-activates` and their later
kills, as honest records. My S1 and S2 survivors are not in that ledger. The
builder's Linux cross-check failed before `arxd` (no cross GCC), so Stage D
has **no Unix compile or runtime credit**. Hosted Linux and macOS CI must
compile and run these tests before merge. Stage B and Stage C credits belong
to their own heads. `ExecutionFailed` / `E_PROTECTION` remain unexplained,
with no fix, waiver or retry inferred.

## Blockers versus deferrals

- **Blocking Stage D acceptance (test-only, small):** S1/T3, S2/T1 and the
  T10 UID-marker assertion. No production source change is needed. Then
  re-run the four mutations above, which must now be killed.
- **External gates, unchanged and not this slice's work:** Stage C PR53 is
  still unmerged. Required gates are hosted Linux/macOS compile and run,
  green required checks, and this different-provider review recorded on the
  publishing PR.
- **Non-blocking, can defer:** S3–S6 and the accepted deviations. G-P1/P2/P3,
  stage E setup and janitor, the stage F `E_PROTECTION` diagnosis and
  experiment, and G activation all keep their gates.

One Standards finding set (2 medium required, 4 low or advisory); worst: S1,
assertions swallowed by the bridge's catch. One Spec finding set (2 medium
partial requirements, 1 low); worst: T1 refusal rows never exercised.
