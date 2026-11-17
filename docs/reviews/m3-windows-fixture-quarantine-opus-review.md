# Opus independent review: full Windows fixture quarantine

Reviewer: Anthropic / claude-opus-5-5 / high; single independent review sub-agent,
no children. Different provider from the Sol builder. Single-agent review covering
two separate axes (Standards, Spec) plus Security.

## Pinned target

- Reviewed head: `46df55b7a81a76780979b00e834894e762c521a4` (docs evidence).
- Source commit: `221077d0c13131be09d7af86568fdc94e47bbe1e`.
- Prepared base: `b0345f3d253f2ef177b9ecf4ba2a1d6887a4d9d9`.
- Main source: `c6121523417089bc0afaf307470183dd4a7f47ba` (merge-base of the head).
- Previous PR164 head: `17a3fb2a7acbd23024b58e3445333298f860f4be`.
- Full PR164 scope: `git diff c612152...46df55b7` (16 files). Focus delta:
  `git diff b0345f3..46df55b7` (10 files). Worktree clean at review start.
- Spec: `docs/briefs/m3-windows-fixture-quarantine.md` (owner decision
  2026-11-17 "Cover all three-node fixture tests", issuance included,
  Ubuntu/macOS required). Also read AGENTS.md, the evidence file and the ADR0024
  whole-fixture addendum.

## Checks the reviewer actually ran

All local, read-only except this file. No Rust build, no Cargo command, no engine
execution, no hosted action.

1. Independent byte-equality script (git show at `c612152` and `46df55b7`):
   removing only `#[cfg_attr(\n    target_os = "windows",\n    ignore = "..."\n)]\n`
   blocks yields byte-identical `three_node.rs` and `issue_tests.rs`. Attribute
   counts: three_node 8 -> 13, issue_tests 0 -> 11.
2. Each of main's eight original `#[tokio::test]` + cfg_attr + `fn` blocks occurs
   exactly once unchanged at head. `EXPECTED` and `REASON` lines in the guard are
   unchanged from main.
3. Test inventory parsed from head: three_node has 13 async fixture tests, all
   Windows-ignored (8 with the original reason, R1-R5 with the whole-fixture
   reason) and one sync `#[test]` `read_failure_attribution_covers_all_errors`
   with no ignore; issue_tests has 11 async tests, all whole-fixture Windows-ignored.
4. `diagnostics.rs` has no diff; its three D1 controls keep the pre-existing
   global `#[ignore = "D1 diagnostic positive control; ..."]`.
5. `windows_diag_sample.py` differs from `b0345f3` on exactly one line (FILTER,
   line 58). IDs, profile, budgets and the guard-import of `EXPECTED` are unchanged;
   no runtime argv is derived from `WINDOWS_UNSAMPLED`.
6. Strict UTF-8 decode of all 10 delta files at head: all decode, LF only, no BOM;
   both Rust files, the guard, the sampler and sampler tests are pure ASCII.
7. `python -I -m unittest discover -s tools/ci -p "test_windows*py"`: 62 tests OK,
   0.178 s.
8. Static guard probes (import of the guard module, no file writes): rejected
   i4 Unix-only `not(target_os = "windows")` predicate, i4 `tokio::test` -> `#[test]`,
   r1 test attribute dropped, pure unit given the fixture Windows ignore, r1 given
   the sampled reason, t3 given the fixture reason, an inserted `#[should_panic]`.
9. Retained builder logs in `D:/Temp/arx-fixture-pw4wskvb/`: all ten SHA-256
   values in the evidence tables match the files. Read the retained JSON as data:
   full list has 13/14 three_node ignored (only the pure unit unignored), 11/11
   issuance ignored, 3/3 D1 controls ignored; sample list has exactly the eight
   original names matched and R1-R5 `mismatch/expression`.
10. CI: `.github/workflows/ci.yml:69` runs `cargo nextest run --workspace --locked`
    with no fixture filter, so on Ubuntu/macOS the inactive cfg_attr leaves all 24
    executed. Not run here; hosted execution remains required.

Reused (not rerun) builder evidence: fmt, workspace all-target locked/offline
Clippy, real nextest list and ordinary fixture-only run (expected exit 4, 0 run /
571 skipped). Accepted on the matching log hashes above.

## Standards

No blocker.

- S1 (non-blocking, possible Duplicated Code): in `tools/ci/test_windows_diag_sample.py`,
  `test_unsampled_cases_mismatch_and_unexpected_match_gates` and
  `test_ignored_unsampled_expression_mismatch_passes_but_match_gates` now build the
  same shape (five R1-R5 cases, `ignored: True`, `mismatch/expression`, then a
  `matches` must gate). The former non-ignored/`reason: ignored` path no longer
  reflects reality, so the conversion is correct; one of the two could be removed
  in a later cleanup.
- S2 (non-blocking, pre-existing): `assert source.count('ignore') == len(names)`
  counts the substring anywhere, so a future comment containing "ignore" fails the
  guard. It fails closed and existed at main (`len(EXPECTED)`); no action required.
- Small conventional commits, one concern, ASCII source, no Cargo/workflow/policy
  edits: conforms to AGENTS.md.

## Spec

No blocker. Every brief requirement checked:

- 13 three_node async + 11 issuance = 24 Windows-only ignores: met (checks 1, 3, 9).
- Pure attribution unit required everywhere: met; guard rejects any ignore on it.
- Original eight attributes and `EXPECTED`/`REASON` byte-identical: met (check 2).
- D1 controls keep previous global ignore, no new platform exclusion: met (check 4).
- Sample stays the original eight; FILTER changes only by five exact exclusions;
  issuance outside the namespace; no guard-derived runtime argv: met (checks 5, 9).
- Bodies, assertions, deadlines, retry, helpers, production unchanged: met (check 1).
- Guard pins the full 24-gate boundary with seven non-no-op mutants per name
  (removed, wrong OS, Unix family, all OS, wrong reason, rename, duplicate; each
  asserted `!= source`), new async/sync test, extra ignore, `#![cfg(unix)]`,
  inventory drift, pure-unit ignore, plus PKI required and both module-binding
  fences. Old blanket issuance-ignore ban replaced only by the exact set. Met.
- Coverage loss stated explicitly (Windows R1-R5 and issuance have no Windows
  execution; sample is non-gating, no acceptance credit; hosted Windows and
  **unverified on real hardware**): met in brief, ADR0024 addendum, sample brief,
  PROGRESS and evidence.
- Health facts retained: main `c612152` RED (R5, 1475 s wall / 738 s charged
  uncredited), PR164 I4 red, Opus injected-latency follow-up (`4ac89a72`) reproduced
  exact I4 once, counterfactual green, hooks removed, hosted cause/rate unknown, no
  cause or fix credit; Sol 3/3, Opus 0/2; M2 open, M3 unchecked; ADR0025 accepted
  but inactive, bootstrap unauthorized: met.
- Owner exception sequencing: docs say root asks once, after the final exact head
  and the other four gates are green, for a one-use budget-only exception. This
  review grants no exception.

- N1 (non-blocking, pre-existing gap, follow-up): the `#[cfg(test)]\nmod three_node;`
  binding in `crates/arxd/src/cluster/consensus.rs` is not fenced. A
  `#[cfg(not(unix))]` there would drop all 14 three_node tests on Ubuntu/macOS
  while the guard still passes. Issue/PKI bindings are fenced; this one was not
  fenced at main either and the brief's allowed source list excludes
  `consensus.rs`, so it does not block this PR. Recommend a later one-line binding
  fence in the guard.

## Security

No finding. Diff touches only test attributes, a CI test guard, one sampler constant
and docs. No secrets, owner data, private paths in source, network or MCP surface.
The evidence names a local `D:/Temp` log directory only.

## Verdict

Standards 2 non-blocking (worst: S1 duplicated sampler test). Spec 0 blockers,
1 non-blocking pre-existing follow-up (N1). Security 0.

Approved for root publication of reviewed `46df55b7` plus this docs-only review
commit as PR164's next head (root must confirm the review commit changes only
this file), subject to: Ubuntu/macOS required jobs actually executing all 24 (not skipping),
the other four gates green, then the single owner budget-only exception question
for the final exact head. No push, PR, status, merge, rerun or admin action was
performed by this review. Windows fixture coverage remains unverified on hosted
Windows and **unverified on real hardware**.
