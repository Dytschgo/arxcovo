# M2 A7/A11 path-role attribution — independent Muse review

Scope: parent `8ce1788` on `work/m2-a7-path-attribution`; feature Sol
`a36e2e8` retained as `1c98fc3` with empty diff on all four
source/evidence files (reviewer-verified: `git diff a36e2e8 1c98fc3`
on `driver.py`, `test_driver.py`, `mutation_check.py`, and the
diagnosis file is empty; the two commits differ only in coordinator
`PROGRESS.md` and the parallel assessment file, which are out of
scope). Read-only review; no subagents. Read: `AGENTS.md`, `KICKOFF`,
ADR0019 D7i (consumed form), the diagnosis evidence
(`docs/evidence/linux-a7-a11-path-refusal-diagnosis.md`), the closed
record (`docs/evidence/linux-a7-a11-run-43715436515.json`) and
disposition (`docs/evidence/linux-a7-a11-hosted-disposition.md`), and
the parallel Muse failure assessment. Builder-executed checks are
reused (41 compatibility tests, 11 path mutants assertion-killed,
`git diff --check`); no rerun here — the delta is pure-mock
validation tooling and this review is a source/schema audit, and the
session carries a low-disk no-build constraint. PR129 was
independently reviewed separately (comment `6894592655`) with
platform CI still pending; it was not polled here.

Feature commit `1c98fc3` touches exactly four files, all inside
`tools/unix-exec-validation/` plus its evidence note: `driver.py`
(+36), `test_driver.py` (+173), `mutation_check.py` (+38), and the
diagnosis file (+124). No ADR, PROGRESS, workflow, package,
production, Rust, helper, or Cargo change. No generic infrastructure.

## Standards — PASS

One concern (attribute pre-mutation A7 path refusals), one small
conventional commit, a test for every behavior, and the evidence
note kept with the code change. No process theater and no new
allowlist, channel, audit framework, or verbose-error path. The
failing-before/passing-after pair is recorded: before the delta,
`test_fixed_role_attribution` failed on
`{'code': 'E_PATH'} != {'code': 'E_PATH', 'role': 'a7_c2_parent'}`;
after, all four real-chain role cases plus privacy/schema
regressions pass.

## Spec — PASS

Only the four existing pre-mutation alias/C2 `sudo_nnp_open` call
sites are wrapped, each with one fixed role (`a7_c2_parent`,
`a7_alias_root`, `a7_alias_target`, `a7_c2_image`), and only an
exact `RuntimeError('E_PATH')` while `feedback_mutation == 'none'`
becomes `A7PathRefusal` ([driver.py:218](C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3beb0fc7-66b744c08ac1bd2a\tools/unix-exec-validation/driver.py:218)).
OS errors, later admissions, and every other refusal code keep
their old form (`test_only_initial_path_code_is_attributed`
covers `E_OS`, `E_ARTIFACT`, `E_RUNTIME` passthrough and the
`staged`-mutation code-only path). `A7PathRefusal` is exact-type
checked, carries code `E_PATH`, and its constructor rejects any
role outside the closed `A7_PATH_ROLES` tuple with `E_RECORD`
([driver.py:886](C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3beb0fc7-66b744c08ac1bd2a\tools/unix-exec-validation/driver.py:886)). `exception_record`
re-validates the role and emits exactly `{code, role}`
([driver.py:897](C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3beb0fc7-66b744c08ac1bd2a\tools/unix-exec-validation/driver.py:897)); `validate_record` admits
the `{code, role}` shape only for `E_PATH`, only with a tuple
role, and only with `mutation_lower_bound == 'none'`
([driver.py:944](C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3beb0fc7-66b744c08ac1bd2a\tools/unix-exec-validation/driver.py:944)), while historical
code-only records still replay (closed-record regression test
included). F0/fences/first/final/cleanup/precedence behavior,
F0b transport, and the no-raw-metadata rule are unchanged.

Effect boundary is preserved: a role labels the failing *call*,
not a concrete failed ancestor or conjunct, so the delta claims
no historical cause and proves no host-metadata fact — consistent
with the diagnosis, which localizes the most direct source-local
`E_PATH` candidate to `a4_directory` inside alias/C2 traversal
but leaves the historical role and failed conjunct unknown, with
no actual logic defect proved. The qualifying controls (root-owned
`0755` ancestry, merged-`/usr` alias, C2 absent or exact mocked
pin) still admit, so no path bypass is introduced; relaxing root
ownership or `no022` remains out of scope. Held-descriptor
closure holds: the failing open raises before its fd is appended,
and the existing `finally` closes every held fd
(`assertCountEqual(closed, held)` on every refusal path).
Failure cleanup is unchanged: refusal precedes staging, so
mutation stays `none`, cleanup stays `not_reached`, internal
stays null, satisfying the existing `validate_record`
consistency checks.

## Security — PASS

The whole exception payload cannot leak an unknown role or a
subclass. Construction rejects unknown roles (`E_RECORD`, tested
with `'/private'` and `7`); `exception_record` uses exact-type
checks, so an `A7PathRefusal` subclass falls through to
`E_RUNTIME` and emits no role; extra keys (`uid`, `metadata`,
`payload`), wrong codes, non-`none` mutations, and internal-mode
carriage of the role shape are all rejected by tests. No path,
UID, mode, metadata, hash, payload, exception string, or
traceback is added, and `raise ... from None` suppresses chain
detail. The closed schema admits exactly four fixed strings, so
there is no oracle for attacker-controlled content. Residual
risk is unchanged: this is validation-tooling feedback shape,
not an execution grant.

## Blockers and publication eligibility

No concrete blockers in this delta. Eligible for publication as
a validation-tooling correction on the stated terms: it grants
no execution, retry, or permission; D7i stays consumed with no
retry or replacement authority; any future execution needs a
fresh reviewed disposition, and source alone grants nothing.
A7/A11, M2, full F/G, and real-Runner PA6 remain **unverified
on real hardware**.
