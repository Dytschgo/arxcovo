# L1 learner catch-up: independent Opus source review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent,
no children, 2026-11-17. Local only: no push, PR, status, comment, hosted
action, install, native/admin/VM step, cache deletion or source edit.

Verdict: **PASS for source review; no blocker.** The L1 test meets the brief
and the Root L1 contract check on all three axes. Three low-severity doc
findings (R1-R3) should be fixed before publication; they change no
behavior. This is source review only. It does not authorize merge,
publication, production admission or promotion.

## Target

- Head `16873ce1c1c8af8a2c91b32c72db58082590e5db`, reviewed in an isolated
  worktree at exactly that commit. Base (merge-base)
  `fa8378889c05c6b87d191610e07e3bef68c2ebca` (PR167).
- Five changed paths: `crates/arxd/src/cluster/consensus/tls_engine_tests.rs`
  (+348, new test only), `tools/ci/test_windows_cluster_quarantine.py`,
  `docs/briefs/m3-learner-admission.md`, `docs/evidence/m3-learner-catch-up.md`
  and `docs/PROGRESS.md`. Nothing changed in production code, helpers,
  `mem_net.rs`, `three_node.rs`, `pki/tls_tests.rs`, `found_tests.rs`,
  `issue_tests.rs`, Cargo, lockfile or workflows.
- Sol source `aa0c771a`. Root `fa7bc545` changes exactly two lines,
  `.learner_ids()` to `.non_write_voter_ids()`. The cached OpenRaft 0.10.0
  `membership.rs` defines `learner_ids()` as a deprecated body that calls
  `non_write_voter_ids()`, so the semantics are identical. `16873ce1` changes
  docs only.
- The base test file is a byte-exact prefix of the head file. All blobs are
  ASCII and use LF; PROGRESS.md had non-ASCII bytes before this change, and
  the added lines are ASCII.

## Standards axis

- PASS. One `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`
  with no `cfg`, `cfg_attr` or `ignore`. Imports reuse `super::*`, where
  `Instant` and `timeout_at` are tokio's and BTreeSet/BTreeMap come from
  `consensus.rs`. Blocking registry and rustls work runs in
  `spawn_blocking`, the same as F2.
- Guard: `PROOF_TOKENS` turns the F2 body check into a per-name map.
  The L1 required tokens are the brief's seven, and the forbidden tokens are
  the F2 list plus `change_membership`, `initialize(`, `.install(`,
  `AddVoter` and `authenticated`. Each token has an inject mutant and a
  remove mutant, and the F2 checks are kept. TLS_REQUIRED has 3 names, so
  `tokio::test` count = 3 fixture + 3 required. The existing drift and
  added-gate mutants now cover the new name. Quarantine 27 (13 three_node +
  11 issuance + 3 TLS fixture) and sampler 8 are unchanged.
- Fowler smells (judgment calls; repository standards prevail):
  Long Function (~345 lines) and Duplicated Code (four config builders,
  three `ConsensusNode::start` calls, three voter/non-voter assertion pairs).
  Both are accepted. The guard checks tokens per body, so a step-by-step body
  is the repository's convention for proofs, and moving code into helpers
  would hide tokens from the guard. Mysterious Name: `l` for the learner log
  index is terse (nit, optional `learner_index`). No other smell is material.

## Spec axis (brief steps 1-9 and Root L1 contract check)

- 1-3: there are two `private_temp()` directories, each asserted private,
  and one shared default `MemRouter`. `fkey` and `jkey` are distinct and
  asserted unequal. Node 2 opens with exactly `founder_init([1; 16])` and
  starts uninitialized (`last_log_index None`, term 0). Its own registry
  refuses `(1, fd)` and `(2, jd)`. No write targets node 2. The four configs
  are built once with production builders and `FixedTime` and kept until
  the last line.
- 4: both directions refuse with NotMember before catch-up. In both pairs
  only the joiner's verifier can refuse, because `freg` already holds node 2.
- 5: `fresh_read` `authorize_peer(2, jd) == Ok`, then a non-blocking
  `add_learner`. Node 1 has voters {1} and learners {2}.
- 6: `applied_index_at_least(L)` on node 2 uses the remaining time of the one
  deadline. Then `(applied, snapshot)` equals node 1's
  `linearizable_state()`. The post-catch-up fence asserts voters {1} and
  non-write voters {2} on **both** nodes, plus Learner and leader 1 on
  node 2. This is the normal-code voter recheck that the root correction
  required.
- 7: own-view handshakes give `(2, 1)` and `(1, 2)` with unchanged configs.
- 8: with node 2 isolated, a second Mint commits with an outcome and voters
  stay {1}. After isolation is lifted, node 2 applies it.
  `router.snapshots == 0` proves the catch-up used log replay only.
- 9: node 2 is isolated, then `router.nodes` is cleared for **both** engines
  before any shutdown (root correction). Node 2 shuts down Ok and its Weak
  log/sm handles expire. It reopens on the same directory with
  `founder_init`, not routed. Its applied bytes and snapshot equal the values
  before shutdown, and its own registry accepts `(1, fd)` and `(2, jd)` and
  refuses `(3, jd)`. The reopened node and node 1 both shut down Ok and all
  Weak handles expire. The configs are dropped last.
- Transport limit: the comment above the test and the evidence both say
  catch-up uses mem_net's default `sender == source` with an empty
  `authenticated` map, which is not authenticated catch-up. The guard
  forbids `authenticated` in the body. The claims do not overreach.
- Windows: one voter and one learner, no `Cluster`. `ci.yml` runs a plain
  `cargo nextest run --workspace --locked`, so the test runs on all three OS.
  Hosted Windows has not run it yet.
- M3 placement: the patch inserts a valid `change_membership(AddVoters)`
  after catch-up and snapshot equality and before the existing fence. It
  failed at line 820 with voters `{1, 2}` vs `{1}`. That is a real promotion,
  not a NotLearner API precondition, and the mutant adds no assertion.
- Deadline and cancellation: one tokio `deadline` at entry, `timeout_at` on
  the whole body, and both Raft waits use
  `deadline.saturating_duration_since(now)`. No new constant. The shutdown
  budgets of 5 s each sit inside the 20 s budget.
  Advisory A1 (not a blocker, same as the existing F2 pattern):
  `ConsensusNode` has no `Drop` impl. If the test panics or times out, the
  engines are dropped without `shutdown()`, and the `TempDir`s can drop
  while Raft tasks still hold store files, which can leave temporary files
  behind on Windows. This happens only on the failure path and needs no
  change in L1.

## Security axis

- PASS. Test-only. The join codes come from fixed test bytes, keys are
  generated in memory, and the directories are asserted private. No secret,
  owner data or external request appears in source, logs or evidence. M2.log
  contains state bytes from the deterministic test, not owner data.
- Raw `add_learner` bypasses the 64 proposal permits and `Owners`. It is
  stated as test-only feasibility, not an admission path. The brief keeps
  D1-D3 open, and the root note correctly makes O2 subject to CA-key forgery
  and replay-order concerns. No trust anchor, bootstrap shortcut or seeded
  joiner state is introduced. The bootstrap cycle is observed (step 4), not
  bypassed.

## Findings

- R1 (low, docs): the brief's top `Status:` line still says "verification
  STOPPED" and "CI 44069931184 is pending". The evidence title still says
  "stopped local round", and its retained text says "not ready for
  publication". Root sections appended below supersede these lines, but
  someone reading only the headline gets the wrong status. Update the status
  and title lines before publication.
- R2 (low, evidence): the root round's `mutants.py`, `results.json` and
  `mutants.json` are not hashed in the evidence. Their observed values are:
  `6cd886bfc1d5375bf97420cabd00177a18eda2d00db36f5da50e30848b240058`,
  `12ee3142d49bb79328e30faeb7139b95bf815c3faf0e88bb3ba4e5f99caa9adc`,
  `28fbb5ccbb001e5dc6d2f3ae3e2ae61b2c233815c489d0875ba6dfc711eb91a0`.
- R3 (nit): PROGRESS says "two deprecated API aliases". It is one alias at
  two call sites.
- A1 (advisory): failure-path engine cleanup, described above.

## Checks: actual vs reused

Actual (this review):
- Resolved head and base, the diff stat, a byte-exact prefix check against
  the base, ASCII/LF checks on the blobs, and confirmation that fixtures and
  production paths are unchanged.
- The HEAD test file SHA-256 is `852d2e4c...a127da622`. It equals `fa7bc545`
  and the before/after values in all three restore records.
- Recomputed SHA-256 of all 13 hashed artifacts under
  `D:/Temp/arx-l1-root-u6lxwfst`; every one matches the evidence.
- Read every log. focused27 lists 27 PASS: 13 founder, 10 `pki::tls_tests`,
  `tls_single_node_directory_precondition`, live_verifier, F2 and L1
  (0.679 s). M1 panicked at 763/771 because a handshake succeeded where a
  refusal was expected. M2 panicked at 814 on snapshot inequality. M3
  panicked at 820 with voters {1,2}. None was a compiler error.
- Read the cached OpenRaft source for the alias.
- Ran the pure guard once (`python -I -B -m unittest`, 4 tests OK, 0.92 s).

Reused, not rerun: root fmt, workspace Clippy (locked/offline, all targets),
focused27 nextest and the M1-M3 executions; exact-main CI44069931184 green.
No Rust build or test ran in this review, and there was no concrete reason
to run one.

## Readiness

Ready to go to exact-head publication review after R1-R2 doc fixes (R3
optional). Still required: fresh green exact main, the five normal gates,
hosted Windows `verify` <= 660 s with L1 passing on every OS, and the root
merge. L2, promotion and D1-D3 remain unapproved. M2 open, M3 unchecked;
Sol 3/3 and Opus 0/2 unchanged. All hardware paths remain **unverified on
real hardware**.
