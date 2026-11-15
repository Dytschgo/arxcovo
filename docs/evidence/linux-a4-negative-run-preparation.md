# Linux A4 negative one-run preparation

2026-11-15. Implementation sub-agent: OpenAI gpt-6.1-sol, high.
Local preparation only. Independent Muse exact-head review is still needed.
Native omitted-kill is **unrun**; no native authority exists yet.

## Exact starting point and candidate binding

- Accepted main/source baseline: `d0b4c5530b1c4c084a35fe40499afa96a2678b0b` (PR #113).
- Baseline tree: `9cfd66dbdf7ce1afc43c1f5acecf4eb62fd09455`.
- Reviewed source head: `15c4d73a3eaf02779672870e5054e34edea6f5c5`; local Git confirms its
  tree equals that baseline tree. PR113 all-five-gates and Muse PASS are supplied
  coordinator evidence; they were not queried or re-proven in this task.
- Actual checkout start: `0d003cbdfb02772918690fb2989ce1c938efa637`, tree
  `9db370ba36316f5085f90d8f0c489661984da337`. Its only delta from baseline is
  coordinator `docs/PROGRESS.md`; this task does not edit PROGRESS.
- Prepared source/policy commit: `a28ccbd84104fd3f94178175a449012f527abc64`, tree
  `ae5130889246c1cc03a9c7662df1de2e6afbbb04`. This is a local implementation commit,
  not the future reviewed head or main dispatch candidate.
- Independent preparation reviewed head, final main squash SHA/tree and dispatch
  run ID: **not yet available**. No future SHA is asserted.

[ADR0019 D7f](../adr/0019-unix-native-execution-bridge.md#d7f-one-fixed-native-a4-omitted-s7-kill-run-2026-11-15)
contains the conditional disposition. Before grant, the coordinator must record
actual independently reviewed preparation head/tree and verify its three changed
execution blobs against the table below, with every other non-document blob
identical to the baseline. Then require independent exact-head review, all five
required checks and coordinator merge, verify reviewed-head/main-squash tree
identity, and bind only the first eligible full main squash containing D7f.
Only the exact three non-doc paths below may differ; no source substitution,
workflow/helper/pin/package/Cargo change or unrelated source/main content is
eligible. Allowed doc paths are exactly those enumerated in D7f. A changed
candidate, advancing main or mismatch requires a new reviewed disposition before
grant. This uses whole-tree Git comparison, with no new inventory infrastructure.

## Changed execution blobs (Git object IDs)

| Path | At accepted baseline | Prepared blob requiring independent review |
| --- | --- | --- |
| `tools/unix-exec-validation/driver.py` | `7d30c46be219a5249bf911674af40f2592d7b861` | `2180b594616acd3f811d6a2c0bd694b20fc29ce0` |
| `tools/unix-exec-validation/mutation_check.py` | `d997dd9309fbccb507daad4c9e2a8d7c0d0f1040` | `00740ee6085f01ab94c63835feb5e49999ef5168` |
| `tools/unix-exec-validation/test_driver.py` | `f9ec1a36611a70a765d4fb3d81b9df680a8e6931` | `2684cc6a4125ae1e880e535950e4b4d606d9aebd` |

Driver delta is one TEST assignment to the literal
`exec::disposable::a4::disposable_linux_a4_omitted_kill`. Test delta changes both
literal default assertions, renames the focused selection test, and supplies the
fixed rejection marker in the default exact-summary mock when negative is
selected. Every prior assertion remains; the four historical exact-summary
modes and explicit historical positive A4 delivery mocks remain covered. The
inverse selection mutant now restores positive A4. No fixture or production
source was changed.

Selected unchanged execution blobs below are cross-checked at baseline and
prepared source commit. The full non-doc Git delta, not just this sample, was
checked to be exactly the three paths above; all other tracked non-doc paths
are byte-identical.

| Path | Unchanged Git blob |
| --- | --- |
| `.github/workflows/unix-execution-experiment.yml` | `dbcca2276307357301719ed68a09631b8ab7a10a` |
| `crates/arxd/src/exec/disposable/a4.rs` | `b17a961996d0e8184da0262ea21e20d5773d484d` |
| `crates/arxd/src/exec/hierarchy/a4.rs` | `1c7192626056ea4aac5f63f6fea3c71ee9d408a3` |
| `crates/arxd/src/exec/hierarchy/a5.rs` | `71f064ccfb78ff06c75a180a61974d57552ea102` |
| `crates/arxd/src/exec/hierarchy/linux.rs` | `7c4196333f4938499188c7f99e5476c955b8e9d0` |
| `tools/unix-exec-validation/a4-helper/helper.rs` | `5a02cf43d8abaa29e221be4ac54221e2ae05354f` |
| `tools/unix-exec-validation/a4-helper/build.py` | `5412b0cf345d3bd5dd0106e972a6b10e12827b7b` |
| `tools/unix-exec-validation/a4-helper/linux-image-pin.json` | `43e23c1b8597eead804e78cdf69c58139a788bc0` |

## One-run procedure and native oracle

D7e and all older grants stay consumed; all previous hosts stay excluded.
Positive run `43666413693` on `a328060000b8cb7bb389d39f8785f96f6bcb0c25`
remains feedback only, as independently assessed in
[m2-a4-hosted-baseline-muse-assessment.md](../reviews/m2-a4-hosted-baseline-muse-assessment.md).
Accepted [plan](../reviews/m2-a4-native-omitted-kill-plan.md),
[plan review](../reviews/m2-a4-native-omitted-kill-plan-muse-review.md),
[source evidence](linux-a4-native-negative-source.md),
[source review](../reviews/m2-a4-native-negative-source-muse-review.md) and
ADR0020 D9a were read; their implemented fixed source remains unchanged.

After D7f becomes effective, coordinator only: verify fresh hosted Ubuntu 24.04,
repository/actor/triggering actor/ref and exact main/source/workflow SHA equality;
set the reviewed-SHA variable once, read it back, dispatch once, and finally clear
it on completion/failure/rejection with independent HTTP 404 verification.
Accepted dispatch consumes D7f regardless of outcome; no retry, rerun or reuse.
The delegated disposable-host actions need no human approval. No owner-PC,
owner-account/admin, static-VM or backup action is included.

Native assessment must establish the shared inner positive assertion rejected
under the fixed omitted write, exact KillTimeout, reached/omitted/forwarded
1/1/0, zero removals, populated retained literal leaf and live unreaped sole
helper with original lifetime remaining before failure cleanup. Require the raw
fixed LF marker exactly once, then checked failure-only owned SIGKILL/reap and
leaf cleanup before READY/identity-ack, outer exact pass/one-pass-zero-failure,
real service success/zero exit and independent driver finally cleanup. A green
job alone is insufficient. All unchanged bounds apply. Retain bounded sanitized
result/provenance plus independent native assessment before any A4 credit.
A4/M2/full F/G/real-Runner PA6 stay open; installed/dedicated paths remain
**unverified on real hardware**.

## Audited checks actually run

Before execution, audited `mutation_check.safe_suite`, selected Guards/A1A2/
A4Delivery/CgroupScan, F0Blockers and F0b Transport/Records/EntryRun seams and
Rust stdin extraction. CLI, RuntimeGuards and CopyGuards are excluded by this
suite. Driver run/entry paths use mocked OS/process/filesystem/provisioning seams
or refuse before effects; cgroup scans mock walk/stat; helper delivery reads only
fixed repository provenance and mocks descriptor effects. Rust extraction includes
only parse/setup/Cancellation/Fake/Manual/MockChild/A4/A5; it excludes LinuxFs,
H15, native adapters, ignored fixtures and image-pin hashing test. Only local
rustc compilation and the generated pure test binary execute; no Cargo/network
or helper build/launch occurs. In-memory mutants never write source.

| Command/check | Actual outcome |
| --- | --- |
| `python -B tools/unix-exec-validation/mutation_check.py` | PASS: 91 pure/mock tests, 40 Python assertion kills, 7 pure Rust baseline selections, 8 Rust assertion kills |
| Driver SHA-256 after counterfactuals | `4865f8fe0445105d0dd86fbe0a0265c34ce9a60da7be1c63764a7d6946cf3f82`, unchanged during run |
| Rust source fingerprint checks | PASS, unchanged |
| AST assertion-call count comparison in test_driver.py | PASS, all 201 assertion calls preserved by method/count |
| `git diff --check` | PASS before source commit |
| UTF-8 / no BOM / LF validation | PASS on all four source/policy files before source commit |
| Baseline/reviewed tree and exact non-doc delta comparison | PASS (local Git, as above) |

No failed fix attempt occurred. No native/helper/ignored/LinuxFs execution,
network, dispatch, variable operation, push, PR, subagent, owner action or
PROGRESS edit occurred. No hosted checks or independent candidate review were
run here. Evidence encoding and final diff checks are recorded in the handback.
