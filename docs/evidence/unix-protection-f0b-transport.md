# F0b bounded internal transport: local mechanism evidence

2026-11-14. Builder: OpenAI / gpt-6.1-sol / high. Implementation recovery
sub-agent; no delegation. Source commit: `b59410fcdabbc110f96f6f0166ccb154bafb6a74`.
Prepared clean HEAD: `4172fda230d6b69616adcd05d47afaf751b8a704`.
Cwd equalled Git top level in the supplied isolated worktree, on
`t3/complete-f0b-transport`; initial status was empty. Accepted contract
`fc36a37`, Sol design review retained `7d7aa2f`, and corrected contract
recheck retained `dbcd578` were read. The base contains the accepted contract
and required discovery change; no workflow changes were made.

Implemented three fixed descriptor-relative channels, exact escaping initial
Refusal guards after installed trust, exclusive first-creator writes, strict
bounded canonical reads, Exception-total conditional snapshot, metadata-only
progress lower bounds and v2-only emission/replay. Snapshot follows the stop
attempt and precedes the independent quarantine/cleanup fences. Stop return
code is ignored. First/final errors and cleanup disposition remain actual;
otherwise-successful telemetry anomalies produce only E_RECORD/exit 1.
README carries coverage and inference limits. No production uid override or
other test-only trust seam was added.

## Executed local checks

Python 3.12 on Windows; no Windows native/Rust tests or driver CLI were run.
All transport/run/entry effects use mocks. Existing owned temporary-file tests
remain confined to their temp directories. No real root, proc/sys/cgroup,
accounts, services, VM, old installation, owner state, network, GitHub, CI or
dispatch operations were performed. Optional POSIX kernel checks were not run
and earn no credit.

Local execution chunks are identifiers for this tool session, not GitHub runs.

| Local run | Command/result |
| --- | --- |
| `79dde1` | Safe suite: **63 passed**, 0 errors, 0.6927393s tool wall time; 46 existing tests plus 17 new methods |
| `f57f8e` | Legacy compiled runner: **23 assertion kills**, 0 errors, no equivalents |
| `f824d1` | F0b compiled runner: **37 candidates: 33 assertion kills, 4 equivalents**, 0 errors |
| source/diff | `git diff --check` passed; only the five authorized source/test/README paths changed before this evidence file |

The existing `Guards.test_cli_wrong_platform_refuses` was deliberately excluded:
it invokes the driver CLI in a child process, outside this task's execution
authority. Thus this is 63/63 selected tests, not a claim that ordinary CI or
all 64 discovered tests ran. CI status is not checked here; the supplied
Windows ExecutionFailed report was not diagnosed or retried.

Reproduction commands (from repository root):

```powershell
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_driver as d,test_f0_blockers as b,test_f0b_transport as t; flat=lambda s:[x for y in s for x in (flat(y) if isinstance(y,unittest.TestSuite) else [y])]; cases=flat(unittest.TestSuite(unittest.defaultTestLoader.loadTestsFromModule(m) for m in (d,b,t))); cases=[x for x in cases if x._testMethodName!='test_cli_wrong_platform_refuses']; r=unittest.TextTestRunner(verbosity=1).run(unittest.TestSuite(cases)); sys.exit(not r.wasSuccessful())"
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0_blockers as t; t.mutations()"
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0b_transport as t; t.mutations()"
```

Both mutation runners require a passing baseline before mutation, exact-once
anchors, successful in-memory compilation, no errored test run, and named
assertion failures. They restore the module binding and compare the raw
driver source SHA-256 before and after:
`dce176c634456b144bac1525338896dce7404171572b7e5d22b7f152c0dbd53c`.
This is the Windows working-file byte hash (Git normalizes line endings).
No mutated source is written. Final checked source is the source commit above.

## Sizes and coverage

The test pins the measured maxima: **757 internal bytes**, **2445 total outer
bytes**, both including the newline. Enumeration covers all three carried
modes, their longest allowlisted site, 32 triples with longest passing
predecessors and a longest refusing triple, maximal first/final refusals,
20-digit run/attempt, longest remaining statuses and progress token.
Internal >1024 and total outer >4096 refuse without truncation.

New tests exercise root/opt/stage NOFOLLOW and trust checks, private stage,
writer allowlist/exact type/phase/trust, first creator, short writes, descriptor
closure, metadata/type/owner/mode/link/size/read-count checks, duplicate fields,
newline/ASCII/canonical form, schema/mode/site/phase/decide disagreement,
all eight captured combinations, unknown/no-channel-open, metadata progress,
marker failure before control.mkdir, and real entry/run stop/cleanup ordering.
Stop rc 0/5, stop timeout, quarantine, body and cleanup errors are covered.
Unknown/malformed/captured apparent successes and reader-exception-to-unknown
produce E_RECORD without fabricated first/final or cleanup tokens.
The stop call AST and original cleanup fences are checked.

Internal-entry tests cover installed and each mode's allowlisted sites using
escaping mocked Refusals. This is conditional capture mechanism evidence,
not real seven-site coverage. **Installed is never captured.** Stable
service_stage/harness_stage trust failures generally prevent channel creation.
Service-finally masking, first-creator loss, exec, non-Refusal errors,
BaseException and termination remain gaps. Absent is never proof of success.
Snapshot evidence assumes existing unit containment and establishes neither
escaped-root coverage, atomic consistency nor immutability.

## Legacy migration

The v2 envelope renames mutation, adds internal and bumps the version. Legacy
scenario snapshots are mocked to absent; new tests exercise the real snapshot.
F0Mechanism resets the new global to prevent cross-test leakage.
The old xf_success text is replaced with the new success-rule anchor.
Its isolated invalid case now changes only cleanup to unconfirmed while
retaining null first/final and a valid service/absent internal summary. Removing
the success rule alone then admits it. None/not_reached cross-field cases set
internal=null so V1 cannot mask the intended violation. Duplicate-key and exact
Refusal anchors are narrowed to their original functions after shared helpers
introduced similar text. All original behavioral obligations remain; counts
alone are not acceptance.

Legacy assertion ledger (`runTest` denotes unittest's named subtest in
test_isolated_replay_cross_fields):

```text
KILLED run_drops_first_capture assertion=test_cleanup_only_and_refusal,test_run_orderings,test_success_and_both_refusals
KILLED run_drops_final_phase assertion=test_cleanup_only_and_refusal,test_run_orderings,test_success_and_both_refusals
KILLED cleanup_confirmed_early assertion=test_cleanup_only_and_refusal,test_run_orderings,test_success_and_both_refusals
KILLED mutation_staged_dropped assertion=test_cleanup_only_and_refusal,test_run_orderings,test_success_and_both_refusals
KILLED mutation_service_dropped assertion=test_cleanup_only_and_refusal,test_run_orderings,test_success_and_both_refusals
KILLED protected_ignores_phase assertion=test_protected_phase_site_bound
KILLED replay_dup_keys_allowed assertion=test_counterfactuals
KILLED replay_input_bound_dropped assertion=test_counterfactuals
KILLED replay_ascii_dropped assertion=test_counterfactuals
KILLED sha_equality_dropped assertion=test_counterfactuals
KILLED protected_bound_dropped assertion=test_protected_phase_site_bound
KILLED conjunct_owner_first assertion=test_protected_phase_site_bound
KILLED entry_first_is_final assertion=test_cleanup_only_and_refusal,test_run_orderings,test_success_and_both_refusals
KILLED refusal_exact_type_relaxed assertion=test_closed_codes
KILLED site_token_unchecked assertion=test_protected_phase_site_bound
KILLED xf_success assertion=runTest
KILLED xf_confirmed assertion=runTest
KILLED xf_none assertion=runTest
KILLED xf_not_reached assertion=runTest
KILLED xf_first_phase assertion=runTest
KILLED xf_final_phase assertion=runTest
KILLED validator_codes_any_str assertion=runTest
KILLED fallback_always_gate_ok assertion=test_valid_gate_invalid_record_fallback
source unchanged sha256=dce176c634456b144bac1525338896dce7404171572b7e5d22b7f152c0dbd53c
```

## F0b candidate ledger

The combined nonblock/type candidate is split into independent nonblock and
type-check mutations. Ancestor NOFOLLOW and separate malformed/captured success
candidates are included. The delayed-reader mutation moves snapshot after
fixture_cleanup; the ordering assertions catch it. Writer walk omission also
breaks valid descriptor traversal, so its listed kills do not establish a
kernel symlink exploit. Flag assertions establish requested open flags under
the fake OS, not actual kernel behavior.

Four equivalents earn **zero** kill credit. EXCL is modeled as refusing an
existing entry; no fake symlink-following write is credited. Shared validation
and canonical reconstruction independently reject the other equivalent edits.
The installed-flag mutation removes the post-installed trust grant; the trust
mutation bypasses the writer's trust check, so these are distinct edits here.

```text
KILLED writer_ignores_trust assertion=test_f0b_transport.Transport.test_writer_guards
KILLED writer_ignores_installed_flag assertion=test_f0b_transport.EntryRun.test_internal_entry_installed_gate_and_escaping_error
KILLED writer_isinstance_refusal assertion=test_f0b_transport.Transport.test_writer_guards
EQUIVALENT writer_site_unchecked shared validate_internal still enforces the same site and phase before create
KILLED writer_drops_excl assertion=test_f0b_transport.Transport.test_writer_and_first_creator
EQUIVALENT writer_drops_nofollow O_EXCL independently rejects every existing entry, including a symlink
KILLED writer_walk_skips_ancestor assertion=test_f0b_transport.EntryRun.test_real_run_stop_order,test_f0b_transport.Transport.test_completion_condition_and_progress,test_f0b_transport.Transport.test_precedence,test_f0b_transport.Transport.test_reader_bytes_guards,test_f0b_transport.Transport.test_reader_metadata_guards,test_f0b_transport.Transport.test_writer_and_first_creator,test_f0b_transport.Transport.test_writer_short_and_invalid_created_file
KILLED ancestor_nofollow_dropped assertion=test_f0b_transport.EntryRun.test_real_run_stop_order,test_f0b_transport.Transport.test_completion_condition_and_progress,test_f0b_transport.Transport.test_precedence,test_f0b_transport.Transport.test_reader_bytes_guards,test_f0b_transport.Transport.test_reader_metadata_guards,test_f0b_transport.Transport.test_writer_and_first_creator,test_f0b_transport.Transport.test_writer_short_and_invalid_created_file
KILLED writer_ignores_stage_mode assertion=test_f0b_transport.Transport.test_stage_walk_guards
KILLED writer_truncates assertion=test_f0b_transport.Transport.test_writer_guards
KILLED writer_raises assertion=test_f0b_transport.EntryRun.test_internal_entry_installed_gate_and_escaping_error,test_f0b_transport.Transport.test_writer_and_first_creator,test_f0b_transport.Transport.test_writer_guards,test_f0b_transport.Transport.test_writer_short_and_invalid_created_file
KILLED reader_follows_link assertion=test_f0b_transport.EntryRun.test_real_run_stop_order,test_f0b_transport.Transport.test_precedence,test_f0b_transport.Transport.test_reader_metadata_guards
KILLED reader_drops_nonblock assertion=test_f0b_transport.EntryRun.test_real_run_stop_order,test_f0b_transport.Transport.test_precedence,test_f0b_transport.Transport.test_reader_metadata_guards
KILLED reader_drops_typecheck assertion=test_f0b_transport.Transport.test_reader_metadata_guards
KILLED reader_ignores_owner assertion=test_f0b_transport.Transport.test_reader_metadata_guards
KILLED reader_ignores_mode assertion=test_f0b_transport.Transport.test_reader_metadata_guards
KILLED reader_ignores_nlink assertion=test_f0b_transport.Transport.test_reader_metadata_guards
KILLED reader_unbounded assertion=test_f0b_transport.Transport.test_precedence,test_f0b_transport.Transport.test_reader_metadata_guards
EQUIVALENT reader_ascii_dropped canonical ensure_ascii byte equality still rejects non-ASCII UTF-8
KILLED reader_noncanonical_accepted assertion=test_f0b_transport.Transport.test_reader_bytes_guards
KILLED internal_site_unchecked assertion=test_f0b_transport.Transport.test_reader_bytes_guards,test_f0b_transport.Transport.test_writer_guards
KILLED reader_accepts_final_phase assertion=test_f0b_transport.Transport.test_reader_bytes_guards,test_f0b_transport.Transport.test_writer_guards
EQUIVALENT reader_mode_mismatch canonical rebuild pins the expected mode, independently rejecting mismatch
KILLED reader_reads_unquiesced assertion=test_f0b_transport.EntryRun.test_real_run_existing_failures_preserved,test_f0b_transport.Transport.test_completion_condition_and_progress
KILLED reader_requires_rc_zero assertion=test_f0b_transport.EntryRun.test_real_run_stop_order,test_f0b_transport.Transport.test_precedence,test_f0b_transport.Transport.test_reader_bytes_guards,test_f0b_transport.Transport.test_reader_metadata_guards,test_f0b_transport.Transport.test_writer_short_and_invalid_created_file
KILLED reader_replaces_cg_fence assertion=test_f0b_transport.EntryRun.test_real_run_existing_failures_preserved
KILLED reader_after_cleanup assertion=test_f0b_transport.EntryRun.test_real_run_existing_failures_preserved,test_f0b_transport.EntryRun.test_real_run_stop_order
KILLED reader_raises assertion=test_f0b_transport.EntryRun.test_real_run_existing_failures_preserved,test_f0b_transport.EntryRun.test_real_success_anomalies_only_fallback
KILLED fd_close_dropped assertion=test_f0b_transport.Transport.test_writer_and_first_creator,test_f0b_transport.Transport.test_writer_short_and_invalid_created_file
KILLED carried_precedence_changed assertion=test_f0b_transport.Transport.test_precedence
KILLED success_accepts_unknown assertion=test_f0b_transport.EntryRun.test_real_success_anomalies_only_fallback,test_f0b_transport.Records.test_cross_fields
KILLED success_accepts_malformed assertion=test_f0b_transport.EntryRun.test_real_success_anomalies_only_fallback,test_f0b_transport.Records.test_cross_fields
KILLED success_accepts_captured assertion=test_f0b_transport.EntryRun.test_real_success_anomalies_only_fallback,test_f0b_transport.Records.test_cross_fields
KILLED progress_counts_link assertion=test_f0b_transport.Transport.test_completion_condition_and_progress
KILLED progress_from_status assertion=test_f0b_transport.Transport.test_precedence
KILLED v1_accepted assertion=test_f0b_transport.Records.test_cross_fields
KILLED outer_overflow_truncates assertion=test_f0b_transport.Records.test_measured_maxima
source unchanged sha256=dce176c634456b144bac1525338896dce7404171572b7e5d22b7f152c0dbd53c
```

## Failures during development and remaining limits

An initial integration fixture used PurePosixPath without an exists method,
causing E_RUNTIME; a mocked cgroup path with an explicit exists result corrected
the fixture. Initial xf_success input also violated confirmed equality; the
isolated cleanup=unconfirmed case corrected attribution. Mutation harness
bring-up found a missing compiled-module __file__, a duplicate close anchor and
tests that errored on deliberately suppressed/raised writers; these were
corrected to explicit assertions before any final kill credit. Marker/fence
tests initially had an exhausted Path mock and a whole-source substring count;
both were corrected to role-based paths and the run_body AST segment.
No failed functional fix sequence reached three attempts. Earlier non-green
runs earn no acceptance/mutation credit; the final runs above are green.

Fresh **Opus high source/security review is still required before publication**.
No independent source review is claimed, and nothing was published, pushed,
merged or submitted to CI. PROGRESS, reviews, proposal, ADRs, policy, workflows,
Cargo and Rust sources are unchanged by this task.

F0b earns local mechanism credit only. No historical diagnosis, reproduction,
retry, new-SHA authority or real-record replay credit is claimed. F2 remains
undispatched; **M2 is incomplete; real-Runner PA6 and real-record replay are
OPEN. All native, installed and dedicated paths are unverified on real
hardware.**


## Coordinator test/evidence correction after review 0c3aa33

OpenAI / gpt-6.1-sol / medium, 2026-11-14. Driver unchanged.
FakeOS now preserves existing bytes on O_CREAT without O_TRUNC, matching the
kernel contract. A separate pre-existing empty channel test pins refusal of
a dead first creator; writer_drops_excl fails that assertion, not invented
truncation. Earlier kill attribution to fake truncation is withdrawn.

Carried-refusal replay inputs individually cover wrong mode, installed and
final phase while otherwise satisfying the outer record. A staged-bound
record with internal summary isolates V1 without a success-path TypeError.
The old internal=None test is also a failed-body record, avoiding that crash.
Marker tests now force create_channel to raise and assert it is swallowed.
Three new compiled mutants remove those guards and fail named assertions.

Final safe suite: 66 selected tests passed in 0.391s, CLI excluded. Legacy
mutation runner: 23 assertion kills. F0b: 40 candidates, 36 assertion kills,
4 equivalents without credit, zero errors. The first mutation run stopped
at an old success/internal=None counterfactual's TypeError; no kill was
credited until that counterfactual was isolated and the full runner passed.

Maximum measured sizes corrected: internal 757 bytes; outer 2447 bytes,
including the first==final, both-initial-phase worst case. Earlier 2445 is
superseded. These remain below 1024 and 4096. Earlier working-copy hashes
must not be read as committed LF hashes. Current driver raw bytes and
committed LF SHA-256 both equal:
`b147b117e7287e663ffdc202688877f79c43c150cb271fe927dcb9d445d3551a`.

Test file SHA-256: `8088fe6096d5d4eb3113fa817f9376226a07d69d096f09b3a54be8bf39d67b85`.

The reviewed migration explicitly permits the narrow test_driver.py v2
counterfactual changes. This separate F0b evidence file is the coordinator's
chosen evidence path; it supersedes the proposal's older append-only naming.
F0b publication will be cut from merged main without Windows diagnostic
proposal/history. Other review advisories remain disclosed. No CLI, native,
privileged operation, dispatch or rerun was performed. M2 incomplete;
real-record replay and real-Runner PA6 OPEN; hardware unverified.
