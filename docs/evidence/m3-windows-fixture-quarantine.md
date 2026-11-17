# Full Windows fixture quarantine: local implementation evidence

Provider: OpenAI / gpt-6.1-sol / high; implementation sub-agent, no children.
Prepared base `b0345f3d253f2ef177b9ecf4ba2a1d6887a4d9d9`;
main source `c6121523417089bc0afaf307470183dd4a7f47ba`;
PR164 prior source `17a3fb2a7acbd23024b58e3445333298f860f4be`.
Source commit `221077d0c13131be09d7af86568fdc94e47bbe1e`.

## Scope and identity proof

13 async three_node fixture cases + 11 async issuance cases = 24 Windows-only
ignores. Original eight attributes and EXPECTED/REASON declarations are unchanged.
R1–R5 share the whole-fixture reason; R5's old reason is normalized. The pure
read-attribution unit remains required. Existing D1 controls are globally ignored
unchanged. Ubuntu/macOS must execute all 24. The bounded non-gating sample remains
only the original eight: five fixed exact-name exclusions R1–R5; issuance outside
its namespace. No runtime guard-derived argv.

Local Python assertions compared both Rust files to main after stripping only
Windows ignore attributes: exact equality, proving no body/helper/assertion/timing
or production changes. Each of main's eight original attribute/function prefixes
occurs exactly once unchanged. EXPECTED/REASON declaration lines are identical to
main. Sampler equality to prepared source after replacing only FILTER is exact.
All changed source is limited to the two Rust attribute files, guard, sampler
FILTER and sampler tests. Docs only add ADR0024/progress/brief/evidence updates.

Tested source SHA-256 (unchanged in source commit):

| File | SHA-256 |
| --- | --- |
| `crates/arxd/src/cluster/consensus/three_node.rs` | `afc3ad8e6fbe104965088fa2ab1f7b81fad8494a804b254c250261d62101993d` |
| `crates/arxd/src/cluster/consensus/issue_tests.rs` | `9e323278bd78034de48376b4e53cd4ce6f72d6968e73dfbff04213be84f27798` |
| `tools/ci/test_windows_cluster_quarantine.py` | `ab2cef1c9104142e82f0587651644015317bdcb77b3cd14617446a0bd561d78f` |
| `tools/ci/windows_diag_sample.py` | `072c4a57eb1908e7a00ba42db065c9620bfd46d24375b363eb23c08605e45b38` |
| `tools/ci/test_windows_diag_sample.py` | `e8ede5a0cee9535776e5120265d7347baad57bcf9d6e3ccc8a94aef5b0c326b6` |

## Checks actually run

Local logs are retained in `D:/Temp/arx-fixture-pw4wskvb/`. New isolated target and
temporary directories; CARGO_NET_OFFLINE=true. No existing cache deletion, install,
admin, VM, old Lab Manager, or engine behavior execution. Python evidence writes
use UTF-8/LF. Two routine corrections: stale R5-only sampler expectations after
initial failing suite; documentation encoding repaired before commit. No counted
engine fix (Sol3/3, Opus0/2 unchanged).

Guard includes seven non-no-op per-name mutations for every one of 24 gates
(removed gate, wrong OS, Unix family, all OS, wrong reason, rename, duplicate),
new async/sync test rejection, extra ignore and cfg rejection, inventory drift,
pure-unit ignore rejection, plus pure PKI and module-binding exclusion mutants.
Sampler pins five exact exclusions against verified guard namespace and checks
that every unsampled ignored mismatch is accepted but any such match gates.

| Check | Exit | Seconds | Log SHA-256 |
| --- | --- | --- | --- |
| guard | 0 | 0.301 | `b6ac13ffda6d9a41b37379b34368d0343483fd0b15bd94489144a7f928793c1c` |
| sampler | 1 | 0.37 | `7a1161e04edef99b98e2a53b185bbc415e1f7a97833ee238d32173d8a2060f8b` |
| fmt | 0 | 1.673 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| sampler-final | 0 | 0.358 | `5426dae5449877dab112f46b826d329fd9e88ad67ec55c1ba88eb2b87fa08733` |
| clippy | 0 | 43.94 | `2cf237420adde9417dd60a98636b96949dd100e579c37346996e8ae52239b4e9` |
| guard-final | 0 | 0.249 | `6c603b3dd60efe6658c442ffeee67af86c39a0098abbc9dff4fd1055605ede89` |
| final-python | 0 | 0.383 | `95a590382d5a19b8d254c14eb83834e3d55ec20f716b5bc3e2cbe888139f9b92` |

`guard-final`: 2 tests pass; `sampler-final`: 60 tests pass; `final-python`:
62 tests pass on final five source files. Initial sampler: 60 tests, two stale
expectation failures and one undefined local variable; retained log above. This
was test implementation failure, not an engine behavior red. Fmt and all-target
workspace Clippy passed with `--locked --offline -- -D warnings` for Clippy.
`git diff --check` passed. Exact commands are in local `checks.jsonl`; test
commands are `python -m unittest discover -s tools/ci -p <pattern>` with patterns
`test_windows_cluster_quarantine.py`, `test_windows_diag_sample.py`, `test_windows*py`;
fmt is `cargo fmt --all --check`.

## Hosted and acceptance limits

No hosted run/rerun, publishing, push, PR mutation, status, watch, merge, label or
protection change. No ignored fixture executed. Hosted Ubuntu/macOS execution of
24 is required but not run locally. Windows fixture coverage remains unverified
on hosted Windows and **unverified on real hardware**. A green eight-test sample
is no Windows fixture acceptance, stability, repair or re-enable evidence.

Current main c612152 / CI43880904887 remains RED: R5 timed out after gate release;
1475 s wall exceeds active budget, 738 s charged uncredited because red. Prior
PR164 run43949259583/job133587063992 I4 red remains: `NotLeader(None)` at direct
replay proposal, issue_tests.rs237, 11.495 s; see retained
[Opus disposition](../reviews/m3-pr164-i4-replay-opus-disposition.md).
Opus induced exact I4 once; counterfactual passed and hooks removed; hosted cause
and reproduction rate unproven. No cause/stability/green-main claim here.

Independent Opus review is required before push. Four hosted gates must pass
before root asks the owner for a one-use budget-only exception on final exact
head. ADR0025 accepted but inactive; bootstrap unauthorized. M2 remains open,
M3 unchecked. Restore the fixture suite together only after a separately reviewed
bounded timing recovery plan with meaningful slow-storage evidence and hosted
acceptance; no additional per-test expansion.

## Real nextest inventory and zero-execution result

Full arxd lib JSON: all 13 three_node fixture names and all 11 issuance names
are `ignored: true`; `read_failure_attribution_covers_all_errors` is `ignored: false`.
The issuance module's actual qualified name is `cluster::consensus::issue::issue_tests`.
Exact sampler `cargo_argv("list")` JSON has exactly eight ignored matches; all five
R1–R5 have `mismatch/expression`. Its bytes were passed to `check_inventory` using
the isolated target's parent as the root (the function checks root/target). This
checks inventory only; it is not a full wrapper execution or ownership check.
Ordinary fixture-only `run` returns expected exit4 with zero tests run. No engine
behavior acceptance is claimed.

Two local evidence-script precondition mistakes were corrected without changing
source or rerunning list commands: initial issuance namespace lookup omitted
`issue::`; initial `check_inventory` root was the checkout instead of the isolated
target parent. Both existing JSON files were reused and all assertions then passed.
These were verifier errors, not engine behavior reds. No Cargo engine loop occurred.

| Command | Exit | stdout SHA-256 | stderr SHA-256 |
| --- | --- | --- | --- |
| full-list | 0 | `acc4f376ad65a096d0691959ecf8a5e40eb92fb9b0dc52b4fef63be2c7507282` | `1f7d4f5a8e0ed9bb9ba27ea8fd5a5f654b5d779d5a6a7f4673790fd417799c7e` |
| sample-list | 0 | `b10313eeb6fb3f49df8fddaa74ae7049976a96b0acbc202b78887900971928fe` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| ordinary-fixture | 4 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` | `0c389ed1d9ae0b8e32f6338572188477df75b5647e07fd5ff0aa1440ee39d671` |

Exact argv actually executed (CARGO_NET_OFFLINE=true throughout):

```text
cargo nextest list -p arxd --lib --locked --offline -T json
cargo nextest list -T json --workspace --locked --cargo-quiet --user-config-file none --config-file tools/ci/windows-diag-sample.nextest.toml -P diag-sample --run-ignored only -E "package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/) & not test(=cluster::consensus::three_node::r1_apply_wait_deadline) & not test(=cluster::consensus::three_node::r2_isolated_read_refusal) & not test(=cluster::consensus::three_node::r3_store_wait_deadline_and_heartbeat) & not test(=cluster::consensus::three_node::r4_read_admission_and_caller_abort) & not test(=cluster::consensus::three_node::r5_shutdown_read_owners)"
cargo nextest run -p arxd --lib --locked --offline -E "package(arxd) & kind(lib) & ((test(/^cluster::consensus::three_node::/) & not test(=cluster::consensus::three_node::read_failure_attribution_covers_all_errors)) | test(/^cluster::consensus::issue::issue_tests::/))"
```

Ordinary fixture stderr summary:

```text
     Summary [   0.001s] 0 tests run: 0 passed, 571 skipped
error: no tests to run
```
