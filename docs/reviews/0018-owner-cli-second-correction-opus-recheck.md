# ADR0018 correction 2: independent Opus recheck

Reviewer: Anthropic / Opus 5.5 / high, 2026-11-12. Single reviewer, no
subagents, docs only. Scope: the Sol correction-2 delta to ADR0018 and its
brief. This review is independent of Sol's correction 2 only. It claims no
independence for the Opus original proposal or Opus correction 1; those keep
their retained Sol reviews.

**Verdict: PASS, with non-blocking advisories.**
Standards: PASS (A-ST1 closed; advisories N1–N4).
Spec: PASS (A-SP1 closed; advisory N5).
The status stays **proposed** until the coordinator records a disposition.
This report accepts, activates and implements nothing.

## Pinned scope

- Prepared HEAD: `d11dcdc58abc33f8b822923f54d8bb37fe1ccdd0`, clean at start.
- Previous proposal: `90649cf0caa6507614e80e6e0cd48b5d4f381d23`. It is not an
  ancestor of HEAD (merge-base `79b7879`), so I compared Git objects directly,
  as instructed:
  - ADR0018 blob `7581134` → `4f213bf`;
  - brief blob `118fce2` → `dc3f19a`.
  - `git diff 90649cf d11dcdc -- <both paths>`: 113 insertions, 58 deletions.
    `--check` is clean.
- Inputs read at HEAD: the retained original Sol review (blob `92e77c2`,
  the same at 90649cf), the Opus correction 1 report (`a465617`, the same at
  90649cf), the Sol recheck (`6c8278a`, the same as commit `099292f`), and the
  correction 2 disposition (`f4086ec`).
- Origins: AGENTS.md, KICKOFF routing and privacy, the original proposal and
  the original review.
- Out of scope: commit d11dcdc also edits `docs/PROGRESS.md`, and there are
  runtime differences between the ancestors. I did not review either.

## Standards

### A-ST1: CLOSED

Residual from the recheck: injected pre-step errors return before the native
operation runs. Native fchmod, write, flush, fsync or check errors could be
mapped to Io or suppressed, and Windows predicates such as the deny ACE were
never exercised.

What the delta does:

- **Brief §3, lines 171–183.** It replaces the `before` hook with a private
  `PostCreateOps` operation seam. Each method returns its own `io::Result`.
  `create_with` maps every Err to Partial through one classifier. The native
  adapter "must propagate ... unchanged" and "must not classify or suppress".
  Write must report an Err that happens after a prefix was written.
  - The classifier now sits after the real operation, not before it.
  - The adapter cannot create, reopen, replace or delete a file.
  - Nothing in production selects a test adapter.
- **§7 native-result propagation, lines 484–490.**
  - Requirement: real Err results enter through the adapter's syscall-result
    seam, for fchmod, write, flush, parent fsync and fact acquisition. "The
    production syscall and fixture result must meet at the same
    result-handling statement."
  - Effect: a mutation that swallows a native error inside the adapter is
    now detectable. The recheck's "mutate native-result handling, not only
    hook handling" is satisfied.
  - How to implement it: give each native method its raw call as a private
    parameter (for example `mode_with(fd, sys)`, where production passes
    `fchmod`), or give Write a generic writer that a test replaces with one
    that writes a prefix and then fails. No new authority is involved.
- **Prefix write.** §7, lines 470–481, tests a Write error both before any
  bytes and after a known prefix in the real owned-temp file. It asserts the
  prefix bytes, Partial and no later operation. The new mutation "ignore a
  prefix-write failure" is listed.
- **Windows predicates.**
  - §3, lines 185–191, separates handle-fact acquisition from a private pure
    checker. Synthetic facts go through the same checker and the same Verify
    classifier.
  - §7, lines 491–502, makes negative fixtures mandatory: foreign owner,
    wrong type, reparse, link count 2, wrong length, absent DACL, a deny ACE
    that is actually present, another ACE type, and a foreign allowed
    recipient. A valid control is included.
  - Skipping the checker or suppressing a rejection must fail. The A-A2
    deny-ACE mutation therefore now meets a real fixture.
  - None of these tests uses NativeOps, OwnerGuard, a session or elevation.
  - The real-GetSecurityInfo limitation is labelled correctly as a hardware
    limitation.
- **Umask.** Lines 476–478 require only no group or other permission bits
  before Mode succeeds, and exact `0600` after it. That is correct: creation
  mode is `0600 & ~umask`, and a umask can remove only owner bits.
  - The "empty bytes" check before Mode can use the handle's or metadata's
    length, so it does not need owner read access.

The unchanged design points survive: no rollback, no path reopen, the fixed
partial line with empty stdout and exit 1, and the exclusive-create boundary.

#### Non-blocking advisories (fix before S2 starts or in the disposition)

- **N1. The serialization error class is unreachable.**
  - Where: line 458 requires the wipe observer on "serialization" errors.
  - Problem: `serde_json` serializing a struct of three `&str` fields (the
    token is always hex) into a `Vec<u8>` cannot fail in practice. Testing it
    would need a fault seam the brief does not specify (Speculative
    Generality).
  - Fix: state that serialization into the owner's buffer is structurally
    infallible, map any Err to `Io` without a test, and drop it from the list.
    Alternatively, name the seam explicitly.
  - The other classes are reachable. Io, for example, comes from an RNG Err
    or a native create error such as a final name that is too long.
- **N2. The `enum PostCreate` tag is stale.**
  - Where: line 158 still declares it.
  - Problem: nothing in the new contract uses it.
  - Fix: either say it labels the recorded order of adapter calls in tests,
    or delete it.
- **N3. The adapter has no named type or method shapes.**
  - Problem: the adapter is passed in before creation, so its methods must
    take the created handle (and the parent for SyncParent) as borrowed
    arguments. Write must take the JSON slice, and Verify the expected length.
  - Fix: state these shapes. Then "no reopen" (line 481) is enforced by the
    structure: no path parameter exists, so tests can only check the parent
    entries.
  - Also give the adapter a distinct name, such as `NativeFileOps`. Tests
    must wrap it, while line 375 forbids constructing `NativeOps`. The two
    types are different, but the shared word invites confusion.
- **N4. The Unix Verify predicates are asymmetric.**
  - Problem: Unix gets only the real hard-link negative. A mutation that
    drops the Unix mode check or the length check would survive.
  - Fix: add two real fixtures. The test wrapper can create both without
    privilege through the held descriptor: `fchmod 0640` before Verify, and
    one extra byte appended before Verify. Owner and type negatives remain
    infeasible without privilege.
  - The recheck did not demand this, so it is not blocking.
- The §3 fact list (line 189) omits DACL presence, which the absent-DACL
  fixture needs. A fact type of `Option<ACE list>` is enough.

No Fowler smell beyond N1 and N2 is raised. The seam is necessary for the
specified tests, not speculative.

## Spec

### A-SP1: CLOSED

Residual from the recheck: `random(&mut raw)?` ran before the wipe owner
existed, so an Err after a partial fill skipped wiping. The owner was also
installed only after serialization.

What the delta does:

- **Brief step 3, lines 201–219.**
  - The sole owner holds initialized `raw [u8;32]`, `token [u8;64]` and
    preallocated JSON storage. It is not Clone or Debug, and it zeroes with
    volatile writes on drop. It is built after the elevation check and
    before the RNG runs.
  - RNG fills `owner.raw` in place. "If RNG fills even one byte and then
    returns Err, return through the owner's Drop." There is no filesystem
    call and no output.
  - Encoding and serialization use the owner's storage. Only the non-secret
    `[u8;32]` digest stays outside it.
  - This is implementable in Rust. Borrowing `owner.token` as `&str` while
    writing into `owner.json` is a disjoint-field borrow, which is allowed on
    a type that implements Drop. getrandom 0.3.4's `fill` takes the borrowed
    array.
- **Exact token hash.** It is preserved. `digest = sha256(&owner.token)` runs
  before any token byte is cleared. The JSON borrows the same text. Step 8
  returns `hex(digest)` only after all of step 7 succeeds. The test using
  `0xA5 ^ i` and file-text comparison, and the clear-before-hash and raw-hash
  mutations, are unchanged.
- **Tests, lines 452–459.**
  - A deterministic RNG writes a nonzero prefix and then returns Err.
  - A `cfg(test)`, boolean-only observer runs inside the shared wipe routine,
    before release. It is checked on success and on each ordinary error
    class, and it exposes no secret bytes.
  - An unwind fixture covers a partial fill: the RNG closure writes and then
    panics, and the test uses `catch_unwind`. No profile in the repo sets
    `panic = "abort"`.
  - Abort, OS copies and allocator copies are explicitly outside the
    best-effort guarantee.
  - The new mutations cover owner-after-RNG, a bypassed Drop and a skipped
    buffer.
- **ADR decision 7, lines 140–151.** It now matches the brief. Correction 2
  is attributed to Sol medium, labelled as proposed and not executed, and
  waives no gate.

### Non-blocking advisory N5

Line 218 now states "Preallocate the exact bound so serialization does not
grow it." No test detects regrowth, and regrowth would leave an unwiped
freed allocation that the observer cannot see. The allocator caveat keeps the
contract consistent, but there are two cheap options:

- assert that the JSON capacity and pointer are unchanged after serializing;
- write into a fixed `[u8; N]` with a cursor writer, which avoids the
  allocator entirely.

Either way, the bound must account for JSON escaping of the Windows pipe
address's backslashes.

### Other dispositions are preserved

- **A-SP2.** Unchanged: step 6 is the boundary, and step 7 failures map to
  Partial with no hash, no unlink, no delete disposition, no truncate, no
  rename and no path reopen.
- **A-SP3, A-A1 and A-A2.** Unchanged by the delta. A-A2 gains real
  fixture coverage.
- **Authority.** None added:
  - no flag, environment variable or feature selects an adapter;
  - no approval, recovery flow, MCP/HTTP entry or account operation is
    introduced;
  - test observers are `cfg(test)` and boolean-only;
  - no secret goes to output or logs.

## Gates and limits

- **Gates.** No CLI implementation until:
  - the contract is accepted;
  - E1 and E2 are hosted green.
- **E1.** As supplied, PR40 merged as `884a301` at `190c1c7` through all five
  gates. Main run 43187047371 was pending at the last check. I did not query
  GitHub.
- **E2.** Stays HELD on the unexplained red run `bdbf032b`. Three
  instrumented passes do not waive it.
- **Commands I ran:**
  - `git rev-parse`, `cat-file`, `log`, `merge-base`, `show`, `ls-tree`;
  - the scoped `git diff` and `diff --check`;
  - grep and sed text reads;
  - an isolated Python check on four HEAD blobs (ADR0018, the brief, the
    correction 2 report and the Sol recheck): strict UTF-8 decode, LF-only
    line endings, and relative links resolved against the HEAD tree. All
    passed. Anchors were not validated.
- **What I did not run or touch:**
  - no build, test, mutation, native, owner, SCM, LSA, account, VM, Lab
    Manager, pipe or service operation;
  - I did not read `.git/acceptance/private-state.json`;
  - no GitHub write, push, PR or watch.
- **Status of the proposed tests.** They are requirements, not evidence.
  Native, installed and dedicated paths are **unverified on real hardware**.

Totals: Standards: 0 blockers, advisories N1–N4 (worst: N1, an unreachable
required test case). Spec: 0 blockers, advisory N5 (worst: no test detects
JSON buffer regrowth).
