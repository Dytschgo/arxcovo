# Log-store shutdown completion regression

Status: local source/test verification complete; independent review and hosted
checks pending. Sol coordinator, medium effort, on main e601caa. No production
behavior changes. This closes P1 from the PR158 Opus store-close review before
any production restart caller is added.

The original node-level controls held only the state-store destructor. Deleting
the log half of the shutdown acknowledgement wait survived those tests. The
shared test helper now chooses either real store, installs the same DropGate,
then erases the store type for the unchanged lifetime/count/assertion sequence.
The two existing tests still choose State; a new ordinary all-platform test
chooses Log. The assertions require Weak expiration while the gate holds the
real lock, no shutdown completion during500ms, release acknowledgement after
the gate opens, successful shutdown and immediate restart without a retry.
No production timeout, deadline, report or quarantine changes.

## Actual coordinator runs

All cargo commands used `-p arxd --lib --locked --offline` for tests, with owned
D:/arxcovo-close-71607f5-sol/target and its sibling tmp directory.

- Control: `cargo test -p arxd --lib --locked --offline store_close_completion_shutdown`
  passed3/3 in5.07s (D:/Temp/arx-close-log-control.log).
- Mutant: change only the wait to `while !self.sm_released.is_set()` in
  consensus.rs; the focused new log test compiles, then fails at runtime with
  "shutdown returned while store destructor held its lock", exit101 in0.06s
  (D:/Temp/arx-close-log-mutant.log). This is the guard the earlier tests missed.
- Restore: original production bytes restored in a finally block, equality
  checked; same3controls passed in5.09s (D:/Temp/arx-close-log-restored.log).
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings` passes
  in10.68s (D:/Temp/arx-close-log-clippy.log). Format and diff checks pass.

No broad suite was repeated for this test-only addition. PR158's source controls
and140cluster passes stay attributed to their recorded versions. The new test
has not run on hosted CI, so no new hosted or Windows stability credit is given.

## Pins and limits

- `crates/arxd/src/cluster/consensus.rs`: `5b83422bf8d0cc56b7c76ad15609543c40c14e00`.
- `crates/arxd/src/cluster/consensus/tests.rs`: `d804f9b752959c0f1021d49817a5a548baf1e987`.

Only the tests file and documentation change. SQLite/OS close-success is still
not guaranteed by the acknowledgement, and no historical flake is claimed fixed.
No restart caller, dependency, Cargo, workflow, platform ignore, scheduling or
retry change. M2 stays open, M3 unchecked, eight Windows quarantines unchanged.
All hardware paths are **unverified on real hardware**.


## Independent review and publication checkpoint

Opus reviewd2bd45d retaineda3dc153 passes Standards, Spec and Security, closing
P1. Its own control3/3, missing-log-wait mutant (only Log failed), cleanup and
restored3/3 passed; fmt/diff passed. It reused root Clippy rather than claiming
another run. No test source changed after e6e7a88.

A1 is corrected before publication: root's blanket Last merged replacement
incorrectly rewrote two historical sections. Both entries are restored byte for
byte from8e1db8f (PR54 and PR57 respectively), rather than retaining the earlier
corrupted PR137 labels. Only the separately scoped Current state carries PR158.
This is documentation repair, with no new execution, waiver or historical gate
authority. Exact-head publication review and five hosted gates remain pending.
