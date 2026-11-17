# Opus review: founder integration onto restored main (delta)

Reviewer: Opus 5.5, high effort, single agent, no children, isolated worktree.
Scope: integration delta only. This is a single-agent review on two separate axes
(Standards, Spec). It is a different-provider review of Sol's integration work.
It grants no merge or publication authority by itself.

## Pins

| Role | SHA |
| --- | --- |
| Reviewed head (evidence/docs) | `bfa8cc375e10c04ba12eca892fcf315a0a055247` |
| Source merge commit | `b8aadf693e2c97cdad0f53434d776c267a21639d` |
| Merge parents | `957184f4591592d511101128a4f8f9dcc88d8ef7`, `77398e4d6799a74aee9a073d7491d6ef34487858` |
| Prepared base / source main | `957184f4` / `39b3dd73052abd64c1c3d3042e62f080469f1597` (PR164) |
| Reviewed founder source | `c45089ffd62d80f799d3a168773bf4f8e624e839` |
| Founder merge base | `c6121523417089bc0afaf307470183dd4a7f47ba` |
| Prior Opus review / recheck | `5284df3` / `77398e4` |

`39b3dd73` is an ancestor of `957184f4`; `39b3dd73..957184f4` is three
docs(ci) commits. `b8aadf69..bfa8cc37` touches only PROGRESS.md and the two
integration evidence files.

## Actual checks run by this reviewer

All read-only or pure. No Cargo, no engine, no loops, no mutants.

1. Six feature blobs at `c45089f`, `b8aadf6` and `bfa8cc3` are identical:
   found.rs `b4de3dac`, found_tests.rs `60c41dc5`, issue.rs `101f0175`,
   registry.rs `ccf1535e`, ca.rs `288be776`, pki/tests.rs `e531935a`.
2. Base blobs of issue.rs, registry.rs, ca.rs and pki/tests.rs are identical at
   `c612152` and `957184f`, so the production delta has the same base.
3. Diff-of-diffs: `git diff c612152 c45089f -- <six>` and
   `git diff 957184f b8aadf6 -- <six>` are byte-identical, including index
   lines. Both SHA-256
   `0a03812fde532bf08359591602a66c46499e36ea61d3b1d14991b160dd421d77`.
4. `git diff --name-only 957184f bfa8cc3` lists only the six feature files, the
   guard script and eight docs files. The `.github` tree, Cargo.toml,
   Cargo.lock, crates/arxd/Cargo.toml, scripts/, sampler source and test,
   consensus.rs, adapter.rs, mem_net.rs, three_node.rs, issue_tests.rs and
   pki.rs have no diff against `957184f`.
5. Main movement after the founder base (`c612152..957184f`) in crates/ is
   exactly 16 added `cfg_attr(target_os = "windows", ignore = <ADR 0024>)`
   blocks in three_node.rs and issue_tests.rs, nothing else. This accounts for
   155 historical minus 16 = 139.
6. Guard merge, compared by Python AST source segments:
   `assert_founder_binding`, `assert_subject_seal` and
   `test_founder_binding_and_seal` equal the `c45089f` versions exactly. All
   seven functions/tests of the `957184f` guard are preserved unchanged. The
   only new names are those three. The `957..b8` guard diff is 57 additions,
   0 deletions.
7. Ignore inventory at head: three_node.rs 13 Windows ignores (8 sampled + 5
   unsampled) plus one pure `#[test]`; issue_tests.rs 11 Windows ignores;
   found_tests.rs 0 ignores. 13 + 11 = 24. Other cluster ignores: 3 D1
   diagnostics and 2 crash children (log_store, state_store) = 29 in-process.
8. `pki/verify.rs` (held PR159 TLS verifier) is absent at head.
9. `python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`:
   3 tests OK. `... -p test_windows_diag_sample.py`: 60 tests OK.
   `node --test scripts/ci-workflow.test.mjs`: 4 pass, 0 fail.
10. Integration JSON parses as strict UTF-8 (ASCII) and its inventory lists 24
    quarantined, 13 founder tests (F1, F2, F3, F3b, F4-F9, three F10), 7 pure or
    store-close checks, and the same eight sampled names as the guard EXPECTED.
    New docs decode as strict UTF-8.

## Reused, not rerun by this reviewer

Builder (Sol) actual results on `b8aadf6`, accepted as recorded evidence only:
fmt; locked/offline workspace all-target Clippy; cluster nextest 139 passed /
484 skipped; in-process 139 passed / 29 ignored; nextest list inventory; log
hashes under `D:/Temp/arx-founder-integrate-c042978f201a49d6b21a8dc864439901`.
I did not open those logs or rebuild. Historical 30 focused / 155 cluster runs
and the 15 builder mutants plus the R2 mutation evidence belong to unchanged code
and were not repeated. Prior Opus security/source approval (`5284df3`,
`77398e4`) covers the founder code; this review adds no new security credit.

## Standards

No findings in source. The integration is a pure union: reviewed founder bytes
plus unchanged main bytes, and the guard is additive with no main bytes removed.
Smell baseline: nothing new introduced by the merge; the guard additions were
already reviewed at `c45089f`.

- S1 (low, docs). PROGRESS.md now has the ported `Founder F1 local
  implementation - 2026-11-16` section after the 2026-11-17 entries, so the
  log is out of date order. Its text ("existing eight quarantines", "Root
  independent review then Opus source review ... remain") is historical and
  could be misread as current. The integration evidence already labels it
  historical. Optional: add a one-line "historical, see integration
  checkpoint" note in the next docs refresh (for example PR165). Not blocking.

## Spec

Spec: owner M3 parallel allowance, docs/briefs/m3-next-runtime-boundary.md
(accepted founder contract), and the integration focus in the assignment.

- Preserve all current main fixture guard: met (check 6).
- Add only the reviewed founder two helpers and one test: met (check 6).
- Exactly 24 Windows ignores and sample eight: met (checks 7, 10).
- Founder 13 cases plus pure and store-close required on every OS: met.
  found_tests.rs has no ignore or cfg predicate, and `assert_founder_binding`
  plus `assert_issuance_required` reject any added skip (guard ran green).
- Production diffs identical old vs new: met (checks 1-3).
- Frozen consensus/adapter/three_node/issue_tests/mem_net/pki.rs/Cargo/
  workflow/sampler, no TLS verifier: met (checks 4, 8).
- Counts 139/139 vs historical 155 and 29 = 24 + 3 + 2: consistent with checks
  5 and 7.

No missing, partial or unrequested behavior found.

## Verdict

**PASS for the integration delta at `bfa8cc37` (source `b8aadf69`).** Findings:
Standards 1 (low, docs ordering/wording), Spec 0.

## Root publication readiness

Ready for root to prepare publication, subject to the remaining gates, which this
review does not satisfy:

- exact-head publication review of the published PR head;
- fresh hosted normal five required gates on that head against green exact main;
- hosted Windows `verify` timing under the 660 s rule with the founder tests
  added (founder F9/F10 deadline cases took about 5 s each locally);
- re-read of main immediately before merge.

PR165 (docs only) may merge first; a resulting PROGRESS refresh is not a behavior
change and does not void this delta review if source blobs stay as pinned. If any
of the six feature blobs or the guard changes, this verdict lapses. ADR 0025
exception remains consumed and grants nothing. PR159 stays held. M2 open, M3
unchecked, Sol 3/3 and Opus 0/2 unchanged; hardware paths **unverified on real
hardware**; no Windows stability credit.

No push, PR, status, comment, watch, merge, admin, VM, install, cache deletion or
child agent was used.
