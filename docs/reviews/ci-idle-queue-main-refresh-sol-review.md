# Idle queue policy: current-main integration review

Reviewer: OpenAI / GPT-6.1 Sol / high, 2026-11-17. Independent of the
builder; single review agent, no children. Standards PASS, Spec PASS,
Security PASS for local preparation. No blocking findings.

## Exact scope

- Reviewed prepared head: facc20e86a8d98f014b996461c837309da4693ff.
- Integration builder: fc8e7de7065580119c4313ec838d280599daa4a8.
- Actual main/base: 3022d601f09f24b0e5f092a33f2e9343b500471d.
- Old reviewed PR163 head: 7aefa2cae0abadda594f2a7f4bad02fd8241a92a.
  Remote identity is supplied by root, not freshly queried by this reviewer.
- Assigned isolated checkout only; initial status clean. Reviewed base-to-head
  diff and commit list, all nine policy/test/fixture files, current workflow
  pins, ADR0025, administration route, PROGRESS, new refresh evidence and
  retained Sol design / Opus source / S1-A2 recheck evidence.
- All nine script working bytes and committed blobs equal old7aefa2ca.
  Full scripts delta from old head is empty. Main integration brings current
  workflow/test context without an additional runtime-policy change.

## Standards

No blockers. AGENTS and current KICKOFF routing supersede CONTRIBUTING's older
cross-provider wording. This separate Sol review meets current non-UI routing.
Static comparisons confirm only the documented policy/tests/fixtures and docs
change against actual main. CI and policy workflows, protection payload,
package manifests/lockfile and history committed blobs match main. No changed
runner labels, dependencies, permissions, quarantines, timeouts or threshold.
The entire main Task history remains verbatim, with historical PR54/#57 labels
preserved. Current last-merged PR174 and health fields are accurate.
No new actionable Fowler smell beyond the retained nonblocking design notes.

A1 (advisory): PROGRESS.md:129 still says "failed904s attempt1" in the new
refresh checkpoint without an explicit historical qualifier. The authoritative
header, current-state entry, ADR and activation route correctly use905s and
explicitly supersede the earlier904s read. Mark that checkpoint historical or
use905s on the next documentation update. It does not weaken a gate.

## Spec

No blockers against the accepted definition and preparation scope. The sweep
credits pending-job intervals only when no job executes; charged time therefore
includes the union of execution, overlapping waits, no-pending idle and tails.
Strict timestamp round trips, exactly three distinct names/ids, run/head/attempt1
bindings, labels and bounded single-page jobs reads guard optional credit.
Only all-green runs/jobs receive credit. ci-speed requires both sides green and
creditable and strict reductions in charged time and longest execution. This
amendment supplies no source speedup and cannot qualify itself as ci-speed.

S1 remains closed: raw duration(run), without forced completion, executes before
optional credit validation. Incomplete attempts still throw; defects in optional
metadata fall back to old wall time. Tests meaningfully cover unfinished800s,
unfinished recording, failed-job credit marker, geometry, metadata, label/head
changes, status length/marker/dedupe and history append/deduplication. The old
30-row history is unchanged, SHA256
3b9d9d5494c8aea0ef715348f7e407b1b8cd50016a2c80d8e2776b90ade82e51.

## Security

No blockers. No candidate code execution, new broad catch or permission change.
Transport/8MiB overflow remains fail-closed. Existing advisories about redundant
candidate reads and jobs-before-duration ordering remain fail-closed, nonblocking.
Historical positional-argument, formatting and candidate-workflow trust notes
remain advisory; no repeated source correction or mutant run is warranted.

The prepared route is bounded correctly: definition acceptance is settled;
only after exact-head independent PASS and three green platform jobs may root
ask once for a separate named-executor budget-only bootstrap on that full head.
Before and during the window, zero armed auto-merges; full original protection
snapshot and exact head/base/live-main pins; remove ONLY ci-time-budget; pinned
squash; restore strict/admin and all five original context/app bindings in
finally, even on merge failure, and compare exact readback. Actions15368 and
agent-review null encoded as-1 are explicit. Drift/restoration failure aborts;
no admin bypass, manual success, label workaround or prior exception reuse.

Locally verified PR164 before/after snapshot bytes are identical (SHA256
41e2ed73ab5516bb5df0c7cfd409b9e41000bf4158e7aa6639fd3bac6c78ef41),
with strict/admin true and five original bindings. The saved authorized-action
record hashes to5e4cb8e3a94d11fdcf036f3dff7c5b929877f7e17bd3b126cae711bb7a69190e
and contains strict=true, Actions15368 and agent-review-1 restoration. Historical
readback proves representation, not authority for a new write. Source publishing
does not activate ADR0025; only a separately authorized pinned merge can do so.

## Evidence actually inspected and limitations

Reviewer ran only static Git/Python reads, hash/blob/encoding/scope/history
assertions and git diff --check. Changed committed files are UTF8/LF without
BOM; nine working script hashes agree with root identity.json and old head.
No Node, lint, Rust, engine, mutant, native or hardware execution by reviewer.

Root identity/lint artifacts in D:/Temp/arx-ci-queue-root-refresh were inspected.
Saved command is npm run lint, exit0,6.922s; raw lint.log recomputed SHA256
6cc523691fae5355c9180a5372974a6ae92b938911703530b5046d48c6704925.
Root disclosed the preceding raw CRLF31/LF assertion error; committed policy.yml
blob4925b7f12bdebcd624024e64af5ce9e000743b67 matches main. No source correction,
lint retry or Node retry is inferred. This is reused root execution evidence.

Builder's committed record reports one synthetic node --test scripts/*.test.mjs
run via sorted expansion:48PASS/0fail,8105.1264ms, exit0; hash
0a23148f0c28bfd40d781c3bb62edb51b40e12fcc2e2d863e507f50bed6cf8e6.
It discloses a later cp1252 glyph-print failure after Node success and child lint
unrun for absent dependencies. Limitation: the supplied builder worktree exists,
but .task-evidence is absent; bounded local discovery found no raw child log.
That hash/command/result is attributed to the committed builder record, not
independently recomputed here. Historical Opus runs, mutants and root lint-fix
Node48 evidence are reused, not rerun. Current source identity supports reuse.

Root main-recovery-success.json in D:/Temp/arx-pr174-hosted was parsed locally:
exact main3022d601 / run44126173935 attempt1 completed/failure, timestamps905s;
recorded trusted pure duration agrees. Attempt2 completed/success restores only
functional health. Original Ubuntu248s/Windows553s times are retained;
macOS first execution348s. All three raw main log hashes match the saved record;
anchored_preproof_single_voter PASS lines confirm0.182s/0.310s/1.020s. The record
reports108 tracked PASS eachOS and fixture28 on Unix; reviewer did not independently
reconstruct the complete108-name inventory. Original cancellation/retry JSON
hashes match the refresh record, including the zero-runner/zero-step cancellation.
No live API, policy evaluator, --record, public status/comment, push, watch,
protection/admin operation, install, cache deletion or VM operation was performed.

## Readiness and authority

Ready for root publication preparation and subsequent final exact-head review
with fresh platform checks. This local review is not a remote agent-review status
and does not carry old hosted checks onto the new head. Five normal gates remain
required; ci-time-budget is expected to fail on the905s old-policy baseline.
Functional green is not budget green. Failed attempt1 receives no idle credit
even after eventual activation and attempt2 success. No other merge follows
activation until a new fully green first-attempt exact-main push and normal
gates pass. Rollback is a reviewed revert through the same gates; temporary
protection restoration is mandatory independently of merge outcome.

Reviewer grants no activation, bootstrap, protection or merge authority.
M2 open / M3 unchecked; Sol3/3 and historical Opus0/2 unchanged. Windows fixture28
remains unverified on hosted Windows; native paths **unverified on real hardware**.

Findings: Standards0 blockers/1 advisory (A1, stale checkpoint duration);
Spec0 blockers; Security0 blockers. Raw child-log availability is an explicit
evidence limitation, not a claimed fresh test pass.
