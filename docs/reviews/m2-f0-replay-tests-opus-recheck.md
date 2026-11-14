# M2 F0: narrow recheck of the ST-B2 replay-test correction

2026-11-14. Anthropic / claude-opus-5-5 / high, review sub-agent (single
agent, no subagents). Builder: OpenAI / gpt-6.1-sol / medium. This is the
different-provider recheck of Sol's test-only correction for ST-B2 in
`docs/reviews/m2-f0-blocker-correction-opus-recheck.md` (original `d608f5c`,
retained `7397353`). That review closed all original source blockers (ST-B1,
SP-B1, SP-B2). Scope is only ST-B2: the new tests, their mutants and the
evidence and progress lines that describe them. Previously accepted source is
not re-reviewed or redesigned.

**Verdict: Standards PASS. Spec PASS. ST-B2 closed.** All eight required
cases are present. Each one isolates its own rule. Each new mutant fails by
assertion, with no errors and no compile credit. The baseline is green, and
the driver source is unchanged. No new blocker. This PASS allows publication
through the coordinator's normal required gates only. It grants no run,
dispatch, retry, merge or experiment authority.

## Isolation and exact pins

The cwd equalled the Git top level, an isolated T3 worktree on branch
`t3/review-f0-replay-tests`. Status was clean before and after every check.
HEAD was `c81a45737ab95ffc760120eeff66e5d2ef077b7b`. I read AGENTS.md and
the dytschgo-review skill.

| Object | Pin |
| --- | --- |
| Base (prior review, retained) | `73973534829bb8ffc3327daf228e4e302a54f60a` |
| Prior review (original) | `d608f5c520a33353c9300638a38529f3f0876838` |
| Source correction (original) | `93b1194cb9efc3101dec8c0e5779f87edc1b7d95` |
| Reviewed head | `c81a45737ab95ffc760120eeff66e5d2ef077b7b` |
| `driver.py` blob (unchanged) | `cf5a0ca64707ccf95036490a06c7dc3b4cb86573`; raw and committed-blob SHA-256 `34a7111cb5d2c02993c6ba4836f0cff893b99e4b5b0564818372eaaf7de66021` |
| `test_f0_blockers.py` blob | `3e2a4172…` → `6545f5d7245c9bd5dc0eee311a0245968a8d0f0d`; raw SHA-256 `7bcd744737cd5b3376e59d62103815e897079df50bdfff971b4ce697a2e29bdd` (working copy and committed blob agree) |
| `test_driver.py` / README / `mutation_check.py` (unchanged) | `0baeeb58…` / `aaa39753…` / `fe5b976b…` |
| Evidence / PROGRESS at head | `6b2f2056…` / `77d73d5d…` |
| AGENTS | `c2ae2594…` |

`git diff --stat 7397353 HEAD` lists only three files: `docs/PROGRESS.md`
(+2), `docs/evidence/unix-protection-f0-feedback.md` (+24) and
`tools/unix-exec-validation/test_f0_blockers.py` (+29). There are no
deletions. `git diff --check 7397353 HEAD` is clean. The `tools/` tree
differs from the base in `test_f0_blockers.py` alone.

## Checks run

These are the only checks I ran. All of them run in memory or as Python unit
tests on Windows (Python 3.12.10). There was no CLI subprocess, native
fixture, system driver, stat observation, provisioning, Cargo, GitHub,
install or dispatch.

1. **Documented safe subset** (evidence line 137 command, `python -I -B`):
   F0Blockers, F0Mechanism, Guards without `test_cli_wrong_platform_refuses`,
   and CgroupScan. **32 tests, OK, 0.207s, exit 0.**
2. **In-memory mutation runner** (`t.mutations()`): the baseline passed.
   All 23 compiled mutants were `KILLED` by assertion failures, and the runner
   rejects any errors. The final line was `source unchanged
   sha256=34a7111c…6021`, exit 0. The new mutants all report
   `assertion=runTest`, except `fallback_always_gate_ok`, which reports
   `assertion=test_valid_gate_invalid_record_fallback`. A unittest
   `_SubTest` failure reports `runTest` as its method name. Check 3 resolves
   which case killed each mutant.
3. **Review-only isolation probe.** The script ran from the session
   scratchpad, outside the repo. It compiled the same 23 mutants in memory,
   built the seven cross-field records exactly as the test does, and fed them
   through `replay`:
   - Baseline: all seven records were rejected with exactly `E_RECORD`. The
     15 original mutants and `fallback_always_gate_ok` also rejected all
     seven.
   - Each new validator mutant accepts exactly one record: `xf_success`
     accepts [0], `xf_confirmed` [1], `xf_none` [2], `xf_not_reached` [3],
     `xf_first_phase` [4], `xf_final_phase` [5] and
     `validator_codes_any_str` [6]. So every case violates exactly one rule,
     and that rule alone makes the record valid when it is removed.
   - Subtest attribution: each `xf_*` mutant and `validator_codes_any_str`
     fails only `test_isolated_replay_cross_fields` with
     `{'case': <its index>}`. `fallback_always_gate_ok` fails only
     `test_valid_gate_invalid_record_fallback`. All have 0 errors.
   - Fallback cause: with the setUp feedback state (`first=None`,
     `mutation=none`, `cleanup=not_reached`), `gate` passes and
     `feedback_line(env, None, None)` raises `RuntimeError('E_RECORD')` from
     `validate_record`. The fallback test therefore exercises the valid-gate
     `else` branch for a real record-validation failure. It asserts an empty
     stdout and an exact `unix experiment refused/failed: E_RECORD\n` stderr.
   - First-null clause (`driver.py:154-155`): neutralising it changed the
     verdict on 0 of 375 enumerated records (5 first × 5 final × all
     mutations × all cleanups). The clause is redundant and its mutant is
     equivalent. It has no `MUTANTS` entry and receives no kill credit. The
     evidence states this correctly.
   - The driver hash was re-read after the probe and is unchanged. The
     worktree stayed clean.

## Standards

Sources: AGENTS ("Every behavior needs a meaningful test"; "Never claim a
check that was not run") and the Fowler smell baseline (judgement calls).

No blockers. ST-B2 is **closed**. The new tests follow the requirement in
the prior review: valid-JSON lines through `replay`, each violating exactly
one rule, pinned to `^E_RECORD$`, with all eight mutants added as compiled
kills.

Advisories (not blocking):

- **ST-A10 (new).** The tests cover the prior review's eight whole-rule
  mutants. Conjunct-level mutants inside a rule are outside ST-B2. One of
  them is non-equivalent and survives: dropping `cleanup == 'confirmed'` from
  the success rule. Without that conjunct, the validator accepts
  `first=final=null, mutation=service, cleanup=unconfirmed` (1 of 375
  records differs). The probe found three other conjunct drops equivalent:
  `first is None` in rule 1, `final is not None` in rule 4 and
  `cleanup == 'unconfirmed'` in rule 7. One more isolated replay case would
  close the gap. This is optional hardening, not a regression.
- **ST-A11 (cosmetic).** `mutations()` prints `runTest` for subtest kills.
  Printing `test_case._testMethodName` with `params` would make the evidence
  name the case directly. The evidence already explains the 0-6 index
  mapping.
- Carried advisories from the prior review are unchanged: ST-A1, A2, A6, A7
  and A8. ST-A9 is resolved: the evidence now says `6f029b37…` was the CRLF
  working copy hash. I confirmed that the committed LF blob and the current
  raw bytes both hash to `34a7111c…`.

Smells: the new tests use dense one-line `with` chains and a list built by
repeated `r = self.valid(); r.update(...)`. These match the file's existing
style. No repo standard is broken, and nothing was flagged beyond that.

## Spec

The spec is ST-B2 in the prior review and the F0 evidence appendix.

PASS. All eight required items are present and correct:

| ST-B2 requirement | Test / mutant | Probe |
| --- | --- | --- |
| success with `mutation` not `service` | case 0 / `xf_success` | accepted only under its mutant |
| `confirmed` with two different fixed codes | case 1 / `xf_confirmed` | ✓ |
| `mutation:none` with `cleanup:unconfirmed` | case 2 / `xf_none` | ✓ |
| `not_reached` with `first != final` | case 3 / `xf_not_reached` | ✓ |
| `first` as a `final`-phase refusal | case 4 / `xf_first_phase` | ✓ |
| `final` as a differing `initial`-phase refusal | case 5 / `xf_final_phase` | ✓ |
| `{"code":"E_NOT_A_CODE"}` in both slots | case 6 / `validator_codes_any_str` | ✓ |
| valid-gate run-mode record failure → exact `E_RECORD` stderr, empty stdout | `test_valid_gate_invalid_record_fallback` / `fallback_always_gate_ok` | cause confirmed as `validate_record` |

The evidence claims match what I observed: 32 tests (my run 0.207s, theirs
0.205s), 23 assertion kills with no errors, the names of the eight
additions, subtest indices 0-6, a named fallback test, no credit for the
redundant clause, an unchanged driver with the CRLF hash corrected, and the
test file SHA-256. The PROGRESS line is consistent with the evidence and
claims no publication, real-record replay, dispatch or activation. The
evidence also says the builder's first edit script refused on its own anchor
before writing anything. That is a process note, which I cannot verify and do
not credit.

## Scope limits carried forward

- There is no historical reproduction or real-record replay, and none is
  credited. The real-record replay gate is **OPEN**. The tests use synthetic
  records only.
- The internal cross-process capture gap stays in the separate F0b
  follow-up. It is an F2 blocker, not an F0 merge blocker.
- No new SHA supplies experiment authority. ADR 0019 D7 is not discharged,
  and failure counters are not reset.
- M2 is incomplete. Real-Runner PA6 is **OPEN**. Native, installed and
  dedicated paths are **unverified on real hardware**.
- This PASS allows publication through the coordinator's normal required
  gates and only through them.

## Summary

Standards: 0 blockers and 2 new advisories. The worst is ST-A10: a surviving
non-equivalent conjunct mutant in the success rule. It is outside ST-B2 and
optional. Spec: PASS, with all 8 ST-B2 items isolated and assertion-killed.
