# M2 F0: independent recheck of the F0 review blocker correction

2026-11-14. Anthropic / claude-opus-5-5 / high, review sub-agent (single
agent, no subagents). Builder: OpenAI / gpt-6.1-sol / medium. This is the
fresh different-provider recheck of Sol's correction for the three blockers
in `docs/reviews/m2-f0-feedback-independent-security-review.md` (ST-B1,
SP-B1, SP-B2). The scope is narrow. Only new blockers are assessed, and no
redesign is proposed.

**Verdict: Standards CHANGES REQUIRED (1 new, narrow, tests-only blocker,
ST-B2). Spec PASS.** All three prior blockers are closed in source. ST-B1 is
closed for its 15 named mutants. SP-B1 and SP-B2 are closed in behaviour. The
new blocker affects only the tests: the correction added a cross-field and
closed-code replay validator, and the committed tests do not isolate it.
The code itself behaves correctly; the probe in check 5 shows that. The fix
adds tests and changes no source. The cross-process capture gap keeps the
prior disposition: it is not an F0 merge blocker, it is an F2 blocker, and
F0b remains separate. This recheck grants no run, dispatch, retry, merge or
experiment authority.

## Isolation and exact pins

The cwd equalled the Git top level, an isolated T3 worktree on branch
`t3/review-f0-blocker-correction`. Status was clean. HEAD was
`4788871a8103078e41a318b3e19c10ef8f12d382`.

| Object | Pin |
| --- | --- |
| Correction source (original) | `93b1194cb9efc3101dec8c0e5779f87edc1b7d95`, retained as `f13df0da36d3e9ac49f587c7978a769b2410c948` |
| Correction evidence (original) | `979e3585d10272d3e178973be4d1b19b493f7781`, retained as HEAD `4788871a8103078e41a318b3e19c10ef8f12d382` |
| Prior review (original) | `4220dd780d52a4008d077a3aab0bbc581c7bb504`, retained as `9c64c6660a82956afff0cd1bff01055d227b7492` (blob `99df914a…`) |
| Merged S2b in prepared base | `9f97a2d` (ancestor of HEAD via merge `b3cd53c`) |
| `driver.py` blob | `cf5a0ca64707ccf95036490a06c7dc3b4cb86573`; LF bytes SHA-256 `34a7111cb5d2c02993c6ba4836f0cff893b99e4b5b0564818372eaaf7de66021` |
| `test_driver.py` blob | `0baeeb58f2e789bf9babd32322fd7ee7000e3093` |
| `test_f0_blockers.py` blob | `3e2a4172fe766118b6b513d980f38d877d257f55` |
| Tool README blob | `aaa397530133e05a6aea5afaafa5e4e9f24b0094` |
| Evidence `docs/evidence/unix-protection-f0-feedback.md` | `a2d166fb6c15030e404eacdc1dbed4f0412a24cd` |
| Workflow (unchanged vs `9f97a2d`) | `e5f1a96dbfa522bda602db45bf24db000ebddd5c` |
| `disposable.rs` (unchanged vs `9f97a2d`) | `586b30811b3af485f6b96f7a4a989b2ba64e3e70` |
| `mutation_check.py` (unchanged vs `9f97a2d`) | `fe5b976bd1b946c29012d33879536d172406dc9f` |
| Proposal / AGENTS / KICKOFF | `8fd8bdc0…` / `c2ae2594…` / `9dc49e72…` (same as prior review) |

The retained commits differ from the originals only in parent-chain
`docs/PROGRESS.md` content. `git patch-id --stable` is identical for each
pair: `f55669cc…` for the source, `d5812b80…` for the evidence and `e105c6a8…`
for the prior review. The merge `b3cd53c` adds nothing over `9f97a2d` in the
tools tree. `git diff --name-only b3cd53c HEAD` lists only PROGRESS, the
evidence file, the prior review, the tool README, `driver.py`,
`test_driver.py` and `test_f0_blockers.py`. `git diff --check 9c64c66 HEAD`
is clean. `decide`, `protected` and `Refusal` have no hunks, so the real
protected predicate is unchanged. The finally cleanup fences in `run_body`
are textually unchanged. The only edits there are the extraction into the
`run()` wrapper and the three `stderr=` sinks.

Read in full: AGENTS, the dytschgo-review and dytschgo-debug skills with
their active profiles, the full prior review, the accepted proposal's Stage F
(F0/F1/F2), the full current `driver.py`, `test_f0_blockers.py`, the
`F0Mechanism` class in `test_driver.py`, the correction diff, the README diff
and the evidence appendix. I also read the historical entry block of
`4b4775f` to confirm the floor it emitted (`E_OS_<errno>`, regex-allowlisted
codes, `E_COMMAND_TIMEOUT`).

## Checks actually run

All checks were synthetic and in-memory on Python 3.12.10 (Windows). There
was no CLI driver invocation, native command, provisioning, account,
service, cgroup, `/proc`, `/sys` or stat observation, `RuntimeGuards` or
`CopyGuards` filesystem test, Cargo run, VM, private state, GitHub, network,
CI or dispatch. Review scripts lived in the session scratchpad and are not
committed. After every run, `driver.py` SHA-256 was unchanged and
`git status` was clean.

1. **Documented safe subset** (the exact evidence command: F0Blockers,
   F0Mechanism, Guards minus `test_cli_wrong_platform_refuses`, CgroupScan):
   `Ran 30 tests in 0.210s OK`, exit 0. This matches the evidence.
2. **Documented 15 mutants** (`t.mutations()`). The mandatory baseline ran
   green first. Each of the 15 was `KILLED` by named assertion failures, with
   zero errors, and the runner printed the same test names as the evidence
   (for example, `replay_ascii_dropped assertion=test_counterfactuals`). The
   runner refuses credit when `result.errors` is non-empty or nothing fails.
   A compile error aborts the run. Each anchor must occur exactly once.
   It restores the module binding in `finally`.
3. **Review-only extra mutants** (23, in memory, against F0Blockers and
   F0Mechanism, after a green baseline of 16 tests):

   ```text
   KILLED   wrapper_always_overwrites / wrapper_never_records / wrapper_reset_dropped
   KILLED   run_body_final_phase_after_stop / entry_first_or_final
   KILLED   timeout_to_runtime / calledproc_to_runtime / oserror_to_runtime
   KILLED   codes_membership_dropped (test_hostile_payload_never_formatted)
   KILLED   fallback_gate_code_lost / guard_code_fixed_runtime (test_gate_fallback)
   SURVIVED xf_final_none            (success requires first=null, confirmed, service)
   SURVIVED xf_confirmed_equal       (confirmed requires first == final)
   SURVIVED xf_none_not_reached      (mutation none requires not_reached)
   SURVIVED xf_not_reached_equal     (not_reached requires first == final != null)
   SURVIVED xf_first_null            (redundant clause; equivalent mutant, see ST-B2)
   SURVIVED xf_first_prot_initial    (a first refusal must be initial)
   SURVIVED xf_final_prot            (final refusal: final+unconfirmed, or initial == first)
   SURVIVED validator_codes_any_str  (validator accepts {"code": <any str>})
   SURVIVED fallback_always_gate_ok  (record failure with a valid gate prints E_RUNTIME, not E_RECORD)
   SURVIVED codes_exact_runtime_relaxed (isinstance instead of exact RuntimeError)
   SURVIVED feedback_line_platform_seam (equivalent: entry() gates sys.platform first)
   SURVIVED rmtree_before_protected  (pre-existing fence, unchanged code; see ST-A6)
   ```

4. **SP-B2 AST pin.** I removed `stderr=subprocess.DEVNULL` from each of the
   three outer `check_output` calls in an in-memory `SOURCE` copy.
   `test_subprocess_sinks` failed by assertion in all three cases: #1, #2 and
   #3 `KILLED`.
5. **Validator behaviour probe.** I built valid JSON lines with `json.dumps`
   and fed them to the real `replay`. All five legitimate shapes are
   accepted: success, body fail with cleanup ok, cleanup-only, pre-try and
   both refusals. Each single-clause violation is rejected with `E_RECORD`:
   c1, c2, c3, c4, c6, c7, an unknown code and a bare `{"code":"E_PROTECTION"}`.
   So the code is correct and only the tests are missing. The probe also
   confirmed that no baseline test writes to the real stderr.
6. **CODES closure (AST).** `CODES` has no duplicates. Every literal passed
   to `require`/`RuntimeError` is in `CODES`. The only `CODES` entries without
   a raising literal are the four mapped codes (`E_COMMAND_FAILED`,
   `E_COMMAND_TIMEOUT`, `E_OS`, `E_RUNTIME`). The nine `E_GATE_*` names
   exactly equal `gate`'s source key set.

## Prior blocker disposition

| Prior blocker | Disposition | Evidence |
| --- | --- | --- |
| ST-B1: run/entry transitions, tokens, phase, bounds untested (14/15 survived) | **Closed** for all 15 named mutants | Check 2. `scenario()` drives the real `entry()` → `main()` → `run()` → `run_body()` with every effect mocked (no `Path`, `subprocess`, `rmtree`, `cg_preflight`, `copy_tree` or `fixture_cleanup` effects). It asserts the exact record, provenance equality, `rmtree` count, and that cleanup runs with phase `final` and cleanup still `unconfirmed`. The counterfactuals are valid JSON and isolated: a same-value duplicate key, a whitespace-padded line over 4096, a non-ASCII key pinned to `E_RECORD_BOUND$`, and unequal SHAs. `test_success_and_both_refusals` asserts `first != final`. |
| SP-B1: fixed codes collapse to `E_RUNTIME` | **Closed in behaviour** | `exception_record` maps an exact `RuntimeError` with one exact `str` argument in `CODES`. It maps exact `TimeoutExpired` to `E_COMMAND_TIMEOUT`, exact `CalledProcessError` to `E_COMMAND_FAILED`, any `OSError` to `E_OS`, and everything else to `E_RUNTIME`, without formatting. `first` is the body exception, including pre-try exceptions through the resetting wrapper. It is null only on success or a cleanup-only failure. `final` is the escaping exception. The cross-field rules are enforced (check 5). In run mode a gate failure prints its fixed gate code, never `E_RECORD` (`test_gate_fallback`). `E_SERVICE_OR_TEST` is preserved (`test_run_orderings`). Guard and internal modes print only a closed code. |
| SP-B2: outer `check_output` stderr reaches the log | **Closed** | All three calls (`driver.py:245`, `:471`, `:479`) pass `stderr=subprocess.DEVNULL`. The AST test requires `stderr` on every `subprocess.*` call, and `stdout` too except on `check_output`. Check 4 shows it is red-capable. The README documents the closed fallback line as the only exception. |

Hostile payloads are never formatted. `record_error` and `gate_error` are
never stringified. The emitted record is still one strict ASCII line of at
most 4 KiB with source SHA equal to workflow SHA. Cleanup fences are intact:
in the mocked run, a cleanup refusal gives `rmtree` count 0. Failures exit
with fixed code 1. There is no invented phase and no invented progress:
`account` and `cgroup` are never assigned.

## Standards

Sources: AGENTS ("Every behavior needs a meaningful test"; "Never claim a
check that was not run") and the Fowler smell baseline. Smells are
judgement calls.

**ST-B2 (new blocker, tests only): the correction's new replay validator is
not isolated by any test.** The correction added the cross-field rules and
the closed-code acceptance in `validate_record`
(`driver.py:134`, `:144-160`) and the record-failure fallback code
(`driver.py:593-594`). In check 3, 6 isolable cross-field clauses, the
closed-set acceptance and the `E_RECORD` fallback all survive single-point
mutation. The three cross-field counterfactuals in `test_counterfactuals`
each violate the success clause plus a second clause. That is the same
"fails the schema anyway" pattern ST-B1 rejected. No test covers a
refusal-phase violation, even though the evidence lists "refusal-phase
records" among the rejections. The code behaves correctly (check 5), so no
source change is needed. Required, as valid-JSON lines through `replay`,
each violating exactly one rule:

- success with `mutation` not `service`;
- `confirmed` with two different fixed codes;
- `mutation:none` with `cleanup:unconfirmed`;
- `not_reached` with `first != final`;
- `first` as a `final`-phase refusal;
- `final` as an `initial`-phase refusal that differs from `first`;
- `{"code":"E_NOT_A_CODE"}` in both slots;
- a run-mode `entry()` with a valid gate whose record fails validation. It
  must print exactly `unix experiment refused/failed: E_RECORD\n` to stderr
  and nothing to stdout.

Add all eight as compiled kills (the six isolable `xf_*`,
`validator_codes_any_str` and `fallback_always_gate_ok`) to `MUTANTS`. Clause
`final is not None and first is None → unconfirmed and mutation != none`
(`driver.py:154-155`) is implied by the confirmed, not-reached and
none-mutation clauses together. Its mutant is equivalent and needs no kill.
Either delete the clause or keep it as documentation, without claiming test
credit for it.

Advisories (not blocking):

- **ST-A1 (carried).** Duplicated triple assembly in `validate_record` and
  `replay`.
- **ST-A2 (carried).** The dead `account` and `cgroup` values remain in
  `MUTATIONS`. They are disclosed, and F0b replaces them.
- **ST-A6.** No F0 test pins `protected(stage,'run_cleanup')` before
  `rmtree` in `run_body`'s finally. The code is unchanged and the prior
  review's probe covered it. It is not newly at risk. The pin could be one
  scenario where `protected` refuses at `run_cleanup` and `rmtree` count is 0.
- **ST-A7.** `CODES` is a hand-maintained tuple. A new `require` literal
  added later would silently map to `E_RUNTIME`. Pin the AST closure from
  check 6 in a test.
- **ST-A8.** `codes_exact_runtime_relaxed` survives. The behaviour is benign
  because the argument still needs an exact `str` in `CODES`, but a
  `RuntimeError` subclass carrying a code is untested.
- **ST-A9 (evidence wording).** The evidence's restoration SHA-256
  `6f029b37…` is the CRLF working-tree rendition of `driver.py`
  (`core.autocrlf=true` on the builder's checkout). The committed LF blob
  hashes to `34a7111c…`. The self-check is consistent. A future evidence
  update should say which bytes were hashed. This is not a false claim.

The new tests carry no smells beyond that. `test_f0_blockers.py` is dense,
with one-line `with` chains and long lines, but the repo documents no style
rule against this.

## Spec

Spec: the accepted proposal's Stage F, F0 (`docs/reviews/m2-remaining-gates-proposal.md`),
plus the prior review's SP-B1 and SP-B2 fix contracts.

**PASS.** The proposal says: "The top level records the first and the final
exception separately, plus fixed mutation tokens ... and a cleanup token".
The code now does this through the real wrapper and exception ordering. The
proposal also says: "The workflow prints only that validated line". The
outer stderr sinks are closed, and the only other output is the closed
fallback line. "Tests exercise the real predicate, both initial/final
exception orderings, hostile payload suppression and every bound": all met;
the validator-isolation gap is recorded once, under Standards. The README
states the replacement success definition (SP-A2) and the BaseException and
termination no-record case (SP-A3). It also states that internal capture at
the seven sites remains uncovered.

Advisories:

- **SP-A1 (carried).** Replay still conflates predicate drift with
  malformation.
- **SP-A4 (carried).** Compatibility of `disposable.rs` with the
  post-0019/0020 Runner contract is a known unknown.
- **SP-A5 (new, disclosed deviation).** The prior fix contract asked for
  `E_OS_<NAME>` when `errno.errorcode` knows the errno. Sol maps every
  `OSError` to fixed `E_OS`. This is safe, closed and disclosed. OS failures
  stay distinct from other classes, but errno granularity drops below the
  historical `E_OS_<errno>` floor. I accept this for the local F0 mechanism.
  Any F2 proposal that relies on distinguishing OS failure classes must
  first add a closed `errorcode`-derived set or explicitly disposition the
  loss.

## Cross-process capture (unchanged disposition)

The seven internal sites remain uncaptured: `installed`, `service_stage`,
`service_cg`, `service_runs`, `capture_base`, `harness_stage` and
`harness_binary`. In the outer record they now surface as the fixed
`E_SERVICE_OR_TEST` classification, which `test_run_orderings` pins. They do
not surface as `E_RUNTIME`. Neither the README nor the evidence claims
complete capture. The prior disposition stands:

- this gap does not block an F0 merge;
- it blocks any F2 or complete-capture claim;
- the F0b contract in the prior review is the separate remedy.

## Gates (unchanged)

- No real captured record exists, and the real-record replay gate is
  **OPEN**. No diagnosis, hypothesis, historical reproduction, retry or fix
  is claimed or credited. Debug Phase 1 has no red-capable loop on the real
  symptom.
- A new source SHA does not discharge ADR 0019 D7, and failure counters are
  not reset. This recheck authorizes no experiment or activation.
- Real-Runner PA6 is **OPEN** and M2 is incomplete. Native, installed and
  dedicated paths are **unverified on real hardware**.
- Publication needs ST-B2's tests landed and rechecked by a different
  provider, then the coordinator's normal required gates. When the only
  delta is tests, a narrow recheck of ST-B2 suffices. A future PASS permits
  coordinator publication through the normal gates only.

## Summary

Standards: 1 new blocker and 6 advisories (ST-A1, A2, A6–A9). The worst is
ST-B2: the new cross-field and closed-code replay validator and the
`E_RECORD` fallback survive 8 isolable mutants. The fix is tests only. Spec:
PASS, with 3 advisories (SP-A1, A4, A5). The worst is SP-A5: `E_OS` without
errno, below the historical floor, accepted for F0 and to be dispositioned
before F2. All three prior blockers are closed.
