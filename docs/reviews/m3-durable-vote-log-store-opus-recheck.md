# M3 durable vote/log store: Opus R1 delta recheck

Reviewer: Claude Opus 5.5, high effort, 2026-11-15. Target: Sol/coordinator
`df35253a6f3fa719986420569c80472f8f56900a` (tree `f801b6c2628750f96b5a0ceef287cc303e95196a`) on
`work/m3-durable-log-boundary`. Delta base: retained review `c0ef722e0ae799c0ee0a299bb4e072023ccd6d68`
(Opus `ea8a25c`), [m3-durable-vote-log-store-opus-review.md](m3-durable-vote-log-store-opus-review.md).
Scope: only the R1 delta. I reused the earlier full source/security review and did not repeat it.

**Verdict: R1 CLOSED. Standards PASS, so Security, Spec and Standards all pass.** Merge is still
blocked by the hosted gates listed below.

## Delta scope

`git diff c0ef722 df35253` changes three files: `log_store/tests.rs` (+107 lines, blob `c8c4c13`),
the evidence (blob `7cbd13e`) and PROGRESS. `crates/` has no non-test change, and `log_store.rs` is
still blob `1c50a296`, the blob reviewed before. No manifest, lockfile, workflow, ADR or dependency
changes. `git diff --check c0ef722 df35253` is clean.

## Guard closure (run by me, not reused)

I applied each mutant from the earlier review to `log_store.rs` one at a time. For each, I ran
`cargo nextest run -p arxd --locked --offline --no-fail-fast cluster::log_store::` and then restored
the file with `git checkout`. After every restore, the SHA-256 was `91f69a6c…83fa` and the worktree
was clean. Every mutant compiled. Each one failed exactly one new test, on an assertion, and the
other 12 log-store tests passed. So no compile error or unrelated invariant explains any failure.

| Mutant | Edit | Failing test (line) | Assertion |
| --- | --- | --- | --- |
| M1 | `id <= last` → `false` | `invalid_purge_…` case 0 (784) | `Ok(())` ≠ `Err(OutOfOrder)` |
| M7 | `purged.is_some_and(id <= p)` → `false` | same, case 1 (784) | `Ok(())` ≠ `Err(OutOfOrder)` |
| M6 | `a.index == p.index && a != p` → `false` | same, case 2 (784) | `Ok(())` ≠ `Err(Conflict)` |
| M8 | drop `entries(first)[0].0 <= id` | same, case 3 (784) | `Ok(())` ≠ `Err(Conflict)` |
| M5 | drop `purged.is_none_or(*a > p)` in `verify` | `reopen_rejects_…` (808) | open not `Corrupt` |
| M11 | drop `last <= p` in meta decode | same (802) | `Ok(Meta{last.term:1})` ≠ `Corrupt` |
| M9 | drop read `p <= purged` | `reads_recheck_…` purged case (836) | `Ok([term1 idx4])` ≠ `Corrupt` |
| M12 | tail-ID check → `false` | same, tail case (836) | `Ok([leader2 idx10])` ≠ `Corrupt` |

The tests assert what R1 required:
- The four invalid operations check the exact error. They also check that the cached `meta` and the
  raw meta blob and `(idx, body)` rows are unchanged, both immediately and after reopen.
- The M5 row and the M9/M12 rows are complete blobs with valid checksums (`decode_row` is asserted
  `Ok`). M9 changes the first live row to a lower term than purged. M12 changes the tail to a lower
  leader with no purge. A lower leader is the right choice because `*p > last` would already catch
  a higher one.
- Both read cases go on to assert `Poisoned` for `vote`, a full `entries` read and empty `append`.
- M11 is checked by calling `decode_meta` directly, which is the function the guard is in, rather
  than through a reopen image. This is enough for R1.

## Evidence wording

Item 1 no longer claims historical "first/tail/purged mismatch" coverage. It says the purged and
read-tail guards were added after R1. The 4097-byte attribution is correct. The test at
`tests.rs:98-101` builds a complete row with length 4097 and payload 4097, which is 4164 bytes and
over `MAX_ROW` 4163, so `reader(b, MAX_ROW)` rejects it before `MembershipBlob::new` runs. Zero
length still reaches the constructor. The new R1 section matches the diff.

## Checks

Actual (this recheck): Windows 11, normal user, `CARGO_TARGET_DIR=D:/Temp/arxcovo-m3-target`,
`TEMP=TMP=D:/Temp`, locked and offline, no installs and no cache deletion.
- `cargo nextest run -p arxd --locked --offline cluster::`: **36 passed, 456 skipped**, 1.233 s,
  run `aa60fb18-d433-45ce-bfed-77bef341623f`. This includes both four-process-kill groups, which
  spawned only owned `current_exe` children.
- The eight mutant runs above. `cargo fmt --all --check` passed.
  `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings` passed.
  `git diff --check` was clean.

Reused, not re-run: the coordinator's 36-test run `2472684f-…` and every earlier full-review check.
I did not run full workspace tests, the hosted three-OS checks, the OpenRaft Suite, or any quorum or
power-loss tests. Only local process-kill consistency is claimed. Power loss, quorum, OpenRaft and
three-node behavior stay **unverified on real hardware**.

## Eligibility

R1 was the only blocker. Its fix touches tests only and is independently rechecked here. The change
can be published once fresh hosted Windows/macOS/Ubuntu fmt, Clippy and cluster gates pass (cluster
<60 s per OS, CI <15 min), along with agent-review and ci-time-budget. Only the coordinator merges.
M2 stays incomplete. A7 stays FINAL NO-GO and the consumed, unconfirmed A12 cleanup stays as it is,
with no retry.
