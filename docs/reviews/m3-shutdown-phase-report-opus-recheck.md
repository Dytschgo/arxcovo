# Opus recheck: shutdown phase report B1 (`ec67681`)

2026-11-16. Opus 5.5 high, fresh, narrow, independent recheck of Sol's B1 correction on
`work/m3-shutdown-phase-report`. The head is `ec67681015b9f184e35a749ffeadd2f207db34c5`.
The prior Opus source review is `0da693b`: Standards PASS, Spec PARTIAL (B1), Security PASS.
This recheck reuses that full source and proposal review and does not repeat it. It makes no
source, PROGRESS, ADR, Cargo or workflow change. It does not push, open a PR, set a status,
merge, watch or start child agents. No other reviewer is active.

**Verdict: PASS. B1 is resolved. Standards PASS, Spec PASS, Security PASS.**

## Scope verified

- `git diff 0da693b ec67681` is exactly one line in `r6_shutdown_report_store_only`:
  `assert!(r.sm_strong >= 1);` becomes `assert_eq!(r.sm_strong, 1);`.
- `git diff 4c909a6 ec67681 -- crates Cargo.lock Cargo.toml rust-toolchain.toml .github` touches
  only that line in `three_node.rs`. The `consensus.rs` blob is `6233249…` and the `adapter.rs`
  blob is `aca96cd…`, both identical to `4c909a6`. The source behavior is unchanged.
- The proposal test (`sm_strong >= 1`, observed 2) and the normal/reaped leg are unchanged.
  The prior review accepted these, and B1 asked for no change to them.
- The new exact value is correct. After `raft.take()`/`sm.take()` and the 5 s weak poll, the
  test's `held` clone is the only remaining `SmAdapter` holder. The head observed `sm_strong: 1`.

## Actual checks run by this reviewer

The copy was a fresh `git archive ec67681` with its own D: target and TEMP/TMP, separate from the
coordinator's target. Logs are local git blobs with path prefixes replaced. They are not published.

| Check | Result | Blob |
| --- | --- | --- |
| `cargo test -p arxd --lib --locked --offline r6_shutdown_report -- --nocapture` (head) | 3 passed; store `sm_strong: 1`, `total_ms` 5014; proposal `total_ms` 5002 | `67c109c1` |
| Mutant B: `log_strong`/`sm_strong` read before `raft.take()`/`sm.take()` | compiles; store-only fails `three_node.rs:868` `left: 2, right: 1`; other 2 pass; exit 101 | `8ea93b3c` |
| Source restored, then `cmp` against `ec67681:consensus.rs` | identical | n/a |
| R6 rerun after the restore | 3 passed | `89c27b21` |
| `cargo fmt --all -- --check` | exit 0 | `e69de29b` |
| `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings` | exit 0; 25.72 s | `17f4ce3e` |

Reused, not rerun: the full `cluster::` suite from the prior review (112 passed, 2 ignored,
8.60 s at `4c909a6`), workspace-wide Clippy, mutant A, and the builder's eight U6 sanity runs.
U6 sanity is not a fix claim. The only source change is one test assertion, so none of these needs
a rerun. Not run: nextest across the workspace, and any hosted CI.

## Status

- This is Sol's second and last seam attempt under the disposition. Behavior fixes used: 0/3.
- The natural hosted U6 cause remains unknown. Induced state-machine latency is only a compatible
  mechanism. M2 remains open. Hardware paths are unchanged and **unverified on real hardware**.
- No A0 read-owner integration is on main (`read_held` stays 0). PR149 stays blocked until this
  parent merges.

## Next

Publication needs a fresh, separate exact-head review of the PR head, all five required gates
green on that head, and the latest exact-main run green. The coordinator's own R6 and fmt run on
its fresh D: target is still running and is not credited here.
