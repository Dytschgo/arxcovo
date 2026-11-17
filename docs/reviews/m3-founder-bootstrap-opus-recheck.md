# Founder bootstrap F1 — Opus correction recheck

Reviewer Opus 5.5, high effort, fresh bounded recheck. Target root Sol
`c45089ffd62d80f799d3a168773bf4f8e624e839`, parent `5284df3` (full Opus review of
`2c6c096`). Scope: R1 encoding and R2 assertion strength only. No broad review restart
and no new smell hunt.

**Verdict: PASS for the R1/R2 correction. R1 and R2 are closed.** The verdict covers source
only. Publication and merge stay held, and this recheck grants no gate waiver.

## Source identity (run by this reviewer)

- `git diff --stat 5284df3 c45089f` shows 3 paths: found_tests.rs +18, PROGRESS.md +2/-2
  and the evidence file +36/-2.
- `git diff --quiet 2c6c096 c45089f` passes for found.rs, issue.rs and ca.rs, so production
  is byte-identical. found.rs blob is `b4de3da`, which matches the review pin. found_tests
  is now `60c41dc`.
- The worktree was clean at HEAD `c45089f` before this file was written.

## Standards

- **R1 closed.** At `5284df3`, PROGRESS had 2 invalid bytes (0x97, 0x96) and the evidence
  file had 4 (0x97 and three 0x96). I mapped 0x96 to U+2013 and 0x97 to U+2014 in the parent
  bytes. The result equals PROGRESS at `c45089f` exactly. It is also an exact prefix of the
  evidence at `c45089f`, followed only by the appended root section. So exactly six bytes
  changed and every other byte is preserved. Python strict `decode('utf-8')` passes for
  PROGRESS, the evidence file and the review file.
- The new test block matches the style of the surrounding F6 code: `assert_eq!` on
  `Result`, with no new helpers or production hooks. No new findings.

## Spec

- **R2 closed.** `founder_guard` (found.rs:157-173) runs one fresh read of
  `authorize_peer(cluster, FOUNDER, key)` and checks the leader first. It then maps
  `WrongCluster` to `Rejected(WrongCluster)` and every other refusal to `Revoked`.
  `authorize_peer` (registry.rs:50-64) checks the cluster before membership and key.
  The new F6 assertions:
  - Current leader comes from `fresh_read`, cluster `[1; 16]` and the founded key give
    `Ok(())`. This positive control shows the ticket reaches the registry branch rather
    than stopping at `StaleIssuer`.
  - Another key, asserted `!= key`, gives `KeyMismatch`, which maps to `Revoked`. This kills
    M2, the registry bypass.
  - Cluster `[2; 16]` with the correct key gives `Rejected(WrongCluster)`. This pins the
    one distinct mapping arm, as brief item 4e requires.
- `NotMember` also maps to `Revoked` through the same `_` arm. A FOUNDER-node ticket cannot
  reach it after founding, so its coverage is not required.

## Security

- No production, CA, custody, transport or seal change. The test reaches `founder_guard`
  and `FounderTicket` only from the existing private test module. The seal surface and the
  `pub(in crate::cluster) mod found` (S1, accepted) are unchanged.

## Root evidence verified (reused, not re-run)

All six `D:/Temp/arx-founder-r2-*.log` SHA-256 hashes match the evidence exactly. Their
contents agree with what the evidence claims:

- control: E0308 compile failure only, with no runtime test executed.
- control-corrected: 1 passed, 0.09 s.
- bypass: `left: Ok(())`, `right: Err(Revoked)`, FAILED.
- cluster-map: `left: Err(Revoked)`, `right: Err(Rejected(WrongCluster))`, FAILED.
- restored: 1 passed, 0.10 s.
- clippy: Finished in 31.15 s.

The guard 3/3, fmt and diff results are reused from root. I did not repeat them.

## Actual checks by this reviewer

- `cargo test --locked --offline -p arxd --lib f6_adapters` at `c45089f`, with
  `CARGO_TARGET_DIR=<worktree>/target-founder-local`: **1 passed, 0 failed, 584
  filtered, 0.09 s.** There was no natural red and no rerun.
- UTF-8 strict decode, the byte-mapping comparison, the production diff and the
  log-hash check described above.
- I ran no mutants. The root mutant logs are hash-verified and show the expected left/right
  values, so no new mutation was needed. No source file was edited.

## Reused, not repeated

The 30/30 review results and 155 builder results on byte-identical production, the natural
cluster loop, the engine and broad suites, the controls, Clippy and the Python guards.

## Limits and status

- Unrun here: broad, natural, hosted and Windows `verify` runs, the 660 s hosted timing,
  fresh exact main, and the five normal gates. The new `target-founder-local/` in this
  worktree is untracked build output. It was not deleted and is not committed.
- Main `c612152` is red (R5 and the 1475 s budget), and the mac queue is 1141, as supplied
  by the coordinator. PR162's source passes, but its platforms are pending and its budget
  fails. PR159 is held. There is no gate waiver and no Windows fix credit. This was root
  small review correction round 1, not a behavior fix.
- Counters are unchanged: Sol shared 3/3, Opus recovery 0/2. M2 open, M3 unchecked.
  Multi-host, transport and custody are **unverified on real hardware**.

No push, PR, status, merge, watch, dispatch, native/install/admin/VM work, cache deletion
or child agent was used. The only repository change is this file, as one local commit.
