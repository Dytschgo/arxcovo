# Founder integration onto restored main

Provider Sol, high effort, single builder, no children. Prepared root base
`957184f4` (docs-only successors of source main
`39b3dd73052abd64c1c3d3042e62f080469f1597`, PR164). Retained founder head
`77398e4d6799a74aee9a073d7491d6ef34487858`; corrected reviewed source `c45089f`,
implementation `ec3e5d2`, evidence `2c6c096`, accepted contract `455d8ea`.
Merge base is `c6121523417089bc0afaf307470183dd4a7f47ba`.

The merge retains current main's 24 Windows fixture quarantines, eight-case
non-gating sample, required pure/store-close tests, all workflow/policy/Cargo
and runtime helper bytes. Only the reviewed founder binding/seal guard is added
to the complete current main guard. No injected-latency source/hook patch is
applied. Founder tests remain required on every OS. No production redesign,
dependencies, TLS, transport, custody or native work.

The final local verification below passed on source checkpoint
`b8aadf693e2c97cdad0f53434d776c267a21639d`. Historical
30 focused/155 cluster results and 15 meaningful builder mutants, plus reviewed
R2 wrong-key/wrong-cluster mutation evidence, are reused, not rerun. See the
[original evidence](m3-founder-bootstrap.md) and retained Opus reviews. Historical
main-red and eight-quarantine statements there describe their earlier revisions.

Supplied main CI44051366327 attempt1 passed on source `39b3dd73`: wall608s,
Windows603s, Ubuntu/macOS24 fixtures each. Windows sample skipped_budget;
no sample execution or Windows stability credit. Full five-check protection
restored after PR164's consumed one-use exception. No remote status query here.

Independent Opus delta/exact-head review, fresh hosted normal gates and hosted
Windows timing remain necessary before publication. PR159 stays separate/held.
M2 open, M3 unchecked, shared recovery counters Sol3/3 and Opus0/2 unchanged;
hardware paths **unverified on real hardware**. One routine documentation serialization correction (1/2); resolving merge
context is not a behavior fix. No runtime corrections.

No push, PR creation, status/comment, watch, merge to main, rerun, protection,
scheduling, admin, VM, installation, old Lab Manager or cache deletion action.


## Final actual verification (Windows, one round)

Source merge commit `b8aadf693e2c97cdad0f53434d776c267a21639d` has parents
`957184f4591592d511101128a4f8f9dcc88d8ef7` and
`77398e4d6799a74aee9a073d7491d6ef34487858`. Later commit is evidence/docs only.
Every command executed once; no natural failure, fix or rerun. Routine integration
corrections 1/2 (documentation encoding only). Merge context resolution preserved
reviewed code and current main. The final historical-label audit caught a Windows
default-decoder conversion during evidence writing. PROGRESS was restored from
the source checkpoint and only the intended new status/checkpoint reapplied with
explicit UTF-8. No source change or runtime rerun resulted. The first audit script
also hit a default-decoder error; its corrected explicit-UTF-8 checks exposed the
historical-byte change before handoff.
An initial read-only identity script assumed held PR159's `pki/verify.rs` existed;
it failed at that lookup. Corrected evidence verifies the path is absent on both
base and integration. No source edit or test retry resulted.

All Cargo checks used a unique retained directory:
`D:/Temp/arx-founder-integrate-c042978f201a49d6b21a8dc864439901/target`,
with TEMP/TMP set to its parent, CARGO_PROFILE_DEV_DEBUG=0 and offline networking.
No cache was deleted. Commands, durations, full source/blob/diff hashes and real
inventory are in [the manifest](m3-founder-main-integration.json).

| Actual command | Result | Command seconds |
| --- | --- | --- |
| `cargo fmt --all --check` | PASS | 1.938 |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | PASS | 52.25 |
| `cargo nextest run -p arxd --locked --offline -E 'test(/^cluster::/)' --no-fail-fast` | 139 passed, 484 skipped; test wall5.639s | 66.859 |
| `cargo test -p arxd --locked --offline --lib cluster:: -- --test-threads=4` | 139 passed, 29 ignored, 417 filtered; test wall10.88s | 11.453 |
| `python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py` | 3 passed (current full-fixture and founder mutants) | 0.266 |
| `python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py` | 60 passed | 0.391 |
| `node --test scripts/ci-workflow.test.mjs` | 4 passed | 0.141 |
| `cargo nextest list -p arxd --locked --offline --message-format json` | 24 fixture ignores; 13 founder tests and 7 pure/store-close checks required | 1.359 |

The 29 in-process ignores comprise 24 Windows fixture quarantines, 3 opt-in
D1 controls and 2 owned crash children. Counts differ from the historical 155
because 16 additional fixture tests are now quarantined; no coverage is restored.
All F1-F10 cases, the pure founder test, pure attribution and five store-close
checks ran in both cluster suites. F10 outer/shutdown took5.169/5.229s and F9
5.550s in nextest; these accepted deadline cases were unchanged.

Real nextest sample **listing only** used the unchanged `cargo_argv('list')`
with `--offline` added. It matched exactly the original eight and excluded R1-R5
and issuance. No ignored fixture or diagnostic sample was executed. Listing took
1.234s. Founder tests have no OS skips; static guard mutations enforce that.

## Production identity and diff-of-diffs

For each of the six feature paths, both blob identity to reviewed `c45089f`
and exact diff equality passed:
`git diff c612152 c45089f -- <path>` equals
`git diff 957184f b8aadf6 -- <path>`. The manifest includes full SHA-256 of each
diff. Production files and tests are unchanged, including the reviewed F6
wrong-key and wrong-cluster correction.

| Feature path below crates/arxd/src/cluster | Git blob |
| --- | --- |
| `consensus/found.rs` | `b4de3dac384a3f1d7c004d81f4472a5bb4ded20c` |
| `consensus/found_tests.rs` | `60c41dc5a3d0f585ef945e6e28ea878af782577a` |
| `consensus/issue.rs` | `101f0175f318902d118d08566ab3623fb0912b83` |
| `enrollment/registry.rs` | `ccf1535e79709c929ffd14c5bfc532253dbb2736` |
| `pki/ca.rs` | `288be77634295529df6e0fb8e85fff46b9e4cb12` |
| `pki/tests.rs` | `e531935a995344e08172760e92cd5e8fec42922e` |

The guard comparison removes only the two added founder functions and one test;
the remainder equals **all** prepared/current-main guard bytes. Each addition
also occurs byte-for-byte in the retained reviewed guard. Frozen main blob pins
in the manifest cover consensus.rs, adapter, mem_net, three_node, issue_tests,
pki.rs, Cargo files, sampler source/test/profile. The entire .github tree has no
diff. The retained latency hook patch is unchanged evidence only, never applied.
No TLS verifier file exists in either tree. Source delta is exactly the six
reviewed feature files; no helper, timing, retry, workflow or policy edit.

## Retained logs

Logs and their manifest live in the unique D:/Temp parent above. SHA-256:

- `fmt.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `clippy.log`: `65d042b9bdc8ec34cef4aa1d99f5f26931eef3ef74b97e39d6f2dd0a0e76baf9`
- `nextest.log`: `c3361c57611511b5b5f69d8a2123d3c0931497544cbfbdfbcbcc3e5e5a28e915`
- `inprocess.log`: `1d93fa1f266bca1176c49aa26d573a276bd002ee7e8b80eafcb1dc52596b0348`
- `guard.log`: `2e440008878d0b729395d52544e1f02b0e529ab51def87ee798c133b0753fbd8`
- `sampler.log`: `b325d6ce7a50daa5abf2be41c0d6d924da8602d3793d1fdd471e9d9b1c019a49`
- `workflow.log`: `1f250244f8ff945f4a69fcfe157390e6b7c4530e18c7b5939ddc28b190df4991`
- `inventory.log`: `5a5c288e28f9e935f17fc15254258015fd120531ec0cb514257182b4943dfb9a`
- `sample-inventory.log`: `7215b66bce10850a02b18051abd54f9879f7e851d63f3a0d239ed3e9860ba273`

Final source scope and strict UTF-8 checks passed; historical PROGRESS Last merged
labels are retained and current state says PR164/main GREEN. Independent Opus
**integration delta/exact-head** review is still pending; older reviews do not
approve this merged head. Hosted founder timing, exact-main refresh and normal
five gates are still unrun here. A later docs-only PR165 refresh may be required;
no source rework follows from text-only changes. No publication authorized here.
Owned Cargo/test processes were observed at zero after completion. All test gate
threads exited with their processes. Build artifacts are retained without deletion.

## Root parent refresh and publication checkpoint

PR165 merged normally as021a89d5. Its tree equals reviewedheadb75006d7;
exact-main CI44055365214 attempt1 passed all3platforms in565s wall, Windows561s.
Root merged021a89d5 into the founder branch; only PROGRESS conflicted. Main health
wording and founder history/status were retained. No crates/tools/workflow/Cargo
diff exists against13a12ee5 after this merge. All6featurefiles remain reviewed
c45089f bytes and full main guard plus reviewed founder additions unchanged.
The Opus13a12ee5 integration verdict therefore still applies to source; the
final publication head needs its own live review/status and5gates. Hosted founder
cost under660s remains unmeasured. No Windows stability or hardware credit.
