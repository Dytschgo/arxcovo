# M3 TLS verifier resumption onto founder main

Status: Opus 5.5 high static contract, 2026-11-17. Docs only; no build, test, resolve, network or hosted action.
Root contract check, then Sol integration, then Opus review.

## Inputs (local git objects only)

- Main source `c40c5477` (PR166 founder); worktree base `e8df35d4` adds PROGRESS only. Exact-main CI44058803351 was pending when this was written. **No integration build or
  publication starts until it is green.** If it goes red, stop and re-plan on the new main.
- PR159 head `3d702d17`, merge base `e601caa5`. Source approved through B1/R1/R2/R3
  (Opus R3 recheck `6930dc6`). It is still held on Ubuntu S8 (CI43865758301) and on the
  Windows engine failures recorded on `work/m3-pr159-windows-failure` (`640a6dcc`..`d9adeb99`):
  F1 issuance `retained_record_is_required_even_while_member_remains_authorized`, and
  F2 `tls_engine_tests::enrolled_target_binding_revocation_and_stale_acceptance`.

## Conflict map (`git merge-tree --write-tree HEAD 3d702d17`)

Tree `01d80f50`: three textual conflicts; no Rust semantic conflict predicted.
- `consensus/issue.rs`: append/append at the tail. Keep main's `mod sealed` /
  `LeafSubject` / `IssueTicket` impl / `found` binding first, then PR159's
  `impl ConsensusNode { peer_registry }` and its `tls_engine_tests` binding. Both stay byte-exact.
- `docs/PROGRESS.md`: 4 hunks. Keep main's lines and add a new TLS checkpoint. Do not delete history.
- `tools/ci/test_windows_cluster_quarantine.py`: one hunk. Keep main's
  `test_founder_binding_and_seal` and PR159's two remove-binding mutants. The B1 conflict
  is not in that hunk. Git **auto-merged** the issuance loop to call
  `assert_issuance_required` on `consensus/tls_engine_tests.rs`, which forbids every ignore.
  That conflicts with the owner scope and must be resolved as described below. Do not delete the guard.
- Clean in the merge, and they must stay identical to PR159: `pki/verify.rs` `c8c198f4`,
  `pki/tls_tests.rs` `1626658e`, `tls_engine_tests.rs` `8488c63c` (before the gates),
  `mem_net.rs` `4bd971e6`, `pki.rs` `9f8dfcf7`. Main never touched them.
- Dependency identity: compared with main, the merged tree adds only the `rustls = "=0.23.46"`
  line (features ring,std, no defaults) in `crates/arxd/Cargo.toml`, plus Cargo.lock
  `rustls 0.23.46` `48e13bd8…fdf` and `rustls-webpki 0.103.16` `4443b1b6…281`, and the arxd
  dependency edge. Main has no Cargo change, so the existing transitive entries are reused.
- Founder `sign_leaf<S: LeafSubject>`: `tls_tests.rs:29` passes `&test_ticket(..)`, so
  `S = IssueTicket` (sealed, implemented in `issue.rs`); callers need the bound, not the trait in scope.
  `enroll`, `ClusterCa::ephemeral` and `private_marker` are unchanged: no CA call conflict. The `pub(super) use tls_tests::*` glob is shadowed by local
  items: `found_tests` defines its own `NOW`/`leaf`, and main's added `pki/tests.rs` lines use none
  of the exported names. Only the compiler settles this, so Sol's first check is the gate.

## Exact TLS fixture classification

Only tests that construct `three_node::Cluster` (`Cluster::new`) are fixture cases:
1. `enrolled_target_binding_revocation_and_stale_acceptance` (this is F2)
2. `authenticated_sender_mismatch_and_assumed_founder_control`
3. `store_lock_wait_does_not_block_runtime`

These stay required on every OS: `live_verifier_does_not_retain_store_and_poison_refuses`
(one node, `ConsensusNode::start` plus `MemRouter`, no `Cluster`), `tls_single_node_directory_precondition`
(pure), all 10 `pki/tls_tests.rs` tests, including the per-side resumption tests and the
adversarial control, the 13 founder tests, and the pure read-attribution unit. This makes 27
Windows-quarantined fixture cases (13 three_node + 11 issuance + 3 TLS). Ubuntu/macOS execute all 27.

## Allowed changes (Sol high integration; one merge commit plus small follow-ups)

- `tls_engine_tests.rs`: add exactly three rustfmt-shaped attributes, between
  `#[tokio::test(..)]` and `async fn` of the three names, in the same form as `three_node.rs`:
  `#[cfg_attr(\n    target_os = "windows",\n    ignore = "Windows whole-fixture quarantine: docs/adr/0024-windows-three-node-test-quarantine.md"\n)]`.
  Nothing else changes, so the diff against `8488c63c` is +12 lines only.
- Guard: add `TLS_FIXTURE` (3), `TLS_REQUIRED` (live_verifier) and `TLS_PURE`
  (precondition). Give `assert_fixture_quarantine` a keyword `required=()`. Its default must
  leave both existing callers behaving exactly as before. A required test must carry `tokio::test`
  and nothing that skips it. The inventory must equal names+pure+required, and the `tokio::test`
  count must equal len(names)+len(required). Add `assert_tls_engine_quarantine`, which:
  (a) applies the fixture check with `REASON_FIXTURE`;
  (b) splits the source into test segments and requires `Cluster::new(`/`Cluster::unfounded(`
  to appear exactly in the `TLS_FIXTURE` segments;
  (c) keeps the `three_node` import once.
  Route `tls_engine_tests.rs` in the every-OS loop through this check (the loop pattern
  `issue_tests.rs` already uses). `pki/tls_tests.rs` stays under `assert_issuance_required`.
  Module bindings and remove-binding mutants stay.
  Add the file to `test_exact_windows_only_quarantine` with these mutants:
  - per name: removed, wrong OS, Unix family, `all()`, wrong reason, renamed and duplicate gate;
  - an extra tokio fixture, an extra `#[test]`, inner `cfg(unix)` and a stray `#[ignore]`;
  - a Windows gate on live_verifier, and a Windows gate on the precondition test;
  - `Cluster::new` inserted into live_verifier;
  - `TLS_FIXTURE[:-1]` drift.
  Every mutant must fail. Existing EXPECTED/ISSUANCE/REASON declarations stay byte-identical.
- Docs: this brief; ADR0024 TLS addendum (a concrete deviation: three new fixture cases);
  a merge/integration section in `docs/evidence/m3-certificate-verifier.md`; the PROGRESS checkpoint.
  Optionally carry `m3-pr159-windows-engine-failure.md` (`42ec6b57`) and the Opus
  disposition from `work/m3-pr159-windows-failure` byte-identical, so that F1/F2 stay on record.
- **Forbidden:** `verify.rs`, `tls_tests.rs` (bytes), `mem_net.rs`, `pki.rs`, Cargo files
  beyond the merge result, `three_node.rs`, `issue_tests.rs`, founder/CA/registry source,
  `windows_diag_sample.py` (FILTER unchanged: TLS tests sit outside its namespace, so the
  sample stays exactly 8), sampler tests, workflows, helpers, timing, retry, scheduling,
  deadlines, Params and production code. Also no new transport, custody, listener or founder
  TLS acceptance; that is the separate F2 vertical in `m3-next-runtime-boundary.md`.

## Verification (one round; stop at the first natural red and do not rerun or fix)

1. Blob/identity script: the pins above, the Cargo delta, `issue.rs` = main + PR159 tail
   exactly, and the `tls_engine_tests` diff contains attributes only.
2. Guard and sampler suites (60, unchanged), fmt, workspace all-target `--locked --offline`
   Clippy with `-D warnings`.
3. A real nextest listing on Windows shows 27 ignored fixture cases, exactly 8 sample matches,
   and the required TLS (12), founder (13) and pure tests not ignored.
4. A single Windows cluster nextest run (non-ignored set). Exact-filter selection of the three
   TLS fixture cases runs 0 tests (expected exit 4). Do not run the quarantined tests locally,
   and give them no local credit.
5. Then: Opus delta review of the integration, Opus exact-head publication review, all five
   required gates, a Windows job of 660 s or less, and `ci-time-budget`/`agent-review`
   not bypassed. Hosted Ubuntu/macOS must *execute* all 27 cases and every TLS test.

## Holds and authority

- Ubuntu S8: the normal PR159 refresh from main (which now has the PR161 attribution) counts
  as **one** observation. A pass is one sample, not a fix. A red keeps the PR held with its
  attributed line. No rerun.
- **The Windows hold needs owner clarification.** The owner's 2026-11-17 scope authorizes
  excluding all three-node fixture coverage, so the three TLS cases are in class and are not
  a per-test expansion. But the Opus disposition `e7e593eb` holds PR159 "until recovery §6
  re-enable evidence exists or the owner decides". The ADR0024 addendum also forbids
  "further per-test expansion". After quarantine, F1 (already quarantined on main) and F2 no
  longer run on Windows. That hides them; it does not prove them stable. Root must record an
  explicit owner or delegated decision that this scope releases PR159's Windows hold before
  any push. Local integration may be prepared first. The user accepted the loss of fixture
  coverage. No other waiver is implied: not for budget, S8, the gates or the review.
- Counters unchanged: Sol shared fixes 3/3, Opus 0/2 (exception, not fix). M2 open, M3 unchecked.
  TLS, Windows fixture behavior and transport are **unverified on real hardware**.


## Root contract decision - 2026-11-17

Accepted for integration after green exact main. The owner's explicit "Cover all
three-node fixture tests" decision applies to the three identified TLS fixture
cases as well as issuance. Root records that delegated scope decision as releasing
PR159's Windows merge-eligibility hold once the full 27-case guard is in place.
This is one fixture-class boundary, not another per-test expansion or proof of
recovery. Windows stability/re-enable holds remain. All pure/single-node TLS and
founder tests remain required on Windows. Ubuntu S8 gets one new-source observation;
any red stops work, with no rerun. No budget/review/timeout/retry waiver is granted.
Earlier owner-clarification wording above is superseded only by this recorded
application of the existing owner decision; no further permission is inferred.

Main prerequisite satisfied: root verified exactc40c547 CI44058803351 attempt1
SUCCESS on all3platforms, wall632s / Windows628s; all13founder tests pass perOS.
Local integration may start. Publication still needs independent delta review.
