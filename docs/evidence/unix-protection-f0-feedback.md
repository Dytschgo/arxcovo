# F0 local protection feedback mechanism

2026-11-14. OpenAI / gpt-6.1-sol / medium, implementation sub-agent.
Local dependent preparation only; PR69 remains pending merge per task context.
No external status check was made.

## Pins and isolation

Cwd matched Git top level in the assigned isolated worktree. Initial status was
clean at prepared base `f55db98c275af28d93b3407c94ae861941692e45`.
Source commit: `2c93991870ca108115dd04e1a7592d728fcfe2ff`.

| Source object | Exact blob |
| --- | --- |
| driver.py | `1ecc7341b194a4e565c1cc3431dcf1c152eae15f` |
| test_driver.py | `c20088afcd82189c9ce20cb6dbbdcc0b7c79affc` |
| README.md | `70373cbe6b68ebcaf0f360f094d35ae32b1860d1` |
| unchanged workflow | `e5f1a96dbfa522bda602db45bf24db000ebddd5c` |
| unchanged disposable.rs | `586b30811b3af485f6b96f7a4a989b2ba64e3e70` |

Read AGENTS, KICKOFF, ADR0019 D7/C2, proposal F0/F1/F2, debug skill and
active profile, and historical phase-1 via
`git show 20f39c0:docs/reviews/unix-protection-phase1-evidence.md`.
No GLOSSARY matched the tracked filename search. Task restrictions superseded
coordinator delegation/progress/babysitter instructions. No agent was spawned.

## Implementation and limits

All 13 protected call sites carry fixed distinct tokens. Synthetic metadata
passes through the same pure `decide` used by the real predicate. Link,
owner-zero and write-mask checks retain historical order; regular files are
still allowed. Records retain ordered predecessors and reject an earlier
refusal. Bounds are 32 triples per refusal and 4096 ASCII bytes for the entire
single JSON line including newline and provenance. Strict schema/version,
exact fields, types, duplicate fields and provenance syntax are validated.
Malformed or overflow input is rejected; no truncation earns replay evidence.

The outer run tracks initial versus finalization exceptions separately and
emits one validated record. Arbitrary exception payloads are never formatted.
Unknown errors use a fixed generic code. Existing failure exit remains 1;
cleanup exceptions continue to escape and refuse success. No cleanup/reuse
fence was relaxed. Mutation denotes a conservative outer attempted boundary;
cleanup is confirmed only after the existing cleanup completes.

**Review limitation:** existing suppressed internal subprocess exceptions are
not transported into the outer record. Internal protection failures therefore
remain generic outer failures, without invented triples. Account/cgroup
progress cannot be inferred from outer mutation tokens. This local mechanism
is not complete cross-process diagnostic capture. Review must disposition that
limitation before any future diagnostic proposal; no scope expansion or
privileged observation was attempted.

Compatibility observations only: disposable.rs continues to call legacy
Runner::run, not run_phased. Current mod.rs routes legacy run through run_core
with legacy error mapping; ADR0019 C1 expressly retains that API. The fixture
still supplies a protected delegated parent and generated identity and invokes
the driver fixture-check mode. These source observations do not establish
post-0019/0020 runtime compatibility, package cleanup, setup or janitor behavior.
No Rust fix or native experiment was attempted.

## Actual validation and failure history

At the source commit the following safe command passed, exit 0:

```text
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_driver; suite=unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.F0Mechanism); suite.addTests(t for t in unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.Guards) if t._testMethodName != 'test_cli_wrong_platform_refuses'); suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.CgroupScan)); r=unittest.TextTestRunner(verbosity=1).run(suite); sys.exit(not r.wasSuccessful())"
```

Result: 22 tests passed in 0.179s before the final fixed-platform-code adjustment;
the final source run also passed all 22 (see retained tool result for timing).
Earlier focused run: 8 F0 tests passed in 0.005s. All are synthetic mechanism
or gate checks. No filesystem stat observation supplies test triples. The CLI
subprocess test was deliberately excluded, as were provisioning/temp-filesystem
runtime fixtures. No system driver, native fixture or native command was run.
`git diff --check` passed before source commit. Workflow diff against the base
was empty; source commit touched only the three permitted tool files.

No failed implementation test/fix attempt occurred. One read-only rg glob was
rejected by Windows filename syntax; an explicit-file search subsequently
succeeded. This is not a symptom reproduction or failed fix. Historical
E_PROTECTION, ExecutionFailed and privacy failures remain unknown; counters
are not reset by this source SHA.

## Gates

No real captured record exists; real-record replay gate OPEN. Debug Phase 1
remains incomplete, with no hypothesis, reproduction, fix, retry or dispatch
claim. A new SHA does not authorize an experiment; F2 authorizes no dispatch.
M2 remains incomplete; real-Runner PA6 OPEN. Native, installed and dedicated
paths are **unverified on real hardware**.

Fresh Opus high security review is required, followed by parent merge/repin
and normal gates before publication. This sub-agent performed no review
request, subagent, GitHub operation, push, PR, CI, dispatch, settings, secret,
account/service/cgroup/VM/owner/private-state operation. Unknown or unconfirmed
cleanup environments must not be reused.

## F0 review blocker corrections — 2026-11-14

OpenAI / gpt-6.1-sol / medium, implementation sub-agent, no subagents.
Initial cwd equalled isolated Git top level and status was clean at
`9c64c6660a82956afff0cd1bff01055d227b7492`.
Source commit: `93b1194cb9efc3101dec8c0e5779f87edc1b7d95`.
Read AGENTS, KICKOFF, accepted proposal, full independent F0 review,
debug skill and active profile, and ADR0019 D7/C2. No diagnosis, hypothesis,
historical reproduction or real-record replay credit is claimed.

ST-B1: new test_f0_blockers drives real entry/main/run with runtime effects
mocked in memory: pre-try refusal; build/service failures with successful or
failed cleanup; cleanup-only failure; success; and distinct initial/final
protection refusals. It checks provenance, exception records, attempted
mutation and cleanup tokens, and confirmation timing before deletion.
Protected phase/site/bound tests use synthetic objects, without filesystem
stats. Valid JSON counterfactuals isolate duplicate keys, whitespace-padded
input bound, SHA equality and ASCII transport. ASCII rejection is pinned to
E_RECORD_BOUND so a later schema rejection cannot earn credit. An AST guard
pins 13 distinct sites and explicit stderr sinks (check_output owns stdout).

SP-B1: closed CODES inventories existing fixed require/raise codes and gate
keys; exact RuntimeError with one exact str argument is allowlisted. Arbitrary
payload formatting is never used. Command timeout/failure have fixed codes;
all OSError instances, including unknown errno, map to fixed E_OS (no dynamic
errno or payload). Body first and escaping final remain distinct, including
null first on cleanup-only failure. Pre-try errors are captured by a resetting
run wrapper; run_body retains the unchanged finally cleanup fences. Strict
cross-field validation rejects inconsistent success, no-mutation, not-reached,
confirmed-cleanup and refusal-phase records. Valid gate failures emit their
fixed gate code, not E_RECORD. Feedback association uses the real platform.

SP-B2: all three outer check_output stderr streams now use DEVNULL. The only
fallback output is a fixed-code stderr line when association/encoding cannot
produce a validated record. No raw logs are emitted.

Actual final-source validation (both exit 0):

```
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0_blockers as t,test_driver as d; s=unittest.defaultTestLoader.loadTestsFromTestCase(t.F0Blockers); s.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(d.F0Mechanism)); s.addTests(x for x in unittest.defaultTestLoader.loadTestsFromTestCase(d.Guards) if x._testMethodName != 'test_cli_wrong_platform_refuses'); s.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(d.CgroupScan)); r=unittest.TextTestRunner().run(s); sys.exit(not r.wasSuccessful())"
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0_blockers as t; t.mutations()"
```

30 tests passed in 0.212s after source commit. All 15 mutants compiled and
were killed by named assertions; baseline must pass before mutation execution:

- run_drops_first_capture, run_drops_final_phase, cleanup_confirmed_early,
  mutation_staged_dropped, entry_first_is_final: test_run_orderings and
  test_cleanup_only_and_refusal (also test_success_and_both_refusals).
- mutation_service_dropped: test_run_orderings, test_success_and_both_refusals.
- protected_ignores_phase, protected_bound_dropped, conjunct_owner_first,
  site_token_unchecked: test_protected_phase_site_bound.
- replay_dup_keys_allowed, replay_input_bound_dropped, replay_ascii_dropped,
  sha_equality_dropped: test_counterfactuals.
- refusal_exact_type_relaxed: test_closed_codes.

The runner restores the original module binding in finally, writes no mutant
source, and checks raw-byte SHA-256 unchanged:
`6f029b376b67d0630f7ab1e177f92bc814ccdbf2e21af24112ed6900e0649031`.
Compile errors and test errors receive no kill credit. git diff --check passed.

Failure history: initial isolated-script invocation could not import driver;
used explicit sys.path in the documented -c command. First mutation pass found
missing final-phase assertion; next found site refusal accepted via a second
fence; added exact phase and E_SITE guards. Next exposed text-normalized versus
raw-byte hash comparison; corrected to raw bytes. The safe legacy AST test
needed run_body after wrapper extraction. Added both-refusal case initially
stopped at mocked E_BUILD; corrected its mock build result and added a mandatory
green baseline before mutants. Final selected tests and all mutants pass.
These were separate test-harness defects, not three failed fixes of one issue;
no historical failure counters are reset.

Only permitted tooling source/tests/README and this evidence appendix changed.
Existing reviews/proposal/PROGRESS/workflow/Rust/native fixture/policy/Cargo
were not edited. No CLI driver/native/runtime-filesystem/provisioning fixture,
existing mutation_check, Cargo, stat observation, host probe, account/service,
cgroup, VM, private state, network, GitHub, push, PR, CI or dispatch was used.
The seven internal sites remain uncaptured: installed, service_stage,
service_cg, service_runs, capture_base, harness_stage, harness_binary.
E_SERVICE_OR_TEST preserves only the outer fixed classification; separate
F0b follow-up is needed before any F2 complete-capture claim. No new SHA
supplies experiment authority. M2 incomplete; real-Runner PA6 OPEN;
real-record replay OPEN; hardware paths **unverified on real hardware**.
Ready for fresh Opus high independent source recheck, not publication/merge.
