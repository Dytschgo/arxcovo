# M2 F0b: independent source/security review

2026-11-14. Reviewer: Anthropic / claude-opus-5-5 / high. Single review
sub-agent; no subagents. Builder reviewed: OpenAI / gpt-6.1-sol / high.
This is a different-provider review of the Sol **implementation** and its
evidence. Opus wrote the original F0/F0b architecture, so this review earns
**no independent Opus architecture credit**. It grants no publication,
execution, dispatch, retry or merge authority.

**Standards: CHANGES REQUIRED (one blocker, two advisories).
Spec/security: CHANGES REQUIRED (one blocker, four advisories).**
The runtime source in `driver.py` meets the accepted contract. I found no
source-level security blocker. Both blockers are about tests and the
evidence ledger: one credited kill comes from fake kernel semantics, and
three contracted behaviours have no isolated test. Fixing them needs only
test and evidence changes. `driver.py` does not need to change.

## Scope, isolation and exact pins

The cwd equalled the Git top level, an isolated T3 worktree on branch
`t3/f0b-security-review`. `git status --porcelain` was empty, and HEAD was
the prepared target. Paths are repository-relative. No owner filesystem
paths or private state are retained.

| Object | Exact identity |
| --- | --- |
| Prepared HEAD (retained evidence) | `dc42a43c22b44d6f834682a1f8dbc937233691aa` |
| Original evidence commit | `5bcb86e35fcdb0cfb8ddd02843b30b6de1e2d61a` (patch-id `f59156f341ae…` equals `dc42a43`) |
| Retained source commit | `ca0b88eeee677d96c41bee665238ef4fe86c5ca3` (parent `011a86f`) |
| Original source commit | `b59410fcdabbc110f96f6f0166ccb154bafb6a74` (parent `4172fda`; patch-id `2b112e766fd1…` equals `ca0b88e`; `git diff b59410f HEAD -- tools/unix-exec-validation` is empty) |
| Current main / merge base | `a9ec0acdefcd2dc60eed34f095869a045cf8e356` (PR71) |
| Accepted proposal | `fc36a37136bd5e1bcba50ae3c644c66bb49a7dc7`; unchanged at HEAD |
| Sol design review | `9682af4` retained `7d7aa2f` (identical patch-id `d7e76a1193c9…`) |
| Opus delta recheck | `40d2d0d` retained `dbcd578` (identical patch-id `463869b687e7…`) |
| `driver.py` blob | `bb5a3004a6420379a2c7fb4cd327d840181d864e` (base `cf5a0ca6…`) |
| `test_f0b_transport.py` (new) | `9c633bded3cc4af34b8c88f2559ac216cad4c196` |
| `test_f0_blockers.py` / `test_driver.py` | `3982a55beb5521f46f372639aec724d185b97eee` / `41aa2287f6b3ec52ffe638e8365198af7110cffc` |
| Tool README | `f366c09bdeb2aa7975e372df629076da5bb2c79b` |
| Evidence `docs/evidence/unix-protection-f0b-transport.md` | `7c0005edbd270710de611965c9981c46146e5e85` |

`git diff --stat a9ec0ac HEAD` touches 12 files. Five are tool files. The
rest are docs: the F0b proposal, its reviews, the evidence, PROGRESS, and
the unrelated Windows proposal and review. No `.github/`, Rust, Cargo,
`disposable.rs`, policy or workflow path changed. In scope here: the five
tool files and the F0b evidence. Out of scope: the Windows proposal
`011a86f` and its companions `1ddd138`, `d2bd9d3` and `f1c8057`, which await
their own recheck.

Read: AGENTS, KICKOFF, the dytschgo-review and dytschgo-debug skills, the
full accepted proposal at `fc36a37`, the Sol design review, the Opus delta
recheck, the full source diff, the full new test file, the
`test_f0_blockers.py` migration, the README diff and the evidence file.

## Checks actually run

All checks ran locally under Python 3.12.10 on Windows, with `-I -B` and
mocks only. No driver CLI, native, Cargo or Rust command was run. Nothing
touched an account, service, cgroup, proc/sys, host stat, VM, owner state,
install, network, GitHub, CI or dispatch. I did not run full unittest
discovery.

| Check | Result |
| --- | --- |
| Documented safe subset (evidence command; `Guards.test_cli_wrong_platform_refuses` excluded, 64 discovered) | **63 passed**, 0 errors; printed `MEASURED internal=757 outer=2445` |
| Legacy `test_f0_blockers.mutations()` | **23 KILLED**, 0 errors; source bytes unchanged |
| F0b `test_f0b_transport.mutations()` | **33 KILLED, 4 EQUIVALENT**, 0 errors; source bytes unchanged |
| Reviewer in-memory probes (outside the repo, not committed; same exact-once/compile/no-write rules; F0b plus F0Blockers suites) | 25 extra mutants; results below |
| Reviewer brute-force maxima using driver pure functions | internal **757**; outer **2447** (see SP-A1) |
| `git status --porcelain` after every run | empty |

The runners compare a byte hash of the working file. On this checkout, which
uses LF, it is `b147b117…3551a`. The evidence records `dce176c6…`, the
builder's CRLF working-file hash. Both describe the same blob `bb5a3004`.

I did not run, retry or diagnose the main Windows ordinary-test
`ExecutionFailed`. I make no root-cause claim about it.

## Standards

**ST-B1 (blocker): the `writer_drops_excl` kill comes from fake kernel
behaviour.**

- `FakeOS.open` treats `O_CREAT` without `O_EXCL` on an existing file as
  truncation (`self.files[name] = b''`).
- Linux does not truncate without `O_TRUNC`. On a real kernel, the second
  writer would open the non-empty first record, and the writer's own
  `st_size == 0` fstat check would refuse it.
- The credited failure is the content assertion in
  `test_writer_and_first_creator`. My probe shows the traceback is the
  `assertEqual(..., first)` byte mismatch. That assertion could not go red
  under kernel semantics.
- The explicit `O_EXCL` flag assertion later in the same test is never
  reached.

This breaks the accepted rule against manufacturing kills with a fake OS,
and AGENTS "Every behavior needs a meaningful test". `O_EXCL` still has a
real, untested property: refusing a pre-existing **empty** channel left by a
dead first creator.

Fix:

1. Make the fake's non-EXCL create open the existing entry without
   truncating it.
2. Add an isolated case: a pre-existing empty, root-owned, 0600, nlink-1
   `feedback-service` must stay empty after a second writer runs.
3. Rerun the ledger and record which assertion now kills the mutant.

**ST-A1 (advisory): fake assertions are swallowed, so attribution is broad.**

`FakeOS` checks these with `assert`:

- component `O_NOFOLLOW`, `O_DIRECTORY` and `O_CLOEXEC`;
- reader `O_NOFOLLOW` and `O_NONBLOCK`;
- read size ≤ 1025;
- progress `follow_symlinks=False`.

They raise inside production `except Exception` blocks, which turn them into
"no fd", `malformed` or "no bound". As a result:

- `ancestor_nofollow_dropped` and `writer_walk_skips_ancestor` are killed by
  seven unrelated tests;
- `reader_follows_link`, `reader_drops_nonblock` and `reader_unbounded` are
  killed by tests that expect `captured`.

These are real flag-request detections, but the kills are not isolated named
cases. Record the flags and assert them in one test per guard, as
`test_writer_and_first_creator` already does for CLOEXEC and EXCL. The
`S_IFLNK`-from-`fstat` cases in `test_stage_walk_guards` and
`test_reader_metadata_guards` model a result no kernel returns for a
followed fd. Label them defensive, or model the link as `ELOOP` from `open`.
The evidence already says that flag assertions are not kernel behaviour.

**ST-A2 (advisory): Fowler baseline smells.** All are judgement calls.

- Possible Duplicated Code: `unique_pairs` repeats `replay`'s inner `pairs`.
- Possible Mysterious Name: `create_channel` also creates the empty progress
  marker.
- `validate_refusal` has no explicit `type(e) is dict` check. A non-dict
  carried refusal still fails closed through `replay`'s `TypeError` catch.
  The emitter path only ever passes dicts.
- The rewrapped stop call has a misaligned continuation line. The AST is
  unchanged, and the AST test verifies that.

## Spec/security

### Contract conformance verified in source

- **Stage walk.** `open_stage` derives the stage only from `plan` and walks
  `/` → `opt` → `arx-ci-<run>` with
  `O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC` and `dir_fd`. Every component
  must be a directory with uid 0 and no 022 bits. The stage must have no 077
  bits. Intermediate fds are closed, and the function returns `None` on any
  failure.
- **Writer guards.** The writer is reached only from `entry()`'s internal
  branch. It requires `internal_trusted`, which is set only on the line after
  `protected(Path(__file__), 'installed')`. It also requires a fixed
  `CHANNELS` key, `type(error) is Refusal`, a mode-allowlisted site and phase
  `initial`.
- **Writer encoding.** It writes canonical ASCII JSON that it validates with
  the reader's validator. The record is at most 1024 bytes and is never
  truncated.
- **Writer open sequence.** It uses `O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC`
  with mode 0600, then `fchmod 0600`. `fstat` must show a regular file, uid 0,
  nlink 1 and size 0. It writes once and checks the length. Fds close in
  nested `finally` blocks. Every `Exception` is swallowed. The entry test
  shows a raising writer leaves the exit code 1, the stderr line and stdout
  unchanged.
- **Snapshot.** The stop call is wrapped in `try/finally`, with argv and
  kwargs AST-identical. `snapshot_internal` runs after the stop returns or
  raises, and before the unchanged quarantine `require`, `fixture_cleanup`,
  `protected(stage, 'run_cleanup')` and `rmtree`.
- **Completion signal.** `snapshot_available` is true only when the stop
  returned and the cgroup `lstat` raised `FileNotFoundError`. The return code
  is ignored, and rc 5 is tested. Otherwise all three statuses are `unknown`
  and no channel is opened. The progress stats still run.
- **Snapshot totality.** The snapshot catches every `Exception` and starts
  from all-`unknown`, so a partial read never escapes. `BaseException` stays
  the disclosed no-record case.
- **Reader.** It opens with `O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC`.
  `fstat` must show a regular file, uid 0, mode exactly 0600, nlink 1 and
  `0 < size ≤ 1024`. Reads stop at 1025 bytes, and the byte count must equal
  `st_size`. The bytes must be ASCII with a single trailing newline.
  Duplicate keys are rejected, keys and types must match exactly, and the
  refusal must agree with `decide`, have an allowlisted site and phase
  `initial`. The canonical re-encoding must equal the bytes read. Bytes are
  never printed, kept or formatted into an exception.
- **Progress.** Only `stat(..., follow_symlinks=False)` on `progress-cgroup`
  and `fixture-identity` raises the bound, and only for a regular, uid-0
  entry. Contents are never opened. Statuses never move the bound (V6).
- **Marker placement.** The marker is created immediately before
  `control.mkdir`.
- **Record v2.** The emitter and `replay` accept version 2 only. The key sets
  and `STATUSES` are closed. V1–V5 and the measured bounds are implemented.
  The provenance rules are unchanged. `PRECEDENCE` selects the one carried
  payload and is documented as display order only, not chronology or cause.
- **V5.** A success line with any status other than `absent` fails
  validation. The run then prints the closed `E_RECORD` fallback and exits 1.
  The real entry/run test confirms that `first` stays null, cleanup stays
  `confirmed`, and nothing is invented.
- **Unchanged surfaces.** `decide`, `protected`, `Refusal` and the
  protection predicate are unchanged. `validate_refusal` is a semantically
  identical extraction. The workflows, Rust, the native fixture, CI and
  policy are not in the diff.
- **Limits.** The README and evidence disclaim escaped-root coverage, atomic
  consistency, immutability and complete capture. They state that
  `installed` is never captured and that `absent` is not success.
  Hardware paths are labelled unverified.

### Findings

**SP-B1 (blocker): required tests 5 and 6 are unmet for three behaviours.**
The proposal requires "each V1–V5 rule rejected by a valid-JSON line that
violates only that rule" and "the marker's failure is swallowed". My
in-memory probes found:

- **V4's carried-refusal clause is untested.** Removing
  `internal_refusal(carried, value['refusal'])` from
  `validate_internal_summary` **survives**. The real `replay` rejects a
  captured `service` carrying a `harness_binary`, `installed` or
  `final`-phase refusal (verified). The mutant accepts all three, and no
  test notices. `replay` input is untrusted, so this is a security-relevant
  validator without a test.
- **V1 has no isolated assertion.** Removing the V1 rule makes
  `test_cross_fields` **error** with a `TypeError`, which earns no kill
  credit. It never fails an assertion. A failure record with
  `mutation_lower_bound='service'` and `internal=null` is accepted by the
  mutant and rejected by the real code, but no such case exists.
- **The marker swallow is untested.** Changing `mark_cgroup` to re-raise
  **survives**. Both marker tests fail the stage walk, so `create_channel`
  returns `None` and never raises. An actual raising failure is never
  exercised. One example is a pre-existing `progress-cgroup` raising
  `FileExistsError`. So "never adds a service failure path" has no test.

Fix: add the three isolated cases and named candidate mutants
(`summary_carried_refusal_unchecked`, `v1_internal_rule_dropped`,
`marker_unswallowed`), then rerun both ledgers.

**SP-A1 (advisory): the pinned outer maximum is not the true maximum.** The
test pins 2445 bytes, built with final phase `final`. A brute force over the
two longest outer sites, both valid phase pairings, all cleanup and bound
tokens, and all 64 status combinations found **2447** bytes. That maximum
uses `first == final`, both `cleanup_capture` with phase `initial`,
`cleanup=unconfirmed`, statuses malformed/captured/malformed and
`fixture-check` carried. It is still far below 4096, so this is safe. The
contract asks for the measured true maximum, so widen the enumeration and pin
2447. The internal maximum of 757 is confirmed.

**SP-A2 (advisory): some reader and writer guards are redundant or
untested.** Report them honestly in the evidence.

- Reader guards masked by the canonical-equality or count checks. These
  mutants survive as equivalents:
  - duplicate-key rejection (`reader_dupkeys_allowed`);
  - the newline check;
  - `0 < size`.

  The tests "exercise" these checks but cannot isolate them.
- Writer checks with no case:
  - post-create `st_uid == 0`;
  - `fchmod`. Dropping it fails closed, because umask can only remove bits
    and the reader would then report `malformed`.
  - `st_size == 0`, which is equivalent under `O_EXCL`.
- Reviewer mutants that *were* killed:
  - dropping the reader-fd, walk-parent or snapshot-stage close;
  - dropping the component uid or write-bit checks;
  - treating other `lstat` errors as absent;
  - removing V3 or the V4 refusal-null rule;
  - moving the marker after `mkdir`;
  - progress following links, ignoring the uid, or being computed only when
    the snapshot is available;
  - allowing any writer phase.
- End to end, `installed` is excluded by the site allowlist as well as the
  trust flag. Forcing `internal_trusted` true at entry survives. That is
  fine as defence in depth, but the entry test does not isolate the flag.
  The unit-level `writer_ignores_trust` kill does.

**SP-A3 (advisory): scope and evidence accuracy.**

- `test_driver.py` changes by one line to reset the new global. That file is
  outside the proposal's Delivery list, yet the evidence says "only the five
  authorized … paths". The change is justified, but it needs a recorded
  disposition.
- The evidence is a new file, not an update to the existing F0 evidence
  file.
- The evidence ledger attributes `writer_drops_excl` to fake truncation
  (ST-B1).

**SP-A4 (publication condition, not a source defect).** HEAD `dc42a43`
carries the unrelated Windows commits `1ddd138`, `f1c8057`, `d2bd9d3` and
`011a86f` between main and F0b. A one-concern F0b PR must be cut from main
with only the F0b source, evidence and review commits, or the coordinator
must dispose of this explicitly. PROGRESS stays the coordinator's.

## Disposition and unchanged gates

After ST-B1 and SP-B1 are fixed in tests and evidence, with the advisories
recorded or fixed, a narrow different-provider recheck of that delta
suffices. The source review above stands unless `driver.py` changes. This
review is not `agent-review` and authorizes no merge. Publication still
needs CI, `ci-time-budget`, a different-provider `agent-review`, and the
coordinator's merge. Ordinary CI is unverified here.

F0b earns local mocked mechanism credit only. **The real-record replay gate
and real-Runner PA6 remain OPEN. M2 is incomplete. All native, installed and
dedicated paths are unverified on real hardware. F2 remains undispatched.**
ADR 0019 D7 stands. A new SHA resets no failure counter and authorizes no
historical retry.

Standards: 1 blocker, 2 advisories; worst is the fake-truncation
`writer_drops_excl` kill (ST-B1). Spec/security: 1 blocker, 4 advisories;
worst is the untested V4 carried-refusal replay validation (SP-B1).
