# Opus recheck: Sol correction of the main71 Windows feedback proposal

Disposition: **REQUEST CHANGES (narrow); no implementation or run authority.**
Anthropic / claude-opus-5-5, review sub-agent, 2026-11-14. Single reviewer,
read-only. This checks only Sol medium's correction delta against Sol high's
seven blockers. It is a different-provider check of that **correction** only.
Opus wrote the original proposal architecture (`cc44625`/`1ddd138`), so this
report gives **no independent credit** to that architecture. Its independent
review remains Sol high `43bb26d`/`d2bd9d3`.

## Exact pins

| Item | Identity |
| --- | --- |
| Review checkout HEAD, clean, cwd = Git top level | `81a8c6cbd3efec14b4ff686671b9ea163f1aff6f` |
| Checkout role | Linked worktree, own worktree Git dir under the common Git dir. Unrelated F0b source branch; used only to read Git objects |
| Target (Sol medium correction) | `011a86f951b8c3cc072464396fbe44fe94d94cfb`, parent `d2bd9d3` |
| Base (retained Sol high review) | `d2bd9d3c5140b3b540b07093bb9bae8930df5513` (tree equals `43bb26d0cd8342b9c1d9887e6bc84b2cd18b3fac`) |
| Original Opus proposal | `cc446251ce03bb1e4ab88cb911ef7e3ed61b012f`, retained `1ddd1387bb2f9140972ab8486d8d8902cdb3012c` (identical trees) |
| Target path, the only file changed in `d2bd9d3..011a86f` | `docs/reviews/windows-main71-feedback-proposal.md` |
| Proposal blob before / after | `8fe32e9acfc37b3e62a349e5d6b691a7ccae236e` / `eb7c712e8189a804e48c85e32f8a2d1da4066ff1` (same blob at review HEAD) |
| Sol high review blob | `docs/reviews/windows-main71-feedback-sol-review.md` `44daccc8a24bc96fa9ac56c9ea2180cc683e136f` |
| Source blob, failure commit `a9ec0ac` and target | `crates/arxd/src/development/windows.rs` `85f0c0eca19950fc65a02b621ff752eb6ba61551` |

I read AGENTS.md (blob `c2ae259`), the Review and Debug skills, the Sol high
review, the full revised proposal and the delta. I read the source blob only
through `git show`, and re-confirmed the 18 origins at lines 64, 89, 221, 231,
240, 286, 289, 324, 329, 414, 420, 423, 430, 448, 471, 478, 508 and 512, plus
the shared `ok` helper at 23–25. Two source facts matter below. `run_inner`
starts with a native `facts(GetCurrentProcess())` call (line 366). The file
calls Win32 directly and has no mock layer, and its existing tests state at
line 695 that they use "real child-local kernel objects rather than mocking
Win32".

## Standards

**PASS: both Sol Standards blockers are resolved. Three non-blocking notes.**

- **S1, mutation evidence: resolved.** P3A has a controlled cleanup clobber
  through the same failure-capture helper and limits the claim to helper
  order. P1 requires each of the 18 real branches to be inspected
  individually. Distinct nonzero indices are asserted by exact output, and P4
  requires each mutation class to have its own assertion.
- **S2, non-test exclusion: resolved.** P3B pins SHA, blob, target, toolchain,
  profile, features and lockfile. It inspects expansions, imports, controls,
  TLS and strings, and claims no added diagnostic logic or strings rather
  than byte equality. P1 requires non-test macros that do not evaluate their
  arguments.
- *Note a.* GetLastError and SetLastError are kernel32 calls. A "no native API
  call" pure clobber test needs either an injected last-error reader in the
  helper or an explicit carve-out for these thread-local accessors.
- *Note b.* A formatter test with distinct indices kills a constant-index
  formatter mutant. It cannot catch a call-site mutant, for example `:1072`
  passing a literal instead of the loop index. Call-site arguments stay a
  source-inspection item.
- *Note c.* P2 deleted the `arx-diag/1` field layout, but Review need 3 still
  names `handles`, `child_code` and `job_violation`. Pin the closed field list
  (missing observations 1–6) so the 384-byte proof and the privacy review
  have a fixed target.

## Spec

**REQUEST CHANGES: one blocker. Four of Sol's points resolved, one partial.**

Resolved:

- **18/28 manifest.** The manifest is independent of `Stage::ALL`, and
  count equality is dropped (P3A, Review need 5). The table still totals
  28 labels over 18 origins.
- **State ownership.** P1 has a same-thread scoped guard. Armed injection and
  captured failure are separate state, and entry clears the capture without
  disarming the injection. Nesting is rejected, and state is cleared on unwind
  and at test exit. The record is a fixed copyable value with no RefCell borrow
  across native calls, formatting or nested stages. Success leaves the capture
  empty, shared helpers carry their context explicitly, and cross-thread use
  needs a new design.
- **Context and Reason.** P2 uses a closed Context enum and keeps the actual
  Reason token. Other errors get missing fields, never stale stage data or an
  ExecutionFailed label. The worst-case 384-byte proof is tested on the
  complete line, and truncation cannot erase the stage, error or index.
  Attr-size error validity and WAIT_FAILED-only validity with raw `wait_ret`
  are in P1, along with the private-field and snapshot limits.
- **Unwrap inventory and thresholds.** The inventory and all thresholds are
  unchanged.

Partial:

- **Reachability for all 28 labels.** The requirement is now correct (P4): all
  28 are required, inputs must be deterministic, each injection must be
  consumed exactly once against the independent mapping, and stdout overflow
  cannot skip stderr. The concrete stream inputs are deferred to the later
  native contract. That is acceptable only as a precondition of any native
  authorization.

**Blocker SP1: category A presumes a Win32 mock seam that does not exist.**
P3A offers "mocked API tests" that make no native call. P3A also says
deletion/mislabel mutations are tested against the mapping, and P4 says
"mocked coverage asserts zero child-creation calls". No such tests can reach
a real `run_inner` site. Every site after `daemon_token_open` comes after
native calls, and the file deliberately avoids mocking Win32. There are two
ways to satisfy P3A as written:

- An undescribed cfg(test) shim for every Win32 call. That is a new
  architecture, it changes test-build call routing for the existing native
  tests, including the failing one, and it needs its own review.
- Pure tests that cover only the `diag` module. Then nothing in the initial
  implementation goes red on a real-site mislabel or deleted marker.

"What this does and does not buy" (lines 231–237) leaves this contradiction
unchanged. It says "the replay tests go red when a stage is mislabeled,
unlabeled or leaks private data", but P3C excludes native replay from the
initial implementation. It also promises a "Win32 error" for every stage,
while attr-size is only a last-error observation with validity.

Smallest correction (recommended): limit category A to the `diag` module
(record, state guard, formatter, capture helper, manifest self-consistency).
State that real-site label correctness, deletion/mislabel detection and
consumed-once coverage stay **unverified** until a separately authorized
native replay. Until then they rest on reviewer source inspection against the
independent manifest. Rewrite the two "buys" bullets to match. If a Win32
shim is wanted instead, propose it as a separate architecture item.

## Security and execution authority

**HOLD: one blocker. Sol security blocker 1 is resolved; blocker 2 is mostly
resolved.**

- **Authority separation: resolved.** P3 separates pure/mock, compile/artifact
  and native categories. Native replay and adding it to discovery need a
  separate scope naming SHA, commands, environment, case and child counts,
  time and cleanup limits. A new SHA or test name and past ordinary CI do not
  authorize historical, privileged or extra retries. No native replay is
  added in the initial implementation.
- **Post-create safety: resolved.** A later authorized assign, token or resume
  case must assert the child never resumes and follows the original
  error/quarantine cleanup. A missing touch marker is not proof that no child
  exists. Fences and quarantine get no bypass. P1 and the Limits section both
  withdraw "Cleanup is equivalent" and keep the assign snapshots
  non-equivalent.
- **Budget: preserved.** The 12-minute job timeout and the under-15-minute CI
  gate stay. Every case, request timeout and two-second cleanup must be
  bounded before native execution. Costs are measured only in an authorized
  run, and the failed 232-second job is not budget evidence. No label is
  deleted, no timeout raised and no skip or gate bypass added.

**Blocker SE1: zero child creation before create applies only to "mocked
coverage."** Sol required that every injection at or before `create_process`
assert zero child-creation calls. P4 line 211 limits this to mocked coverage,
which SP1 shows cannot reach these sites. No later native pre-create case has
to make the assertion. Correction: every at/before-`create_process` case,
mocked or native, asserts zero CreateProcessW calls through a test-only
counter at that call site. The counter must be inert outside `cfg(test)`.

Non-blocking:

- P1 no longer states the injection mechanics that the original P3 had. Only
  the Limits section implies them. Restate them: the armed point is checked
  before the real call, the injected API is never called, and the seam cannot
  be selected from a binary, config, CLI or MCP.
- P1/P2 change test builds of existing ordinary native tests through TLS
  reset, failure-path snapshot queries and `run_ok`. State that the success
  path adds no native calls or handles. Also state that running these tests
  falls under the future implementation PR's ordinary CI, which is neither
  replay authority nor a retry of run `43545244266`.

## Leftover text

- Status (lines 3–4) still says the proposal "needs an independent Sol high
  review". The closing line says it awaits this recheck. Reconcile them.
- The heading "Review needs (Sol high, independent)" and needs 1, 4 and 6 are
  stale. Need 4 frames TLS on nextest's process model, which P1 now rejects
  as proof. Need 6 says "the replay test", which is out of the initial scope.
  Need 3 names fields that are no longer defined.
- Table line 60 should say that `daemon_token_info` and `child_token_info`
  merge four token classes as declared granularity.
- Debug Phase 1 is correctly open (lines 5–7 and 233): no focused reproducer
  exists, the cause is unknown and the main failure is unresolved. No
  historical retry is proposed. These need no change.

## Disposition

Apply SP1 and SE1, which are text-only corrections inside the proposal, and the
leftover reconciliations. Then get a narrow recheck. This report grants no
implementation, build, test, native call, launch, CI dispatch, retry,
publishing or merge authority.

Performed: read-only Git object inspection and this file's write and commit.
Not performed: implementation, build, test, native API call, process launch,
GitHub/network/install, owner/private-state operation, subagent, or edits to
the proposal or PROGRESS. Native, installed and dedicated paths are
**unverified on real hardware**. M2 is incomplete.

Standards: 0 blockers (3 notes). Spec: 1 blocker (SP1, nonexistent mock seam
and "buys" overclaim). Security: 1 blocker (SE1, zero CreateProcessW limited to
mocks). No run authority.
