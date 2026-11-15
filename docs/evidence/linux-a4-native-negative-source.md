# Linux A4 native omitted-S7 source evidence

2026-11-15. Builder: OpenAI gpt-6.1-sol, high; sole implementation sub-agent.
Source contract implementation only. Fresh independent Muse source review is
required before publication. No publication or native execution is granted here.

Base: `ac5f069aab24e5a31e1ab33b5791f08f49aba707`, prepared from merged PR112/main
`f15e9acc46b4534eb9d760b546f358f83a47a12d` as supplied by coordinator.
Source commit: `d29d085088e7181427d24a7d0bdc16d3d7655cdf`.

## Implemented contract

- The positive and fixed omitted-kill ignored entries share one private fixture
  body; each entry fixes its case internally, with no external selector.
  Driver TEST remains `exec::disposable::a4::disposable_linux_a4`.
- Test-only OmitS7 uses wrapper-owned directory traversal tags and held handles,
  without raw descriptor export. It counts reached, omitted and forwarded kill
  writes and removal attempts. Exactly the first runs/cgroup.kill byte `1` is
  omitted with Ok; duplicate attempts, other targets and bytes fail closed.
- A4/A5 observed siblings retain the actual outcome of a single production
  setup call and all prior acceptance checks. A4 rechecks population, membership
  and lifetime immediately before setup. The native clock and five-second setup
  bound are unchanged. The negative never calls the success reap on refusal.
- Only the shared `assert!(accepted)` is caught. The rejection requires the
  assertion panic, completed preconditions, exact KillTimeout, one reached/one
  omitted/zero forwarded writes, zero removals, trusted retained literal leaf
  structure, real population, live sole child and positive original lifetime.
  The native fixture additionally verifies unreaped identity and held/fresh leaf
  device/inode equality before the marker.
- B1 marker is fixed `ARX_A4_OMITTED_S7_REJECTED` plus LF, written with raw
  libc::write to STDOUT_FILENO, with partial-write completion and at most 64
  EINTR-only retry iterations. No panic payload, PID or path is exported.
  Driver requires exactly one complete marker; it removes that fixed byte sequence
  before its unchanged exact test-ok/one-pass/zero-failure oracle, because a raw
  write can interrupt libtest's in-progress test line. Mock coverage includes
  this interleaving, missing/duplicate marker and false summary/service success.
- After rejection attribution, checked owned-child SIGKILL/reap precedes checked
  leaf cleanup. Cleanup rechecks held/fresh identity, trusted descriptors and
  bounded emptiness, removes only arx-0-0, and confirms absence and empty runs.
  READY and identity-ack occur only afterward. Existing Drop remains fallback;
  independent driver finally cleanup remains mandatory and fails closed.
- Production algorithms, adapter behavior, ports, helper source/image pins,
  package, Cargo, workflows and ADRs were not changed. LinuxFs additions are
  cfg(test) cleanup/identity checks only. PROGRESS belongs to the coordinator
  and was not edited. No module inventory change was needed.

## Audited verification actually performed

Before running tests, inspected the Fake/Manual/MockChild callgraph and the
Python safe_suite, A1A2, A4Delivery, CgroupScan, F0Blockers and F0b transport/run
mock seams. No pure test enters LinuxFs, helper spawning, ignored fixtures,
owner/admin operations or actual driver dispatch. Source-fence tests only read
bounded repository source. Rust stdin counterfactuals include only parse/setup,
Cancellation and the pure Fake/A4/A5 modules; image hashing remains covered by
Cargo tests. They never include a native adapter or ignored fixture.

Commands (Rust test commands use `-p arxd --lib --locked --offline`):

| Check | Actual result |
| --- | --- |
| `cargo test ... exec::hierarchy::fake::a4:: -- --test-threads=1` | 8 passed |
| `cargo test ... exec::hierarchy::fake::a5:: -- --test-threads=1` | 5 passed |
| `cargo test ... exec::stage_b_tests::p_scan_ -- --test-threads=1` | 3 passed |
| `cargo test ... exec::stage_b_tests::l6_e3_tokenized_source_inventory_and_mutants -- --exact` | 1 passed |
| `cargo test ... exec::stage_b_tests::s12_e4_portable_production_pins -- --exact` | 1 passed |
| `cargo test ... exec::stage_b_tests::w8_e7_ -- --test-threads=1` | 2 passed |
| `python tools/unix-exec-validation/mutation_check.py` | 91 audited mocks, 40 Python assertion mutants killed; 7 pure Rust baseline selections and 8 Rust assertion mutants killed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | passed on Windows |
| `git diff --check` | passed |
| UTF-8/no-BOM/LF check on changed files | passed |
| `cargo check -p arxd --tests --target x86_64-unknown-linux-gnu --locked --offline` | blocked in libsqlite3-sys: missing x86_64-linux-gnu-gcc; Linux fixture not compilation-verified |

Compiled counterfactuals retain the four A1/A2 checks and add forwarded omitted
S7, missing shared assertion, swallowed KillTimeout and premature success reap.
Pure attribution tests also refuse earlier refusal/Ready, missing/duplicate or
wrong-target writes, empty events, expired/early-dead child, missing panic,
removal attempts, foreign/nested leaves and forwarded-write evidence. Python
mocks refuse unknown negative-name variants before effects, helper cleanup
failure, wrong service exit, missing/duplicate markers and false test summaries.

During extension, the stdin harness initially failed compilation because the
extracted A4 omitted its parent import; subsequent anchor checks exposed duplicate
A1/A2 anchors after inlining A5. Added the import and scoped historical mutants
to their original A1/A2 body; final combined run passed. No failed/errored mutant
was credited. Source fingerprint checks confirm counterfactuals do not edit source.
No architecture blocker was found; the cross compiler is a validation limit.
No compiler installation, native/helper/ignored-fixture execution, network,
workflow dispatch, variable operation, push, PR or child agent occurred.

## Exact execution-source blobs

No existing source pin was silently widened or replaced. Historical evidence
stays historical; this table records every changed source blob for fresh review.
Helper image/source/recipe pins remain unchanged.

| File | Old Git blob at base | New Git blob at source commit |
| --- | --- | --- |
| `crates/arxd/src/exec/disposable/a4.rs` | `dfa36d53f0176a8c88b52303a44502ab7f4c82b3` | `b17a961996d0e8184da0262ea21e20d5773d484d` |
| `crates/arxd/src/exec/hierarchy/a4.rs` | `64e1ddb679ed3517e776735933dc59b2442256d6` | `1c7192626056ea4aac5f63f6fea3c71ee9d408a3` |
| `crates/arxd/src/exec/hierarchy/a5.rs` | `416d33808795e05585b6c2055e192ab9f73afb9f` | `71f064ccfb78ff06c75a180a61974d57552ea102` |
| `crates/arxd/src/exec/hierarchy/linux.rs` | `4ad78a672679226fc11c7899a6ef6b033f833a7d` | `7c4196333f4938499188c7f99e5476c955b8e9d0` |
| `tools/unix-exec-validation/driver.py` | `eb085b3e416f9bd8ca08c87c95c84fcd27cafd18` | `7d30c46be219a5249bf911674af40f2592d7b861` |
| `tools/unix-exec-validation/mutation_check.py` | `143bd308d3e4ded15ee8f25b6aa95761bab2bef1` | `d997dd9309fbccb507daad4c9e2a8d7c0d0f1040` |
| `tools/unix-exec-validation/test_driver.py` | `bb0db6f29f1fe72ef17ca9f5258cac99eaff7364` | `f9ec1a36611a70a765d4fb3d81b9df680a8e6931` |

Native negative remains **UNRUN and NOT granted**. Fresh Muse independent source
review, required CI and coordinator integration are outstanding. A separate
reviewed one-run disposition is required for native execution. A4, M2, remaining
F/G and real-Runner PA6 remain open. Installed/dedicated and all hardware paths
are **unverified on real hardware**. No A4 acceptance is claimed.
