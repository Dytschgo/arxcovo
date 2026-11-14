# macOS launchd empty-profile implementation evidence

Date: 2026-11-14. Builder: OpenAI / gpt-6.1-sol / high, implementation
sub-agent, no delegation. This is local engineering evidence, not installed
acceptance or an independent review. No publication or merge performed.

## Source pins and isolation

- Initial HEAD: `05aff28fd7df2aee73e966e4443b866f1d9bb149`, porcelain empty.
  Supplied worktree cwd equals Git top level; its own worktree gitdir differs
  from the common gitdir. No checkout, ref refresh, or network operation.
- Actual-main baseline: `4ec11b914039c81f5401fdfbe678201372742964`.
  Initial `git diff 4ec11b9 HEAD -- crates/arxd/src/service` was empty.
- Source commit: `b6c1d74349d66d7f571025fe9150ac9e66737009`.
- Proposal supplied `657d06a`, retained `f560c21`; checked-in proposal blob
  `93f3c6ccc8725e40f542818826551e4b1fd71829`.
- Grok design review supplied `b353a21`, retained `476fe06`; checked-in review
  blob `abf56af2045be6d3e243654918f6fd50d1799035`.
  Both complete documents were read. AGENTS, KICKOFF, CONTRIBUTING,
  engineering safeguards, service source and relevant ADRs were inspected.
  Explicit task scope overrides kickoff delegation and PROGRESS updates.

## Implemented contract

Borrowed LaunchdObservation carries termination, both byte slices, independent
EOF completeness and truncation flags. Neither observation nor capture formats
private bytes through Debug or Display. The accept-set is empty: every input
is Unknown. No parser delimiters, profile, fixture, numeric absence constant,
or print-derived path access is introduced.

Mac Installable/Unloaded treat every Ours state as loaded, including Stopped
and Failed. Linux semantics remain unchanged. Pure macOS status has exactly
three success pairs: Absent+Absent, Identical+Absent, Identical+Ours(Running).
All other pairs return Manager. Manager status calls that helper.

The macOS query verifies the fixed binary using existing protection rules,
then uses the private capture helper and its pure observation-to-query mapping.
Capture keeps at most 65536 stdout and 4096 stderr bytes, marks overflow,
observes each EOF independently, and drains both nonblocking pipes fairly.
Each pass permits at most 65 reads of at most 1024 bytes per stream. Deadline
checks bracket each pass; 10ms pauses stay within the 10s deadline. A process
exit alone never establishes complete output. Nonzero exits still drain.
Spawn failure yields Incomplete without kill; later capture failure attempts
one kill followed by at most one second of nonblocking reap polling. Failure
remains Incomplete regardless of cleanup. There is no retry or stream logging.

Source comparison against actual-main proved `bounded` and Runner::command
byte-identical (after text LF normalization), and owner_plan.rs has no diff.
Linux keeps its 4096 stdout cap and null stderr; mutators retain 35 seconds and
null streams. Existing PATH, LANG, LC_ALL, SYSTEMD_PAGER=cat and SYSTEMD_COLORS=0
are preserved. The separate macOS query clears its environment and sets only
fixed PATH, LANG=C and LC_ALL=C, with cwd / and null stdin.

## Exact checks run

Host: Windows. Rust toolchain pinned to 1.95.0. No test below launches a child
process: query tests call pure functions; capture tests use an injected clock,
pipe and wait transport; lifecycle tests execute owner plans through an
in-memory OsManager. The selected existing query tests were inspected before
execution. Other compiled test bodies were not selected.

- `cargo test -p arxd --lib --locked --offline pure_ --no-run`: passed
  (compile only; no broad test execution).
- `cargo fmt --all`: applied formatting. Initial fmt check reported formatting
  differences; final `cargo fmt --all -- --check`: passed.
- `cargo clippy -p arxd --lib --tests --locked --offline -- -D warnings`:
  initial check found collapsible_if and needless_range_loop in new capture
  code/tests; both fixed. Final check passed, including after mutant restore.
- Each test listed below ran individually using
  `cargo test -p arxd --lib --locked --offline <full-name> -- --exact`.
  Each reported exactly **1 passed, 0 failed**. All 14 were rerun after
  restoration; no zero-test result was credited.

- `service::owner_query::empty_profile_tests::pure_empty_profile_every_observation_unknown`
- `service::owner_query::empty_profile_tests::pure_macos_state_and_status_matrix`
- `service::owner_unix::launchd_capture::tests::pure_capture_exact_caps_nonzero_and_signal`
- `service::owner_unix::launchd_capture::tests::pure_capture_fair_flood_and_overflow`
- `service::owner_unix::launchd_capture::tests::pure_capture_failures_deadline_and_bounded_reap`
- `service::owner_unix::launchd_capture::tests::pure_capture_eof_before_exit_and_wouldblock_fairness`
- `service::owner_unix::launchd_lifecycle_pure_tests::pure_uncertain_and_loaded_install_never_mutates`
- `service::owner_unix::launchd_lifecycle_pure_tests::pure_post_bootout_requires_fresh_absence_and_keeps_plist`
- `service::owner_unix::launchd_lifecycle_pure_tests::pure_bootstrap_and_bootout_gates`
- `service::owner_query::tests::bounded_queries_reject_ambiguous_or_foreign_output`
- `service::owner_query::tests::not_found_cannot_hide_an_active_or_transitioning_unit`
- `service::owner_query::tests::active_unknown_and_foreign_cannot_pass_installable_or_stop_proof`
- `service::owner_query::tests::installed_absence_differs_between_platforms`
- `service::owner_query::tests::gates_and_status_keep_unknown_errors_fixed`

These cover the exhaustive 3-artifact by 10-query macOS status matrix, every
macOS State, preserved Linux inactive/failed behavior, all uncertain gates and
expectations, no install mutation on loaded/uncertain queries, bootstrap only
on absence, bootout on every loaded state, and fresh post-bootout refusal
preserving the plist. Capture cases cover exact caps, flood overflow on either
stream, WouldBlock fairness, nonzero/signal preservation, EOF before exit,
exit without EOF, spawn/setup/read/wait failure, noisy deadline crossing,
never-exiting/never-closing transports and bounded unsuccessful cleanup.
No native descriptor, wall-clock or process-cleanup proof is claimed.

## Compiled mutation checks

For each mutation, the corresponding exact test was rebuilt and run. Credit
required exit 101, completed test compilation, exactly one running test,
a FAILED test result and the named assertion below. Compiler errors, missing
tests and infrastructure failures were explicitly rejected as mutation kills.
All eight met that oracle; no compiler-error kill was credited.

| Mutation | Exact test suffix | Named assertion |
| --- | --- | --- |
| constant-absent | `pure_empty_profile_every_observation_unknown` | `empty-profile-refuses-every-observation` |
| mac-stopped-installable | `pure_macos_state_and_status_matrix` | `mac-all-ours-block-install` |
| mac-failed-installable | `pure_macos_state_and_status_matrix` | `mac-all-ours-block-install` |
| running-without-plist | `pure_macos_state_and_status_matrix` | `exhaustive-macos-status` |
| loaded-status-stopped | `pure_macos_state_and_status_matrix` | `exhaustive-macos-status` |
| unknown-gate-allowed | `pure_macos_state_and_status_matrix` | `uncertain-gate-refuses` |
| unloaded-without-absence | `pure_post_bootout_requires_fresh_absence_and_keeps_plist` | `only-absence-proves-unloaded` |
| failure-no-kill | `pure_capture_failures_deadline_and_bounded_reap` | `cleanup-only-after-spawn` |

Each source was restored in a finally block from its exact original bytes,
then byte equality was asserted. Both mutated sources had zero CR bytes;
these LF SHA256 values matched after restore and at source commit:

- `owner_query.rs`: `b9afb2b10f2c4737dac0908289597c31b63f7627a5a1c48511225740a3cdae5e`.
- `owner_launchd_capture.rs`: `16e70bff7e5580b1774354a57c21a13d1ec772fe3610cff36380ae649273a5ea`.
- `owner_unix.rs`: `17d2a2c892d4885c291f4ca8fa2377807ab28b42408e0f76cede715fa54ad8e7`.

`git diff --check` passed before source commit. Scope inspection showed only
owner_query.rs, owner_unix.rs, the new private owner_launchd_capture.rs and the
narrow ADR 0014 addendum. This evidence is the separate follow-up commit.
PROGRESS, other design/reviews, CI/workflows, Cargo/policy and the optional
Windows harness are unchanged.

## Limits and next gates

No actual launchctl, native query, service/account/process/system inspection,
owner/private-state/VM access, install, dispatch, privileged operation,
network/GitHub/CI action, fixture/evidence capture or raw stream logging ran.
No ordinary_process_timeout_and_output_bounds, echo/false/sleep, full workspace
or native tests ran. Synthetic mock bytes are not compatibility fixtures.

Engineering gaps: fresh independent Grok exact-head source review before normal
publication; hosted Unix type/runtime gates; production native adapter and pipe
behavior; actual process cleanup proof; separately authorized, reviewed absence
and running compatibility profiles; installed lifecycle/boot/restart evidence.
The Unix adapter was source-inspected only: Windows checks do not type-check
cfg(unix) code. Real hardware paths are **unverified on real hardware**.

Owner-only work remains separate: dedicated standard account, protected install
and real service setup/acceptance when separately authorized. No owner action
is required for this slice or substitutes for the engineering gates.

Production dedicated execution remains inactive. Since the parser is always
Unknown, real macOS service operations still refuse service-manager. M2 is
**incomplete**, real-Runner PA6 **OPEN**, Linux F/G **OPEN**. Windows main71 has
cause **unknown**, with no retry authorized. No claim here changes those states.
