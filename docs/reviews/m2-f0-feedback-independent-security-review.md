# M2 F0: independent security/source review of the local protection feedback mechanism

2026-11-14. Anthropic / claude-opus-5-5 / high, review sub-agent (single
agent, no subagents). Builder: OpenAI / gpt-6.1-sol / medium. This is the
different-provider independent review for the F0 source. The original M2
remaining-gates design is Opus-authored. This review gives that design no
independent Opus architecture credit and reviews only Sol's implementation
against it.

**Verdict: Standards CHANGES REQUIRED (1 blocker). Spec CHANGES REQUIRED
(2 blockers).** The cross-process capture gap is **dispositioned explicitly,
not waived**. It does not block merging F0 as a bounded local mechanism once
SP-B1 is fixed. It **does block** any F2 diagnostic proposal that claims
coverage of internal-process sites or of account/cgroup progress. The remedy
contract is in "Cross-process capture contract (F0b)" below. This review
grants no run, dispatch, retry or merge authority.

## Isolation and exact pins

The cwd matched the Git top level, an isolated T3 worktree on branch
`t3/m2-f0-security-review`. The status was clean. HEAD was
`378e6b8c364d2e039a2dc05f2b3f8056deb8b1ce` (retained prepared head).

| Object | Pin |
| --- | --- |
| Prepared base (S2b publication) | `f55db98c275af28d93b3407c94ae861941692e45` |
| F0 source commit | `2c93991870ca108115dd04e1a7592d728fcfe2ff` (retained as `a872fa2`, same tree `8d69bc0e…`) |
| F0 evidence commit | `5775b406c4eae62a05cbd77f7ecab6dac2bbc7bc` (retained as HEAD `378e6b8`, same tree `ce21c6f3…`) |
| `driver.py` | `1ecc7341b194a4e565c1cc3431dcf1c152eae15f` |
| `test_driver.py` | `c20088afcd82189c9ce20cb6dbbdcc0b7c79affc` |
| `tools/unix-exec-validation/README.md` | `70373cbe6b68ebcaf0f360f094d35ae32b1860d1` |
| Evidence `docs/evidence/unix-protection-f0-feedback.md` | `7f51278ca0ddaaec66b9cf308cfa72447f2dd31e` |
| Unchanged workflow | `e5f1a96dbfa522bda602db45bf24db000ebddd5c` |
| Unchanged `disposable.rs` | `586b30811b3af485f6b96f7a4a989b2ba64e3e70` |
| Unchanged `mutation_check.py` | `fe5b976bd1b946c29012d33879536d172406dc9f` |
| Historical driver (base = `5b4f6b2`) | `4b4775f965d3d1d93f71aa7a88f6d2f46d792ddb` |
| Phase-1 report via `git show 20f39c0:…` | `26a0500631e1f77aad1486db0d9df31af1ce9adc` |
| Proposal / ADR 0019 / AGENTS / KICKOFF | `8fd8bdc0…` / `cc36ce4e…` / `c2ae2594…` / `9dc49e72…` |

`git diff --stat f55db98 2c93991` touches only the three tool files.
`git diff --stat 5b4f6b2 f55db98` over the workflow, `disposable.rs` and the
driver is empty, so the base driver is the historical dispatched driver. Read:
AGENTS, KICKOFF, the dytschgo-review and dytschgo-debug skills with their
active profiles, the proposal's F0/F1/F2 and Stage F sections, ADR 0019 D7 and
C2, the historical phase-1 report, the full F0 driver, the tests, the README
diff, the evidence file, and `disposable.rs` lines 36-51 and 150-160.

## Checks actually run (all synthetic, in-memory)

No CLI driver invocation, native command, provisioning, account, service,
cgroup, `/proc`, `/sys` or stat observation, VM, private state, GitHub,
network, CI or dispatch was used. Throwaway scripts lived in the session
scratchpad and are not committed.

1. **Builder's safe subset** (Python 3.12.10, Windows): F0Mechanism + Guards
   (minus the CLI subprocess test) + CgroupScan. `Ran 22 tests … OK`. This
   matches the evidence claim.
2. **Review mutants.** Each is a single exact-once text substitution, compiled
   into a fresh module and run against the same 22 tests:

   ```text
   SURVIVED run_drops_first_capture      (run() no longer records first)
   SURVIVED run_drops_final_phase        (finally no longer sets phase=final)
   SURVIVED cleanup_confirmed_early      (finally starts as 'confirmed')
   SURVIVED mutation_staged_dropped
   SURVIVED mutation_service_dropped
   SURVIVED protected_ignores_phase      (Refusal always 'initial')
   SURVIVED replay_dup_keys_allowed
   SURVIVED replay_input_bound_dropped
   SURVIVED replay_ascii_dropped
   SURVIVED sha_equality_dropped
   SURVIVED protected_bound_dropped
   KILLED   conjunct_owner_first
   SURVIVED entry_first_is_final         (entry emits final as first)
   SURVIVED refusal_exact_type_relaxed
   SURVIVED site_token_unchecked
   ```

3. **Real `run()`/`entry()` ordering probe.** Every effect was mocked: a
   MagicMock stage, patched `subprocess`, `copy_tree`, `fixture_cleanup`,
   `rmtree` and `cg_preflight`. A synthetic `protected` raised a real
   `Refusal` with fixed triples. Results:

   ```text
   pre-try refusal (run_parent)     exit=1 first=E_PROTECTION@run_parent/initial final=same mutation=none cleanup=not_reached rmtree=0
   body E_BUILD, cleanup ok         exit=1 first=E_RUNTIME final=E_RUNTIME mutation=staged cleanup=confirmed rmtree=1
   body E_BUILD, cleanup refusal    exit=1 first=E_RUNTIME final=E_PROTECTION@run_cleanup/final mutation=staged cleanup=unconfirmed rmtree=0
   body refusal, cleanup refusal    exit=1 first=E_PROTECTION@run_artifact/initial final=E_PROTECTION@run_cleanup/final cleanup=unconfirmed rmtree=0
   body E_SERVICE_OR_TEST           exit=1 first=E_RUNTIME final=E_RUNTIME cleanup=confirmed
   body PermissionError(13, <path>) exit=1 first=E_RUNTIME final=E_RUNTIME (path not emitted)
   gate refusal, run mode           exit=1 stdout='' stderr='unix experiment refused/failed: E_RECORD'
   gate refusal, guard mode         exit=1 stdout='' stderr='unix experiment refused/failed: E_RUNTIME'
   max two-slot record (31+1 triples each) = 1596 bytes
   ```

4. **AST inventory.** There are 13 `protected` calls with 13 distinct literal
   tokens equal to `SITES`. Three `check_output` calls (`driver.py:217`, `:432`,
   `:440`) pass no `stderr=`.

None of this reproduces anything historical. It is mechanism and review
evidence only.

## What holds (verified)

- **Predicate equivalence.** `decide` refuses exactly when the historical
  `require(not S_ISLNK and uid == 0 and not mode & 0o022)` refuses. Ancestors
  are checked in the same order, and the first refusing item ends the walk.
  The conjunct order link → owner → write matches Python short-circuit order.
  Regular files stay allowed. `write_bits ∈ {0, 2, 16, 18}` is exact.
- **Fail-closed behaviour.** Every refusal or error path still raises and exits
  1. Cleanup refusal still escapes before `rmtree` (probe: `rmtree=0`). Success
  now also requires a valid record, so a record failure gives exit 1. The new
  pre-checks (`E_SITE`, `E_RECORD_BOUND`, an invalid phase in `Refusal`) all
  raise; none relaxes a fence.
- **Initial/final ordering in the real code.** `run()` records the body
  exception, switches to `final` before the cleanup checks, and marks cleanup
  `confirmed` only after `rmtree`. The probe confirmed both orderings and the
  pre-try path.
- **Payload hygiene.** `exception_record` uses an exact `type(...) is Refusal`.
  It never calls `str`, `repr` or `args` on other exceptions, as the hostile
  `__str__` test and the PermissionError probe show. The record carries no
  path, uid number, name, argv, environment or payload. Run and attempt are
  bounded GitHub IDs.
- **Strict parsing.** The parser takes a single ASCII line of at most 4096
  bytes with exactly one newline. It rejects duplicate keys, has exact key
  sets at every level, checks types exactly (bool is excluded from int), and
  maps RecursionError, ValueError and TypeError to `E_RECORD`. Overflow
  refuses and never truncates. The worst case of two maximal refusals measured
  1596 bytes.
- **Fences.** The workflow and `disposable.rs` are byte-identical to the
  historical dispatch. `gate` is unchanged. Provenance requires
  `source_sha == workflow_sha`, and `gate` also requires `GITHUB_SHA`. The
  README and evidence correctly say this is association, not attestation.

## Standards

Sources: AGENTS ("Every behavior needs a meaningful test"; "no secrets or
private owner data in … logs"), KICKOFF Working style, and the Fowler smell
baseline. Smells are judgement calls.

**ST-B1 (blocker): the new behaviours lack meaningful tests.** In review
check 2, 14 of 15 single-point mutants survive the committed tests. The
following are unguarded:

- the real `run()` except/finally transitions (`driver.py:479-495`);
- the mutation and cleanup tokens;
- phase propagation from `protected` into `Refusal`;
- `entry()` putting the body exception, not the final one, in `first`;
- duplicate-key rejection. Its test line `{"a":1,"a":2}` fails the schema
  anyway;
- the input bound. Its test line `'x'*4096` fails JSON anyway;
- the ASCII check;
- SHA equality.

`test_entry_fixed_failure_exit_and_single_line` mocks `main` and sets
`feedback_first` itself, so it never exercises `run()`. Required: tests that
drive the real `run()` with all effects mocked, as in review check 3, and
assert the exact record for each ordering. Also required: valid-JSON negative
lines (a valid record padded past 4096 bytes, a valid record with a duplicated
key, a valid record with a non-ASCII value), a valid record with unequal SHAs,
and an assertion that `first` differs from `final` in the emitted line. Add
the predicate, ordering and bound mutants above to `mutation_check.py`, or to
an F0 equivalent, as compiled kills.

Advisories (not blocking):

- **ST-A1, Duplicated Code.** `e['predecessors'] + [[e['kind'], e['owner_zero'],
  e['write_bits']]]` appears in both `validate_record` and `replay`. Extract a
  `refusal_triples(e)`.
- **ST-A2, Speculative Generality.** `MUTATIONS` lists `account` and `cgroup`,
  but nothing assigns them. This is disclosed. Either remove them or implement
  them under F0b, so the schema cannot suggest a progress level it can never
  report.
- **ST-A3.** `feedback_line` calls `gate(env, 'linux')` instead of
  `sys.platform`, a test seam in production. With a GitHub-shaped environment
  off Linux, it prints an associated `E_RUNTIME` record (exit 1) instead of
  `E_PLATFORM`. Pass the real platform and have the tests patch it.
- **ST-A4.** `run()` mutates the module globals `feedback_*` and nothing resets
  them. The tests patch only `feedback_first`. Isolation then depends on test
  order. Reset them at `run()` entry, or patch all four.
- **ST-A5.** No inventory test pins "13 calls, 13 distinct literal tokens equal
  to `SITES`". Add the AST check from review check 4.

## Spec

Spec: the accepted proposal's Stage F, F0 (`docs/reviews/m2-remaining-gates-proposal.md`).

**SP-B1 (blocker): first and final exceptions are not usefully distinguishable
because fixed codes collapse.** The spec says: "The top level records the
first and the final exception separately". The historical driver emitted
allowlisted fixed codes, and that is how phase 1 recovered `E_PROTECTION` at
all. F0 maps every non-`Refusal` exception to `E_RUNTIME`. That covers
`E_SERVICE_OR_TEST`, `E_BUILD`, `E_QUARANTINE_CGROUP`, `E_UNIT_COLLISION`,
OSError (formerly `E_OS_<errno>`) and timeouts (formerly `E_COMMAND_TIMEOUT`).
As probed:

- A gate refusal in run mode is mislabelled `E_RECORD`. In guard mode it loses
  `E_GATE_*`.
- An internal-process failure cannot be told apart from a build, collision or
  OS failure.
- "Body ok, cleanup failed" is indistinguishable from "body failed, cleanup
  failed" whenever both codes are `E_RUNTIME`, because `entry()` copies `final`
  into `first` when there was no body exception.

This regresses below the historical diagnostic floor. It also hides the
capture gap: the record cannot even say "an internal process failed". Smallest
safe fix:

1. Add a closed tuple `CODES` holding every fixed literal, plus the
   `E_GATE_<KEY>` names generated from `gate`'s fixed key set,
   `E_ACCOUNT_COLLISION` and `E_MODE`. Map with `type(error) is RuntimeError and
   len(error.args) == 1 and error.args[0] in CODES`, so no regex, no
   formatting and no payload.
2. Map `subprocess.TimeoutExpired` to `E_COMMAND_TIMEOUT` and
   `CalledProcessError` to `E_COMMAND_FAILED`. Map OSError to `E_OS_<NAME>`
   only when `errno.errorcode` contains its errno. Everything else stays
   `E_RUNTIME`. The validator accepts only that closed set.
3. Make `first` the first exception raised anywhere in `run()`, pre-try
   included, or `null`. Make `final` the escaping exception or `null`. Validate
   cross-field consistency:
   - `final` is null only on success, with `first` null, cleanup `confirmed`
     and mutation `service`;
   - a `final`-phase refusal implies cleanup is not `confirmed`;
   - mutation `none` implies cleanup `not_reached`;
   - `first` null with `final` set occurs only for cleanup-phase failures.
4. In run mode, if `gate` fails, print the fixed gate code, since no
   association exists. Never print `E_RECORD` for a gate refusal.

**SP-B2 (blocker; small and pre-existing): raw subprocess stderr can reach the
job log.** The spec says: "The workflow prints only that validated line." The
outer `run` process calls `check_output` without `stderr=` at `driver.py:217`
(`stat -f`), `:432` (`systemctl show`) and `:440` (`git rev-parse` with
`safe.directory=<checkout path>`). Their stderr goes straight to the public
log. The historical driver had the same three lines. F0 newly states the
single-line contract, and the README says the existing subprocess output
suppression is unchanged. Add `stderr=subprocess.DEVNULL` to all three, and
add an AST test that every outer `subprocess` call passes explicit `stdout`
and `stderr` sinks. The fixed fallback stderr line
`unix experiment refused/failed: <code>`, used when no record can be produced,
is acceptable as a closed-code line. Document it as the single exception.

**Spec test requirement.** The spec says "Tests exercise the real predicate,
both initial/final exception orderings, hostile payload suppression and every
bound." The predicate and hostile payload parts are met. Orderings through the
real `run()` and the input/duplicate/ASCII bounds are not; see ST-B1. This is
recorded here for traceability and is not a third blocker.

Advisories:

- **SP-A1. Replay conflates predicate drift with malformation.**
  `validate_record` requires `decide(triples)` to equal the recorded ordinal.
  A real line that disagrees with a later predicate therefore fails as
  `E_RECORD`, the same outcome as garbage. Split structural validation from
  agreement. `replay` should return `(recorded, computed)` and the replay test
  should assert their equality. A disagreement then shows as a red assertion,
  not a parse refusal. This is still red-capable today, but less legible.
- **SP-A2. The acceptance-marker contract changed silently.** The lines
  `harness: 1 passed; …` and `cleanup: confirmed` are gone. Those lines define
  `harnessPassObserved` and `cleanupConfirmedObserved` in
  `docs/evidence/unix-execution-first-dispatch.json`. The README must state the
  replacement: success means `first=null`, `final=null`, `mutation=service`
  and `cleanup=confirmed` in one validated line. A future acceptance review
  must use that definition.
- **SP-A3 (pre-existing).** `except Exception` does not catch BaseException
  subclasses, and SIGTERM from a job timeout skips `finally`. In both cases no
  record is emitted and cleanup is not reached. Unknown cleanup already
  excludes that environment. Note this in the README as a no-record case.
- **SP-A4.** The compatibility of `disposable.rs` with the post-0019/0020
  Runner contract remains a known unknown. The evidence's source observations
  are fine as observations only.

## Cross-process capture gap: disposition

**Facts.** Seven of the 13 sites run only in internal processes:

- `installed` in every internal mode;
- `service_stage`, `service_cg` and `service_runs` in `service`;
- `capture_base` in the `service` loop and in `fixture-check`;
- `harness_stage` and `harness_binary` in `harness`.

None of their `Refusal`s leaves the process:

- service stdio goes to `systemd-run … stdout/stderr=DEVNULL`, with the
  unit's own stderr going to the journal;
- harness output goes to `test-output`, which is deleted with the stage;
- `disposable.rs` runs `fixture-check` with `.stderr(Stdio::null())`
  (lines 49-51 and 153-160).

In the outer process an internal refusal surfaces as `E_SERVICE_OR_TEST`,
which F0 currently collapses to `E_RUNTIME`.

**Source inference, not runtime-verified.** In `4b4775f`, only `protected`
raises `E_PROTECTION`. The only text that reaches the job log is the outer
`run` process's top-level line, so the historical escaping `E_PROTECTION`
came from one of the six outer sites: `cg_preflight`, `run_parent` and
`run_artifact` in the body, or `cleanup_capture`, `cleanup_base` and
`run_cleanup` in `finally`. F0 instruments all six. This narrows where the
historical refusal arose. It states **no hypothesis about which site or why**,
and it is not a reproduction.

**Disposition.**

- **(a) Not a blocker for merging F0** as a bounded local artifact/replay
  mechanism, provided SP-B1 lands. Internal failures are then labelled
  `E_SERVICE_OR_TEST` (untransported) rather than an undifferentiated
  `E_RUNTIME`. The README and evidence already disclose the gap without
  inventing triples.
- **(b) A blocker for any F2 proposal**, or any claim of complete cross-process
  diagnostic capture, that relies on locating refusals at the seven internal
  sites or on `account` or `cgroup` progress. Such a proposal must first land
  F0b below, or explicitly limit its claims to the six outer sites plus the
  `E_SERVICE_OR_TEST` label.
- **(c)** Outer mutation tokens stay "attempted boundary" lower bounds. They
  never prove that an account or cgroup mutation did or did not happen.

## Cross-process capture contract (F0b; separate one-concern PR, own review)

The smallest safe plumbing contract. It preserves no raw subprocess output, an
unchanged workflow and all owner, actor, ref and source/workflow SHA fences:

1. **Channel.** Use only fixed files inside the root-owned 0700 stage:
   `feedback-service`, `feedback-harness` and `feedback-fixture-check`. Do not
   use pipes, `systemd-run --pipe`, the journal, stdout/stderr or environment
   variables.
2. **Writer.** This runs in an internal mode's top level only, and only for
   `type(error) is Refusal`. It writes only if the process's `installed` check
   had already passed, since that check covers every stage ancestor. A
   refusal at `installed` itself writes nothing. Steps:
   1. Open the stage with `O_RDONLY|O_DIRECTORY|O_NOFOLLOW`.
   2. Require `fstat` to show a directory with uid 0 and `mode & 0o077 == 0`.
   3. Create the mode's file with
      `os.open(name, O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC, 0o600, dir_fd=fd)`.
      `O_EXCL` keeps the first write per mode.
   4. Write one validated ASCII line of at most 1024 bytes:
      `{"schema":"unix-protection-f0-internal","version":1,"mode":…,"refusal":<Refusal.record>}`.
      It holds no pid, uid, path or payload.

   Any writer exception is swallowed. The internal exit stays non-zero and the
   original failure is unchanged.
3. **Reader.** This runs in the outer `finally`, only if `unit_created`, after
   `systemctl stop` (so no writer is alive) and before any check that can
   raise or delete. It uses the same stage dir fd and checks. For each file:
   1. Open with `O_RDONLY|O_NOFOLLOW|O_NONBLOCK`.
   2. Require `fstat` to show a regular file with uid 0, mode 0600, `nlink == 1`
      and size of at most 1024 bytes.
   3. Read at most 1025 bytes. Require ASCII, exactly one trailing newline and
      no duplicate keys. Validate with the same refusal validator.
   4. Require the mode to match the filename, the phase to be `initial`, and
      the site to belong to that mode's allowlist:
      - service: `service_stage`, `service_cg`, `service_runs`, `capture_base`;
      - harness: `harness_stage`, `harness_binary`;
      - fixture-check: `capture_base`.

   The reader never raises, never prints or echoes bytes, and never changes
   exit or cleanup semantics. Any failure maps to the fixed status `malformed`.
4. **Record.** Use version 2 with one `internal` slot: `null` (no unit),
   `{"mode":m,"status":"absent"|"malformed"}` or
   `{"mode":m,"status":"refusal","refusal":{…}}`. Pick the slot by the fixed
   precedence service → fixture-check → harness, and add `internal_count`
   (0-3). The worst case of three maximal refusals stays well below 4096 bytes:
   about 2.4 KiB, extrapolated from the measured 1596 for two. Pin that with a
   test. Overflow still refuses.
5. **Progress lower bound** (replaces the dead tokens). The outer process uses
   existence-only checks through the stage dir fd and never reads contents:
   - `progress-cgroup`, an empty `O_EXCL` file the service creates immediately
     before `control.mkdir`, gives `cgroup`;
   - an existing `fixture-identity` gives `account`.

   The order is `none < staged < service < cgroup < account`. Rename the field
   `mutation_lower_bound`, because absence never proves no mutation.
6. **Synthetic tests and mutants**, using mocked fds and no system paths:
   - the writer refuses an untrusted stage and a failed fstat, and keeps the
     first write via `O_EXCL`;
   - the reader rejects a link, wrong owner, mode or nlink, oversize input,
     non-ASCII input, multiple lines, duplicate keys, a wrong mode or site, and
     a final phase;
   - hostile file bytes never appear in output;
   - reader exceptions change neither the exit code nor the cleanup result;
   - the maximal record stays within 4 KiB;
   - compiled kills for `writer_ignores_trust`, `reader_follows_link`,
     `reader_unbounded` and `internal_site_unchecked`.

   No hosted or native run is part of F0b.

F0b authorizes no observation. ADR 0019 D7's no-retry restriction and the
proposal's F2 conditions remain fully in force.

## Gates (unchanged)

- No real captured record exists, and the real-record replay gate is
  **OPEN**. Debug Phase 1 is incomplete: there is no red-capable loop on the
  real symptom, and no hypothesis, historical reproduction or fix is claimed.
  The historical replay was not called.
- A new source SHA does not discharge D7. F2 authorizes no dispatch. Failure
  counters are not reset.
- Real-Runner PA6 is **OPEN** and M2 is incomplete. Native, installed and
  dedicated paths are **unverified on real hardware**.
- Publication waits on parent (PR69) merge, re-pin onto current main, a
  source PASS after ST-B1, SP-B1 and SP-B2 are fixed and rechecked, and the
  normal required checks. This review is evidence only. It confers no run,
  dispatch or merge authority.

## Summary

Standards: 1 blocker and 5 advisories. The worst is ST-B1: 14 of 15 mutants
of the new run/entry/replay behaviour survive. Spec: 2 blockers and 4
advisories. The worst is SP-B1: collapsing fixed codes to `E_RUNTIME` defeats
first/final separation and regresses below the historical diagnostic floor.
The capture gap is not an F0 merge blocker after SP-B1, but it is an F2
blocker, and the F0b contract above is the remedy.
