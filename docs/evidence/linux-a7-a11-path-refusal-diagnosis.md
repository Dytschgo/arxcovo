# A7/A11 D7i pre-mutation path refusal diagnosis

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-15.
Parent `8d58ed6`; accepted hosted source main `0011d23`.
No subagents. Fresh independent Muse review remains required before publication;
the parallel Muse failure assessment is separate evidence, not a review of this delta.

## Historical observation and limits

The retained [closed record](linux-a7-a11-run-43715436515.json) and
[disposition](linux-a7-a11-hosted-disposition.md) identify accepted attempt1,
run43715436515/job132815838689: failure after18s in the root guard/stage step,
after safe tests, fetch and compile-only A4 success. First/final `E_PATH`, mutation
`none`, cleanup `not_reached`, internal null. There is no role attribution.
Both pins absent404 and host exclusion are inherited closed evidence, not queries
performed here. D7i is consumed, with no retry or replacement authority.

Read AGENTS/KICKOFF, dytschgo-debug and its active profile, ADR0019 D7i/ADR0020
D9b, preparation/source/delivery/qualification/integration evidence and relevant
Muse source reviews. The earlier atime correction is already present; it targets
alias/content stability and does not explain this closed `E_PATH` observation.

## Symptom loop established before hypotheses

Audited the new test and actual `run -> run_body -> a7_admission -> a7_alias_c2 ->
sudo_nnp_open -> a4_directory` chain before executing it. Platform/root/package
comparison/cgroup ports are mocked; the real alias and descriptor traversal logic
remain. All opens, fstat/stat, closes, readlinkat and optional image digest are fake.
The package reader is never invoked. Effects are poisoned. No actual adapter,
Runner, LinuxFs, helper, account, policy, sys/proc or host query is exercised.
Repository source/evidence reads and Python development execution are local only.

First invocation (before any source change or ranked hypotheses):

```powershell
python -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_driver; r=unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromTestCase(test_driver.AdmissionPathRefusal)); sys.exit(not r.wasSuccessful())"
```

Result then: 1 test, 0.006s, OK. The test asserts exact first/final `E_PATH`,
mutation `none`, cleanup `not_reached`, internal null and descriptor closure when
hypothetical `/usr/local` metadata has non-root ownership. This is a deterministic
reproduction of an **expected protective refusal under hypothetical metadata**,
not proof that the historical host had that metadata. The qualifying control has
root-owned0755 directories, accepted merged-/usr alias and C2 absent or exact
mocked image pin. The loop can go red if the actual fence is bypassed or feedback
changes. Final shortest invocation selects
`AdmissionPathRefusal.test_exact_path_refusal_loop` alone with the same runner.

## Ranked hypotheses and source/mock results

Ranked after the first loop and before testing these predictions:

| Rank | Hypothesis and prediction | Result |
| --- | --- | --- |
| 1 | C2 protected ancestry refuses: changing a directory owner/type/write bit should cause `E_PATH` before cgroup admission. | Confirmed as expected logic in mocks for `/`, `/usr`, `/usr/local`, `/usr/local/bin`; ownership, both write bits, link and regular-file counterfactuals refuse. No historical ownership inferred. |
| 2 | Explicit alias-root or target ancestry refuses: injecting a failure on that open should preserve `E_PATH` and close held descriptors. | Confirmed in mocks for the second root open and `/usr/bin` target open, independently of the first C2 parent traversal. No historical failing role inferred. |
| 3 | Valid merged-/usr metadata falsely generates `E_PATH`: fully qualifying metadata should still refuse if traversal/flag/type logic is defective. | Not demonstrated. Qualifying absent-C2 and present-C2 controls admit through the real chain. Existing alias tests cover atime changes, invalid targets and image mismatches; their codes are distinct from this fence. This is bounded mock evidence, not whole-host qualification. |
| 4 | Package or cgroup checks emit the same code: their checked-source paths should contain a reachable `E_PATH` for the fixed inputs. | Not supported by inspected source. Package collector exceptions collapse to its fixed result, then driver `E_ARTIFACT`; cgroup metadata uses protection/cgroup codes. `protected` has a lexical `E_PATH` guard, but its pre-effects call sites supply fixed absolute paths without `..`. No package/cgroup adapter was run. |

Pre-effects order is gate/attempt1, package admission, C2/alias admission,
cgroup preflight, root/cgroup recheck, fixed stage-parent protection and exclusive
reservation. Staging comes afterward. A4 delivery is unselected by fixed A7 TEST.
OS open failures remain `E_OS`; C2 and alias-specific failures do not become role
records. The most direct source-local `E_PATH` candidate is `a4_directory` within
alias/C2 traversal, but the historical role and failed conjunct remain unknown.
No actual logic bug was proved. Relaxing root ownership or no022 would change the
security contract and is not a fix justified by these results.

## Minimal attribution delta and red/green

Only four existing pre-mutation alias/C2 `sudo_nnp_open` call sites can translate
an exact `RuntimeError('E_PATH')` into `A7PathRefusal`. They retain code `E_PATH`
and emit one fixed role: `a7_c2_parent`, `a7_alias_root`, `a7_alias_target`, or
`a7_c2_image`. A role labels a call, not a concrete failed ancestor/conjunct.
No path, UID, mode, metadata, hash, payload, exception string or traceback is added.
Later admissions, OS errors and other codes retain their previous forms.

F0 version2 admits exactly `{code:E_PATH, role:<fixed enum>}` only with mutation
`none`; existing consistency checks require matching first/final, cleanup
`not_reached` and internal null. Historical code-only records still replay.
F0b rejects these records internally; its precedence/schema/transport are unchanged.
No code allowlist expansion, verbose error, new channel or audit framework exists.

Before the source delta, `test_fixed_role_attribution` failed by assertion:
`{'code': 'E_PATH'} != {'code': 'E_PATH', 'role': 'a7_c2_parent'}`.
After the delta, all four real-chain role cases and privacy/schema regressions pass.
Mutation testing initially exposed a test-harness issue: shared `os.stat` mocking
intercepted unittest traceback formatting on an intentionally failing mutant.
The fixture now isolates the driver's OS port; that failed harness run earns no
mutation credit. Final mutation runs require assertion failures and zero errors.

## Checks actually run

All Python commands use `-B`; no build/temp output or cache is created. No Cargo,
compiler or large build runs. The following focused commands ran successfully:

```powershell
python -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import mutation_check; mutation_check.admission_path_mutants()"
python -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation'); import test_driver,test_f0b_transport,mutation_check; s=unittest.TestSuite(); [s.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(c)) for c in (test_driver.AdmissionPathRefusal,test_driver.RuntimeAdmission,test_driver.F0Mechanism)]; s.addTests(test_f0b_transport.suite()); r=unittest.TextTestRunner().run(s); assert r.wasSuccessful(); mutation_check.execution_admission_mutants()"
git diff --check
```

- New suite: 6 tests PASS (0.033s in final mutant baseline); 11/11 in-memory
  mutants killed by assertions, with source bytes unchanged. Covers owner, each
  write bit, directory type, nofollow, final-directory validation, role mapping,
  pre-mutation restriction, serialization and closed-schema restrictions.
- Combined audited compatibility suite: 41 tests PASS (0.126s), including
  existing admission, F0 mechanism, F0b transport/records/entry lifecycle and
  precedence. Measured existing maximum internal757/outer2447 bytes.
- Existing execution admission mutants: 6/6 killed by assertions.
- Original exact symptom loop re-run after the delta: 1 test PASS. Its expected
  refusal remains; the added role is the corrected feedback behavior.
- `git diff --check`: PASS, including the final evidence file.

Only `driver.py`, `test_driver.py`, `mutation_check.py` and this evidence change.
No ADR, PROGRESS, workflow, package, production, Rust, helper or Cargo change.
TEST remains fixed A7. No grant or code-based permission change, host/sys/proc/
account/policy read, sudo query, network, variable, dispatch, install, push, PR or
watch operation ran. No old Lab Manager or owner-admin work occurred.

Historical cause remains unknown. Coordinator disposition and fresh independent
Muse review of this exact source are still required before publication. This
instrumentation grants no execution or retry. A7/A11, M2, full F/G and real-Runner
PA6 remain **unverified on real hardware**.
