# Windows execution choice local evidence

Base main: `cd8907500dcd89fddc0c659590f962cbf03c7ed5` (merged PR181).
Initial feature: `96592cf4e4e0e27a07dca16493aca4b1be4eb943`.
Provider for builder, coordinator corrections and independent first review:
Sol 6.1/high. Independent review is separate from the builder/coordinator.

Initial source/artifact manifests and the 49 distinct selected library passes
are retained under `D:/Temp/arx-execution-choice-build`. Those passes span
compatible revisions; they are not one 49-test run. The first review verifies
their hashes and states their applicability and limits.

Correction evidence in that same task-owned D: directory:

| Artifact | Executed result |
| --- | --- |
| `review-corrections-final.log` | 13 account-choice/store cases passed; includes actual queued worker, held terminal commit, panic/failed audit, all-column/index migration, empty v1 sequence and rollback |
| `listener-stop-final.log` | 1 authenticated local SDK/listener stop case passed; subsequent request rejected without another row |
| `review-legacy-final.log` | 9 legacy pure cases passed |
| `foreground-final.log` | 2 foreground unit cases passed, including nonzero unconfirmed-drain mapping and existing HTTP response drain |
| `source-final-clippy.log` | workspace/all-targets Clippy with `-D warnings` passed |
| `source-final-build.log` | normal production daemon build; no policy fixture |
| `review-corrections-summary.jsonl` | exact commands, exit codes and wall durations, including unsuccessful intermediate checks |
| `review-corrections-paused-stall.log` | unsuccessful paused-clock variant: 12 completed cases, queued-helper case stalled; task-owned test process stopped, exit -1 at about 60 s |
| `source-final-clippy-style-finding.log` | one collapsible-if style finding, corrected before final Clippy |

The final correction manifest pins all changed Rust/test files, selected logs
and the production binary by SHA-256 and size. No hosted check or native runtime
acceptance is implied by these local artifacts. The main comment-only change
after the foreground test does not change behavior; the production binary is
rebuilt from final source.

Ten `activity_v2` integration cases failed at ACL fixture setup before behavior:
`SetNamedSecurityInfoW` returned Access Denied (5), expected 0. Their original
`inventory-activity-regression.log` is preserved. No base runtime comparison
exists, so these failures are not classified as pre-existing. Required hosted
checks remain necessary; the fixture was not changed or bypassed.

Production MCP smoke and restart persistence are pending. The planned smoke uses
fresh private D: state, an ephemeral synthetic agent credential, actual permitted
foreground token, the normal daemon binary and stdio bridge, and `cargo --version`.
It retains sanitized booleans only. Forced daemon restart after an observed
terminal row/child cleanup will be labeled separately from graceful shutdown.
Current-user smoke is not Claude Code acceptance or completion of M2.

Schema v4 is an operational one-way door for an older binary: preserve a private
backup while the old daemon is stopped before upgrading existing state. Restoring
that backup loses post-backup activity. Initial smoke touches no owner history.
See [account setup and rollback](../windows-execution-accounts.md).

M2 OPEN / M3 UNCHECKED; dedicated, elevated/service, VM and other OS qualification
remain **unverified on real hardware**. PR182 HOLD/NOFIX and exhausted engine
counters are unchanged. No rerun/dispatch or timeout/quarantine change occurred.
