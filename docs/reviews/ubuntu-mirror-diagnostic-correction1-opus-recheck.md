# Ubuntu mirror read-only diagnostic: correction 1 delta recheck

Anthropic / Opus 5.5 / high. Fresh independent Standards/Spec/security delta review,
read only, 2026-11-12. The source author is a different provider (OpenAI gpt-6.1-sol
high). Single-agent review, no subagents. An earlier recheck was parked before it
finished; this review starts over and does not rely on it.

**Verdict: ACCEPTABLE FOR EXECUTION REVIEW. No blockers.** Every required item from
the prior review (S1, T1, T2) and the recommended T3 is implemented, and I checked
each one against the source, not the summary. The change adds no writer, no network
access and no apply/_apply call, and it keeps the closed numeric output. Every
failure path still exits 1 (2 for arguments) and is bounded and sanitized. Two Low
findings and several Info notes remain; none of them blocks. This review authorizes
no push, label, hosted run or write. The coordinator/root accepts and controls
publication.

## Pins

- Runtime worktree: `<prepared-worktree>` (isolated T3 checkout). `pwd` matched
  `git rev-parse --show-toplevel`, and the initial status was clean. The path is not
  canonical `D:/Code/Arxcovo`. The checkout HEAD is `882917f` (product docs), which
  does **not** contain the target. All review used Git objects and a private
  `git archive bd707ae8` export in session scratch. I never used checkout source.
- Target (evidence) head: `bd707ae8ca3ddcc500b5c4395ab23d546f480938`.
  Source/test/workflow: `b1c7a4d9b23302f69adcb59703c9077d977d2d77`.
  Correction base: `22a98f61eb4b85e43fab65da0b3ca6939aba1b6d`, an ancestor of the target.
- `22a98f6..bd707ae` touches exactly five files: `ci.yml` (+3), the collector (+83/-9),
  the collector tests (+253/-3), PROGRESS (+10) and the evidence file.
  `b1c7a4d..bd707ae` touches only PROGRESS and the evidence file.
- `tools/ci/ubuntu_mirror.py` and `tools/ci/test_ubuntu_mirror.py` are byte-unchanged
  from the base (`git diff --quiet`). The helper is 251 lines, and raw and CR-stripped
  SHA-256 are both `a945b3d8…409b80ff`, matching the collector's pin.
- All five correction-manifest hashes match `git show bd707ae8:<file>`.
  `git diff --check 22a98f6 bd707ae8` exits 0.

Read in full: AGENTS.md, the review skill, the prior review
(`docs/reviews/ubuntu-mirror-readonly-diagnostic-opus-review.md` at 22a98f6), the full
target collector, collector tests, helper, workflow `verify` job, the evidence file
and the PROGRESS delta.

## Checks I personally ran (independent credit)

All checks ran against the private export, on this Windows host only.

- `python -B -m unittest discover -s tools/ci -p 'test*ubuntu_mirror.py'` under
  CPython 3.12.10 ran **53 tests: 51 passed, 2 skipped** (both Unix-only native tests).
  Under CPython 3.14.2 the result was the same: 53 tests, 51 passed, 2 skipped. The
  3.14 run exercises the PEP 667 `FrameLocalsProxy.get` path.
- Seven negative-control mutants of my own, each run on a copy of `tools/ci`. In
  that copy the baseline has one expected error, because the workflow file is
  absent. Every mutant added failures beyond that baseline:
  `return 0` from main; swapped `entries` directory index; origin check removed;
  `has-signed-by` forced True; `tree.close()` removed; the cleanup `except BaseException`
  removed; and `netloc-host` forced True.
- Manifest hashes, `diff --check`, ancestry and the helper-unchanged check.

Not run: native Unix descriptor, FIFO, hard-link, ownership and real-signal behavior;
the hosted workflow; sudo; real `/etc`; production context, main or apply. The two
native tests are wired and skipped here. Native, installed and dedicated paths
remain **unverified on real hardware**.

## Prior findings: verified dispositions

| Prior | Status | Verified against source |
| --- | --- | --- |
| S1 raise site | **Fixed** | `refusal_facts` (`observe_ubuntu_mirror.py:55-102`) walks to the innermost `tb_next`. It emits a function ID (fixed 6-tuple, 1-based) and a line only when all of these hold: `_HELPER_PINNED`, `co_filename == _spec.origin`, `co_name in FUNCTIONS` and line 1..251. Otherwise it emits 0/0 and returns before reading any local. The helper is compiled from the same bounded bytes that were hashed (`:23-29`), so `co_filename` is the exact origin string and line numbers decode against the pinned SHA. `Report.emit` also bounds `raise-line ≤ 251` and `raise-function ≤ 6` (`:127-128`), and tests cover both rejections. |
| S1/S2 compound booleans | **Fixed** | Site `rewrite_sources:97`: `bool(fields.get(k))` for types/suites/components, `"signed-by" in fields`, and equality with the helper's own default. These are the helper's exact predicates (`ubuntu_mirror.py:95-96`). Site `:93`: `continuation` uses a leading space/tab; `duplicate` uses the helper's own fixed regex plus a membership test. URI sites `replacement:35/39` and `rewrite_mirror_list:166/169` re-split the named `uri`/`tokens[0]` and emit eight fixed comparisons; `ValueError` maps to all-False. Only named locals are read, and they are type-checked (`type(x) is dict/str/list`). Only `bool` values leave the function. `host-kernel` separates an already-rewritten list (169) from a foreign host (S2). I traced the short-circuit in `ubuntu_mirror.py:168`: line 169 is reached only for a host outside `HOSTS`, and an allowlisted-host refusal surfaces as `replacement:39`. The tests encode exactly that. |
| T1 native tests in CI | **Fixed** | New step `ci.yml:71-73`, directly after the observer, uses the identical `failure() && runner.os == 'Linux' && steps.ubuntu_mirror.outcome == 'failure'` condition. `python3 -B` runs as the runner user, with no `sudo`, `||` or `continue-on-error`. Because `failure()` is already true after the helper step, the step runs whether the observer step exits 1 or crashes, so a test failure cannot suppress the observation. A workflow test pins the order, the duplicate condition count, and the absence of sudo and `||`. New native test `:542-586`: real `scandir(fd)` names and an empty sources dir; real read of a regular file gives `owner=false` (non-root); FIFO, directory and hard link give `regular=false`, `regular=false` and `single-link=false`, each under 1 s; after unlink the regular read returns its bytes. The test-local `policy` wrapper substitutes only `st_uid=0` and asserts the real uid is non-zero. Both native tests skip as root before fixture work. |
| T2 main/entries | **Fixed** | `MainTests` injects `sys`, `signal`, `context`, `DescriptorTree`, `observe` and `print`, and asserts that nothing is written to stderr. A real complete fixture plan returns 1 with a final `done/publication-unresolved`. The internal `RuntimeError`, close `OSError` and close `Deadline` cases each give a fixed single row and no `private-owner` text. Extra argv returns 2 before `context` or the tree is constructed. `test_entries_selects_correct_directory` pins 1→fd[2] and 2→fd[3]. My mutants confirm that the return-value, directory-swap, close and cleanup-catch tests have teeth. |
| T3 alarm cleanup | **Fixed (alternative)** | `main` `finally` (`:328-339`) nests `try: setitimer(0) finally: tree.close()` inside `except OSError` / `except BaseException`. An alarm during cancellation or close still attempts close and yields fixed JSON. An alarm that arrives after the body but before `finally` lands in the body's `except BaseException`. The timer is one-shot, so after `setitimer(0)` returns no new alarm can arrive. A test covers both the alarm-free path and an alarm raised during cancel, using fake signals only. `pthread_sigmask` was not adopted. The remaining window is in I1 below. |
| T4/T5, S3/S4 | Unchanged, as accepted | Caps, suffix-format semantics and fence limits are unchanged and honestly restated. |

## Security checks (delta)

- **Read-only and no network:** the new imports are `hashlib`, `re` and `urllib.parse.urlsplit`, all pure. The only new filesystem access is one bounded `open(_spec.origin, "rb").read(65537)` of the sibling helper at import time. The workflow already trusts and executes that file as root in the previous step. The AST call-name fence still passes. No `apply`, `_apply`, `read_source`, staging, chmod, fsync, replace or socket call exists. Bounded descriptor-relative `O_NOFOLLOW`/`O_NONBLOCK` reads, policy, rechecks and every cap are byte-identical to the base.
- **Output privacy:** every new emission is a fixed predicate name with a `bool` or a bounded int (function 0..6, line 0..251). No traceback is formatted. No filename, source line, URI, host, keyring, environment, exception text or local value is emitted. The unknown-origin path returns before touching `f_locals`. The classification test asserts that the encoded report contains none of `ubuntu.com`, `kernel.org`, `private.invalid`, `/ubuntu`, `keyring`, `private-owner`, `/etc`, `Signed-By`, `Traceback` or the fixture name.
- **Exception containment:** any unexpected error inside `refusal_facts` (TypeError, Stop on report budget, Deadline) is raised inside the `except mirror.Refusal` handler. It therefore escapes `observe`'s sibling clauses and lands in main's `except BaseException`, which writes the fixed `internal/unknown` row and exits 1. The result is fail-closed and sanitized, but loses the classification (I2).
- **Exit codes:** main returns 1 on every non-argument path, including a complete dry plan, and 2 for any argv. No red-to-success path was found. The helper step, its tests and its required gates are unchanged.
- **Test step safety:** the tests use only owned `TemporaryDirectory` trees, an injected facade, and injected main, context and signal. No test opens real `/etc`, calls real `context`, installs a real timer, or uses sudo. The only repository file a test reads is `ci.yml`. The root observer runs with `-B`, so it leaves no root-owned pycache to interfere with the user-level test step. The step runs only on the failure path, adds a few seconds, and stays inside the 12-minute job timeout. There is no ci-time-budget or agent-review bypass.

## Standards

| # | Sev | Location | Finding |
| --- | --- | --- | --- |
| R1 | Low | `test_observe_ubuntu_mirror.py:547-583` | **The native test depends on the runner's umask.** Fixture modes come from the umask. Under umask `0002`, `regular`/`hardlink` would be `0664`, and policy checks `private-write` before `single-link`. The hard-link assertion would then fail with `['private-write', False]`. The loop aborts there, so the FIFO, directory and final read facts would go unasserted in the single hosted attempt. GitHub-hosted Ubuntu runners are believed to use umask 0022, but that is **unverified here**. The failure would be self-describing and would not affect the observer JSON or the red job. Hardening is test-only: `os.chmod(root/"regular", 0o644)` after creation. It is recommended and not required; applying it would need a minimal delta recheck. |
| R2 | Info | `observe_ubuntu_mirror.py:88-99` | `host` is bound only inside the `try`, and its later use depends on `parts is None` short-circuiting. That is correct as written but fragile. Possible smell: initialize `host = None`. |
| R3 | Info | collector `:69-83`, tests, evidence | The raise-site line literals (35/39/93/97/166/169) are duplicated across collector, tests and evidence. This is acceptable because the hash pin turns any helper drift into 0/0, and `assertTrue(diag._HELPER_PINNED)` fails the suite. Judgement call (Primitive Obsession), not a defect. |

Smell baseline: nothing else material. `refusal_facts` reads frame locals by design,
so Feature Envy is not applicable for a frame-introspection diagnostic.

## Spec (prior review "Required before execution" 1–5 and hosted-run plan)

| # | Sev | Spec line | Finding |
| --- | --- | --- | --- |
| P1 | Low | Hosted-run plan: "Record … the native collector test result" | **Evidence should record the native test count, not just pass/fail.** Native tests skip as root, which still shows as a green step. On the hosted non-root runner the expected outcome is `Ran 30 tests … OK` with **no skips**. Any `skipped=N` means native coverage did not run and must be recorded that way. Add this to the coordinator's run protocol; no code change is needed. |
| P2 | Info | Required item 2: compound booleans at `:166` | At `rewrite_mirror_list:166` the URI booleans describe `tokens[0]`, but they do not separate "token count not 1/2" from "priority format". The raise line already says the shape was wrong. A third boolean (`token-count-ok`) would close the gap. Not requested; not required. |
| I1 | Info | T3 | Residual cosmetic window: if the one-shot alarm fires exactly while main's `except BaseException` assignment runs (after a different exception), `finally` runs and the Deadline then re-raises after it. Print is skipped, Python writes a traceback (code lines and the workspace path, no runner data) and the exit is non-zero. Practically unreachable, and still red. The same class applies to import-time failure (helper unreadable or larger than 64 KiB+1, causing a truncated compile), which happens before main's guard. |
| I2 | Info | Required item 1 | Errors inside `refusal_facts` degrade to `internal/unknown` (fail-closed) rather than keeping `parser-unknown` plus partial facts. Unreachable with the pinned helper and a report budget of about 520 rows. |

Satisfied: item 1 (raise site, origin and identity gated, 0/0 otherwise). Item 2 (all
listed booleans at 97, 93, 35/39 and 166/169, booleans only). Item 3 (step wiring, plus
native regular/FIFO/hard-link/directory/owner=false cases). Item 4 (main and entries).
Item 5 (every prior S1 probe is mapped: ports/EC2 regional → 35, inline continuation
`Signed-By` → 93, foreign/kernel mirror → 169, 17 entries → 177, plus a redaction
assertion on every case). The S2 Kernel-host boolean is present. The evidence
honestly keeps the post-failure snapshot limit and the absence of a faithful
failing-image fixture.

## Safe execution disposition

**Eligible for exactly ONE first-attempt hosted run, and only after root/coordinator
acceptance of this review.** This review does not authorize it.

- Use the separate diagnostic branch at an exact reviewed head (`bd707ae8`, or a head
  whose only delta is review docs). Not PR54, not performance attempt 3, no
  `ci-speed` label, no workflow dispatch on PR53/54.
- Record:
  - the runner image revision;
  - the helper's token;
  - the single observer JSON line, decoded with the version 2 tables and the pinned helper SHA `a945b3d8…`;
  - the native test result with its exact counts and skip count (P1).
- No blind rerun to catch a particular image; the `ubuntu-latest` revision is not
  controllable. If the helper passes, record "not reproduced on revision X" and stop.
  If the complete plan passes, record `publication-unresolved` and authorize no write.
  The job stays red. No gate, budget, timeout, OS-matrix or signed-mirror policy
  change follows from any outcome.
- Budget state carried forward. Performance is at 2 of 3 attempts failed, and this
  diagnostic neither consumes nor resets attempt 3. Ubuntu 26 fetch-path incident
  failure 43270363385 counts as one failed attempt, and there is no reset. Main's
  906 s failed baseline remains authoritative. Attempt 2 remains nonqualifying.

## Limits carried forward

M2 is incomplete. Native, installed and dedicated paths are **unverified on real
hardware**, and Unix descriptor and real-signal semantics are unverified locally
(Windows host). ExecutionFailed/E_PROTECTION are a separate, unknown issue, with no
fix, waiver or retry. I did not touch GitHub, push, labels, CI, sudo, real `/etc`,
native host workflows, owner/admin/account/service, cgroup, VM/Lab Manager,
experiments, private state, ignored fixtures or the babysitter, and installed no
toolchain. PROGRESS and source are unchanged by this review.

**Count:** Standards 3 findings (worst: R1, native test depends on umask, Low).
Spec 4 findings (worst: P1, record native skip count, Low). No blockers.
