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
