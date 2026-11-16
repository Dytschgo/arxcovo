# S2b readiness gate: explicit-release barrier repair

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-16.
Base: `ec94a5a02b3ba73739cc0aedbcc9dd2309140221`, on merged main
`9074a85a4e36ba86dadd836e2508a0484ef6eba2`.
Source commit: `711b454fceba2ed665236f184884b11322fc5074`.
Local only; fresh independent Opus review is required before publication.

## Reported failure and scope

The coordinator reports first post-C1 main CI run 43819925277, Windows job
133140434354: `s2b_latched_readiness_gate` failed in 9.271 seconds. Expected
`ARX-S2B-GATE-PANIC` at 106; unexpected `recv_timeout` unwrap `Timeout` at
132; final assertion at 178, `actual bridge gate entered after admission`,
left 1, expected 2. Windows: 507 passed, 1 failed, 16 skipped, 58 unrun;
nextest step 233 seconds, test wall 124.487 seconds. Ubuntu/macOS green.
This report was supplied with the task, not independently fetched here.

C1's changed Playwright install was not reached. There is no C1 main
confirmation or causal attribution. No CI rerun was requested. Sampling
and feature work remain held pending repair gates and green main.

Read AGENTS, KICKOFF, dytschgo-debug and active profile, ADR0020 D6,
accepted S2b U-a/U-b/U-c proposal, and S2b independent review. The repair
changes only the portable S2b test and its exact source SHA in the existing
S2b harness. The pin update preserves manifest applicability; no harness
execution or mutation semantics change. All production paths, including
Runner, executor, readiness latch and post-admission gate, are unchanged.
No PROGRESS, ADR, Cargo, workflow, C1 or sampler changes.

## Feedback loop and minimization

Dedicated build/temp root: `D:/arxcovo-s2b-ec94a5a-20261116-sol`.
PowerShell environment used for Cargo and test execution:

```powershell
$env:CARGO_TARGET_DIR='D:/arxcovo-s2b-ec94a5a-20261116-sol/target'
$env:TEMP='D:/arxcovo-s2b-ec94a5a-20261116-sol/tmp'
$env:TMP=$env:TEMP
New-Item -ItemType Directory -Force -Path $env:TEMP | Out-Null
cargo test -p arxd --lib --locked --offline s2b_latched_readiness_gate --no-run
```

Build exit 0, 53.90 seconds. Executable:
`D:/arxcovo-s2b-ec94a5a-20261116-sol/target/debug/deps/arxd-c184af5585f22530.exe`.
Unchanged bounded loop, actual invocation (with `$exe` set to that path):

```powershell
$clock=[Diagnostics.Stopwatch]::StartNew(); $passes=0
for($i=1; $i -le 40 -and $clock.Elapsed.TotalSeconds -lt 180; $i++){
  $out=& $exe --exact service::unix_bridge::tests::s2b::s2b_latched_readiness_gate --nocapture 2>&1
  $code=$LASTEXITCODE
  $out | Out-File -Encoding utf8 "D:/arxcovo-s2b-ec94a5a-20261116-sol/baseline-$i.log"
  if($code -ne 0){ $out; Write-Output "STOP relevant failure iteration=$i exit=$code"; break }
  $passes++
}
```

Result: **40 passed in 3.1507067 seconds**. No natural reproduction.

Two single-run test-only delay controls followed, stopping at the first
relevant red. Neither is a proposed fix or evidence of natural CI cause.
Both used `tokio::time::sleep(Duration::from_millis(5200)).await` and:

```powershell
cargo test -p arxd --lib --locked --offline s2b_latched_readiness_gate -- --nocapture
```

1. Delay immediately after barrier arrival, before panic dispatch: exit 101,
   5.24 seconds, Timeout at 132 then `panic dispatch returned` failure.
   Rejected as a nearby symptom, not the requested exact reproduction.
2. Delay immediately after panic dispatch returns and `calls == 1`: exit 101,
   5.24 seconds. Exact signal:

```text
s2b.rs:106:21: ARX-S2B-GATE-PANIC
s2b.rs:132:18: called Result::unwrap() on an Err value: Timeout
s2b.rs:179:5: assertion left == right failed: actual bridge gate entered after admission
  left: 1
 right: 2
```

The assertion moved from 178 to 179 solely because of the added delay line.
Only one inserted sleep is needed; deleting it restores the unchanged loop
above. No extra caller, simulated report, direct entry-counter write, state
reset or production edit is needed. This is a deterministic exact seam for
a delayed coordinator, not proof that the hosted failure had that cause.

## Ranked hypotheses and probe

Recorded before the fix:

1. The barrier's elapsed-time expiry kills the held request before explicit
   release. Prediction: replace this expiry with sender-lifetime release;
   the exact delay control passes without changing any gate assertion.
2. The panic dispatch leaves the slot occupied. Prediction: the real active
   counter's zero assertion fails even after barrier repair.
3. Readiness/gate logic fails. Prediction: existing entry, Runner-count,
   refusal or latch checks still fail after barrier repair.

The first prediction passed; the others were not observed under the control.
The test had conflated a synchronization hold with a five-second deadline
for unrelated work. The held worker could panic before its intended gate
entry. The coordinator then found one bridge entry instead of two.

## Regression and repair

Added `s2b_latched_readiness_gate_delayed_release`, reusing the actual
`stale_admission` scenario and every assertion with a 5200 ms after-panic
delay. Before repair, ran:

```powershell
cargo test -p arxd --lib --locked --offline s2b_latched_readiness_gate_delayed_release -- --exact service::unix_bridge::tests::s2b::s2b_latched_readiness_gate_delayed_release --nocapture
```

Exit 101, 5.26 seconds: expected panic, unexpected Timeout at 132, entry
assertion left 1/right 2. This is the fail-before regression.

One fix attempt: held receiver now waits for explicit release with `recv()`.
The coordinator owns the only release sender; an assertion failure unwinds
and drops it, disconnecting the receiver so that failure does not strand
the worker. No longer-running numeric timeout replaces the old five seconds.
The separate five-second arrival observation is unchanged. A genuinely hung
coordinator is still subject to the surrounding test/CI process deadline;
this channel wait intentionally has no independent elapsed-time deadline.

Release still occurs only after panic response, ordinary early refusal,
unchanged entry count, and actual zero active-slot observation. The held
request must then enter the actual bridge exactly once, make zero additional
Runner calls, return the containment refusal and leave readiness latched.
Independent Ready-plus-residue coverage is retained. No assertion was relaxed.

## Actual validation

All following commands ran on Windows with the environment above:

```powershell
cargo test -p arxd --lib --locked --offline service::unix_bridge::tests::s2b -- --nocapture
cargo fmt --all -- --check
python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
& $exe service::unix_bridge::tests::e6:: service::unix_bridge::tests::e7:: exec::residue_tests::cleanup_source_pins_release_sites_and_legacy_errno_order exec::stage_b_tests::p_scan_checked_nonempty_inventory exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory exec::stage_b_tests::l6_e3_tokenized_source_inventory_and_mutants exec::stage_b_tests::s12_e4_portable_production_pins --nocapture
git diff --check
rg -n '\[DEBUG-' crates/arxd/src/service/unix_bridge/tests/s2b.rs
```

- S2b: exit 0, **4 passed**, 5.26 seconds, including original scenario and
  delayed regression; expected caught control panics remain visible.
- Fmt: exit 0, no output.
- S2b Python harness: exit 0, **11 passed**, 0.309 seconds; mock/pure only,
  including exact pinned-source and mutation applicability checks.
- Locked offline workspace/all-target Clippy: exit 0, 25.32 seconds.
- E6/E7, residue and source guards: exit 0, **28 passed**, 0.91 seconds.
- Diff check: exit 0. DEBUG search: exit 1, no matches.

The broad workspace test suite was not run, avoiding the known owner-account
baseline failures. No ignored test, M2 native fixture, installation, cache
deletion, admin action, old Lab Manager or VM operation was performed.
No temporary delay remains outside the deliberate regression parameter.

## Blobs and local artifacts

Final Git blobs:

- `crates/arxd/src/service/unix_bridge/tests/s2b.rs`:
  `5920727330870610529254093c72efe8ec6ec636`.
- `tools/ci/s2b-native/driver.py`:
  `2239703768990efec7324539a71193d0ae62da2b`.
- Exact S2b source SHA256:
  `a5dfde4904bf5d45b66f5ae3719883d2d61a66e17e74bc7c4f1055f90bbd1f7b`.

Logs are local under the dedicated root, not committed or independently
reviewed. SHA256 values:

| File | SHA256 |
| --- | --- |
| delay-before.log | f2fe803eec5ec203607d112561ee6f97f271ae3eccab8b06ce796eea83cb08e7 |
| delay-after-panic-before.log | 685b3c1d68d20e6534c3001c808f2ecdc36e3f5ac9811ff8d9663bdc21a2d498 |
| regression-before.log | cbf097bcf8e51815f552761a5bfad0d5c8cd3b9e3a7608aa11eac9d4d788a4c7 |
| s2b-after.log | d21d0e7e6dd42c0d329a81fe80dcef27a9324bda5d17a700047d0ab1e236fe6a |
| guards.log | e42cb39156135a5b4bc987044f82fc683597f6606a7213bd5571cc26153c78e8 |
| s2b-harness.log | 770e9877fd8e2295ec692a77f4470d649a1fc34fcd9a845cd7801dfb8b202908 |
| clippy.log | 3ae8bd8da4213b53dbd5d7cc9d2f774380691f7d1a7f23c5804c85fb8a7fe660 |

## Handoff

One fix attempted for this separate repair; zero failed fix attempts. The
stopped cluster suite's three-fix budget is untouched. Natural failure cause
remains unknown; only the exact delayed-coordinator failure seam is proven.
No push, PR, merge, watch, child agent or scheduling occurred. Fresh Opus
review, all five hosted gates, coordinator merge and main-green confirmation
remain required before sampling/features. M2 open, A7 NO-GO, A12 consumed
without retry, M3 unchecked and Windows cluster quarantine remain unchanged.
Hardware paths are **unverified on real hardware**.
