# M3 TLS verifier founder-main integration - Opus delta review

Reviewer: Opus 5.5 high, single independent reviewer, no children, 2026-11-17.
Builder of the reviewed integration: Sol high. This review is my own work; it does
not re-credit earlier Opus approvals as new evidence.

Verdict: **PASS (delta approved)**. No blocking or required-fix findings. PR159 may be
fast-forwarded to the integration for publication, subject to every remaining gate
listed at the end. Nothing here is a merge approval.

## Pinned target

- Integration: `a1fba89dbddd4f3033562da883651c0bd1f7a68d` (merge; parents
  `8676a6f7607c1f9ac5f4ee87b1798f8ba9e34b7c` prepared branch and
  `3d702d17a6175c054c73292aca0385bd8740f2a2` reviewed PR159 head).
- Main source: `c40c547721641974bcb407ce343c795e6c6609ec` (ancestor of the integration;
  `c40c5477..8676a6f7` is four docs-only commits). PR159 merge base `e601caa5`.
- Reviewed scopes: `git diff c40c5477 a1fba89d` (17 files) and
  `git diff 3d702d17 a1fba89d` (the founder side plus the integration additions).
- Spec: `docs/briefs/m3-tls-verifier-resumption.md`, including the Root contract
  decision. Standards: `AGENTS.md` plus the review-skill smell baseline.
- Worktree clean before this file was added. No `.github/` change in the delta.

## Actual checks run by this review

1. Blob identity (git objects):
   - `pki/verify.rs` c8c198f4, `pki/tls_tests.rs` 1626658e, `consensus/mem_net.rs`
     4bd971e6, `pki.rs` 9f8dfcf7: integration == PR159 head. `mem_net.rs` and `pki.rs`
     are identical in the merge base and main, so main never touched them.
   - `Cargo.toml` f0a31a70 and `Cargo.lock` 6bb1f1c0 == PR159; base == main for both.
     Delta from main is exactly the `rustls = "=0.23.46"` line (no default features,
     `ring`,`std`), the arxd `rustls` edge, and two lock entries: rustls 0.23.46
     `48e13bd8...8fdf`, rustls-webpki 0.103.16 `4443b1b6...b281`.
   - `three_node.rs` 0fd3ad55, `issue_tests.rs` b5a00615, `windows_diag_sample.py`
     8002788b, `test_windows_diag_sample.py` 4060b5b3: integration == main.
   - `docs/briefs/m3-certificate-issuance-first-slice.md` == PR159 (main == base).
2. `tls_engine_tests.rs` 8488c63c -> 674f6d53: the diff is exactly three 4-line
   `cfg_attr(target_os = "windows", ignore = REASON_FIXTURE)` blocks, each between
   `#[tokio::test(..)]` and `async fn` of the three named tests. Reason string is
   byte-identical to the one used five times in `three_node.rs`. No body change.
3. `issue.rs`: integration (341 lines) == main (314) + PR159's base->head tail
   (`impl ConsensusNode { peer_registry }` + `tls_engine_tests` binding), verified by
   textual diff in both directions. Main's sealed `LeafSubject`/`IssueTicket`/`found`
   block precedes the TLS tail unchanged.
4. Semantic-interaction read of main's founder changes against PR159:
   `authorize_peer` body unchanged (main only adds `founded`/`next_node_id`
   accessors), `PeerRefusal` unchanged so `peer_registry`'s exhaustive match holds;
   `sign_leaf<S: LeafSubject>` refactor keeps the same validity/expiry semantics;
   `Cluster::new` body unchanged on main (only attributes added); `unfounded` is
   private, so `Cluster::new(` is the only constructor reachable from the TLS file.
   Founder tests use `MemRouter::default()`, whose empty `authenticated` map takes the
   old `sender != source` path.
5. Guard source review (`tools/ci/test_windows_cluster_quarantine.py` 927e47a4):
   `required=()` keyword-only default makes the new `elif` unreachable and leaves the
   inventory and `tokio::test` count equations unchanged for both existing callers.
   `assert_tls_engine_quarantine` applies the fixture check with `TLS_PURE` and
   `TLS_REQUIRED`, requires constructor segments == `TLS_FIXTURE`, and pins the
   single `three_node` import. Routed through the every-OS loop; `pki/tls_tests.rs`
   stays under `assert_issuance_required`; both new module bindings and their
   remove-binding mutants are present. All brief-listed mutants exist; EXPECTED,
   ISSUANCE, REASON, REASON_FIXTURE and PURE declarations are unchanged from main.
6. Python, run locally (`-I -B`, CI-equivalent discover commands): quarantine suite
   3/3 OK (0.877 s), sampler suite 60/60 OK. Total 63.
7. Own extra static mutants against the live guard (scratch probe, not committed),
   all killed: `Cluster::new` in a new helper; required test renamed; required test
   made sync `#[test]`; pure test made tokio; `#[ignore]` on required;
   `cfg_attr(windows, ignore)` alternate spelling on required; inner statement
   `#[cfg(unix)]` in required body; fixture gate moved from a fixture test to the
   required test (count still 3); fixture body with constructor removed;
   `cfg(not(target_os = "windows"))` on required; `tls_engine_tests` binding changed
   to `cfg(all(test, unix))`; extra attribute on the `pki` `tls_tests` binding; a
   Windows gate in `pki/tls_tests.rs`.
8. Retained builder evidence at
   `D:/Temp/arxcovo-tls-integration-24e5c5ed079e4c12943de1079a646549/logs`: all 11
   SHA-256 values recorded in `docs/evidence/m3-certificate-verifier.md` recomputed
   and match. Parsed `list.json`: 32 ignored `cluster::` tests = 27 fixture cases
   (13 `three_node` + 11 `issue_tests` + 3 `tls_engine_tests`, the exact three names)
   + 5 pre-existing unconditional ignores (3 D1 controls, 2 crash-child harnesses);
   required: 12 TLS (10 pure + live verifier + precondition), 13 founder,
   `read_failure_attribution_covers_all_errors`. `cluster.log` summary 151 passed /
   449 skipped; `exact-three.log` 0 run / 600 skipped / "no tests to run".
9. PROGRESS: removed lines are only the superseded volatile status header/bullets;
   the PR165/021a89d5/CI44055365214 history remains in the dated log.

## Reused, not re-run by this review

- Rust compile, fmt, workspace Clippy, real Windows nextest listing and the single
  cluster run: builder evidence, verified here only by log hashes and parsing.
  No new Rust build was needed: no concrete concern arose that bytes outside the
  pinned set could change compiled behavior.
- Cryptographic verifier mutants and B1/R1/R2/R3 security approvals: retained Opus
  reviews; reuse is valid because `verify.rs`, `tls_tests.rs`, `mem_net.rs`, `pki.rs`
  and the dependency set are byte-identical to the reviewed PR159 head.
- Main CI44058803351 attempt 1 green (632 s wall, Windows 628 s, founder 13/OS):
  recorded by root; not re-observed here.

## Standards

- AGENTS.md: one-concern integration; no production redesign, timing, retry,
  workflow, helper or sampler change; tests accompany the new guard behavior;
  unverified-on-real-hardware labels present in ADR, evidence and PROGRESS. No
  secrets or owner data seen in the delta. No breach found.
- S1 (judgement, possible Repeated Switches / Duplicated Code, non-blocking):
  `test_issuance_required_on_every_os` repeats the three-way conditional
  `(assert_issuance_quarantine if ... else assert_tls_engine_quarantine if ... else
  assert_issuance_required)` twice. A single filename->check map would read better.
  Test-only; not worth a new commit before publication.

## Spec

- All "Allowed changes" items are present and the "Forbidden" list is respected
  (checked by blob identity above). The ADR0024 addendum, evidence section and
  PROGRESS checkpoint match the Root decision: release of the Windows
  merge-eligibility hold only, stability/re-enable hold retained, no waivers.
- P1 (residual, not a defect): the three TLS fixture cases have not yet executed on
  the merged source anywhere; local verification was Windows-only, where they are
  ignored by design. Their first execution on founder-main bytes is the hosted
  Ubuntu/macOS publication run, which the brief already requires. Static risk is
  low (step 4), but no credit is given until hosted Ubuntu/macOS execute all 27 and
  every TLS test.

## Remaining gates (unchanged by this review)

Fast-forward PR159 to `a1fba89d`; fresh Opus exact-head publication review; all five
normal gates; hosted Windows job <=660 s; `ci-time-budget` and `agent-review` not
bypassed; hosted Ubuntu/macOS execute all 27 fixture cases and every TLS test; Ubuntu
S8 gets exactly one updated-source observation (red stops, no rerun); fresh green
main after merge. Windows fixture coverage stays quarantined and unverified; F1/F2
remain recorded failures with no stability claim. M2 open, M3 unchecked; Sol 3/3,
Opus 0/2 unchanged. TLS, transport and Windows fixture behavior are **unverified on
real hardware**.

No push, PR, status, comment, watch, merge, admin, native VM, install, cache
deletion, children or loops were performed by this review.
