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

The first production MCP observation ran at source `ba959133`, using fresh
private D: state, an ephemeral synthetic agent credential, actual permitted
foreground token, the normal daemon binary and stdio bridge, and `cargo --version`.
Current-user capability, command, Job identity/cleanup, correlated terminal
activity/privacy and forced-restart persistence passed. The combined observation
FAILED at the subsequent dedicated fixture startup: its synthetic account name
was 21 characters, exceeding the existing maximum 20. This is invalid input;
validation was not changed and no retry occurred. Driver wall time was 0.828 s,
outer wall 0.990 s, exit 1 under a 90-second outer deadline.

The [sanitized first observation](windows-execution-choice-smoke-first.json)
retains only fixed categories/booleans and binary identity. Original private
configs stay outside the repository in the task-owned D: tree. Forced restart
followed an observed terminal commit/child cleanup; graceful shutdown remains
unproved. This Python MCP client is not Claude Code acceptance or M2 completion.
Independent second review keeps source NO-GO for the detached SDK admission race.
The owner-authorized Opus 5.5 attempt must address that lifecycle defect, preserve
this failed observation and prepare valid fresh input before another reviewed
new-source observation.

The owner-exception Opus 5.5/high attempt returned a synchronized admission/ticket
implementation and three actual SDK lifecycle regressions. Run 03 on seam-only
baseline fails all three at F1 assertions; run 05 on corrected source passes 31
focused cases in one run. Foreground 2/2, fmt, Clippy and production build passed.
The returned seven source pins, nine command logs, source diff and binary hash
were independently checked by the coordinator. Full evidence stays at
`D:/Temp/arx-opus-admission-attempt1`; fresh source review is still required.
The new binary is
`f889367307c8561cd813b115b6a6fb908631bf736f233f2d2ca8f7f7d6224b48`, replacing
the earlier binary path while preserving the v1 hash as historical evidence.

Independent [final source review](../reviews/windows-execution-choice-final-review.md)
at exact `7a42ffed` gives source GO: standards/spec pass, S1 and F1 closed, no
confirmed security bypass. It verifies all 14 frozen source pins, seven returned
pins, nine command logs, SDK/diff/binary/script identities and separately permits
one v2 production observation. Required hosted gates remain mandatory.

That [v2 observation](windows-execution-choice-smoke-v2.json) now PASSES all 12
checks on the exact reviewed normal binary, with no policy fixture. Driver wall
time 1.203 s; outer wall 1.319 s, exit 0 under a 90-second deadline. It verifies
omitted-mode current-user inventory, real `cargo --version`, actual Job identity
and cleanup, durable correlated terminal activity without raw owner identity or
rights, persistence across forced daemon restart, and durable dedicated-mode
refusal without fallback. Only task-owned hidden processes were launched/stopped.
No graceful signal shutdown or Claude Code acceptance is claimed. Original v1
FAIL evidence is retained. Hosted checks and M2 acceptance are still pending.

Schema v4 is an operational one-way door for an older binary: preserve a private
backup while the old daemon is stopped before upgrading existing state. Restoring
that backup loses post-backup activity. Initial smoke touches no owner history.
See [account setup and rollback](../windows-execution-accounts.md).

M2 OPEN / M3 UNCHECKED; dedicated, elevated/service, VM and other OS qualification
remain **unverified on real hardware**. PR182 HOLD/NOFIX and exhausted engine
counters are unchanged. No rerun/dispatch or timeout/quarantine change occurred.
