# ADR 0018 advisory clarification: independent Opus review

Anthropic / Opus 5.5 / high, independent documentation reviewer, 2026-11-12.
Single-agent review on two separate axes; no subagents. This review covers
only the Sol medium N1–N5 clarification. It makes no claim about the
independence of the original Opus architecture, whose cross-provider credit
remains the retained Sol reviews.

Verdict: **Standards PASS** (0 blockers, 2 nits).
**Spec PASS** (0 blockers, 5 advisories).

## Pins

- Target: `dfcb643da5d66d2300471c51edc41b14cda6891a` (one commit on
  `2b805c450880fd12b9f0cd9771974e4fa1337a55`, main after #45).
- Scope diff for the ADR and brief:
  `git diff 29068df056ca7d752189a95b70c223de33e091a1 dfcb643 --
  docs/adr/0018-owner-cli-wiring.md docs/briefs/owner-cli-wiring.md`.
  29068df is not an ancestor of the target (`merge-base --is-ancestor` = 1).
  Runtime differences between the two trees are out of scope.
- Also read: the new
  [disposition](0018-advisory-clarification-disposition.md), the `docs/PROGRESS.md`
  delta from 2b805c4, and the final
  [correction-2 recheck](0018-owner-cli-second-correction-opus-recheck.md) N1–N5.
- Retained reports, blob identical at 29068df, 48757f0 and dfcb643:
  `0018-owner-cli-sol-review.md` 92e77c21,
  `0018-owner-cli-corrections-sol-recheck.md` 6c8278a3,
  `0018-owner-cli-opus-correction.md` a4656175,
  `0018-owner-cli-second-correction-opus-recheck.md` 384bdbd0 and
  `0018-owner-cli-second-correction.md` f4086ec4.
- Standards sources: AGENTS.md, CONTRIBUTING.md, docs/KICKOFF.md, and the
  Fowler smell baseline.

## Standards

Each advisory is checked for closure, implementability and consistency.

- **N1 closed.** Brief step 3 states that the three-string serialization is
  structurally infallible. An unexpected Err maps to Io, and the owner drops
  before any filesystem call. The brief forbids a fault seam. The observer
  list no longer has a serialization class; it is now Parent, Exists, Io and
  Partial, each reachable.
- **N2 closed.** `enum PostCreate` is gone. A cfg(test) order-recording enum
  is allowed.
- **N3 closed.**
  - The `ClientHandle` aliases (`std::os::fd::BorrowedFd`,
    `std::os::windows::io::BorrowedHandle`) are valid std types.
  - The cfg-gated trait methods give Write the JSON slice and Verify the
    expected length. The methods take no path and do not take ownership.
  - `NativeFileOps` is named apart from NativeOps. Its context holds only
    current-user comparison facts.
  - The Windows facts gain DACL presence.
  - These shapes work with libc 0.2.191, windows-sys 0.61.2 and Rust 1.95.
- **N4 closed in substance.** It adds three real owned-temp fixtures: a hard
  link, `fchmod` 0640 through the held descriptor, and one byte appended
  through the held descriptor. None needs privilege. They reach native Verify,
  so they are real native checks rather than injected Errs.
- **N5 closed in substance.** The bound is the compact empty-values object
  length plus 6 × the sum of the value byte lengths, computed with checked
  arithmetic before RNG.
  - The bound is sound for serde_json 1.0.145. Its worst escape is `\u00XX`
    (6 bytes) for an ASCII control byte. Non-ASCII bytes pass through
    unchanged.
  - Writing into a `Vec` with enough capacity does not reallocate.
  - The ban on post-RNG growth and on any intermediate secret String or Vec
    strengthens the single-owner buffer rule.

Smells:

- **Nit S1.** Brief line 574, "map a post-create failure at **any tag** to
  `Io`", is N2 residue. Rename it to "any post-create step".
- **Nit S2.** Brief lines 214 and 540 run past the surrounding wrap width.
  This is cosmetic; no repository rule covers it.

No other baseline smell applies. Mechanical checks passed: no CR, no
trailing whitespace, final newline present, strict UTF-8, relative links
resolve, and `git diff --check` is clean for the changed docs. The
`t3-thread://` babysitter link in PROGRESS predates this change (176af7a)
and is out of scope.

## Spec

The invariants are preserved:

- The exact ASCII token hash is unchanged.
- The wipe owner is still built before RNG, and the N5 bound is reserved
  before RNG.
- Single-owner buffers are unchanged and tightened.
- The exclusive-create Partial boundary is unchanged.
- There is still no rollback, deletion or reopen by path. The N4 fixtures
  "retain the file/names without repair or rollback".
- The fixed `arxd: client-config-partial\n`, empty stdout and exit 1 are
  unchanged.
- No valid NativeOps test or production `create` is introduced.
- No product approval flow, flag, environment selector, privilege, account,
  service or owner authority is added.

The required tests hold up:

- No new test is unreachable. N1 removes the only unreachable one.
- The 0640 fixture uses synthetic credentials under a private 0700 parent,
  so it is safe.
- Native errors stay covered through the unchanged native-result propagation
  seam.

Header and gates:

- The brief header now reads "accepted core contract; advisory clarification
  pending review". It requires a fresh Opus review of the clarification
  before publication or implementation. Earlier proposed paragraphs carry a
  historical label (brief lines 12 and 630).
- Gate facts that local Git can confirm match:
  - #9, #26, #34 and #36 are merged;
  - #39, #40, #41, #43 and #44 are merged;
  - #45 is 2b805c4.
- E2 draft #46 and its gate state are credited to the coordinator; I did not
  check them.
- All code gates stay closed.

Advisories (none blocking):

- **A1. The ADR header keeps stale review language.** The brief fixes this;
  the ADR does not.
  - Where: ADR lines 3–5 still say "It needs a fresh independent Sol review
    before any implementation." Lines 17–18 ("status is still **proposed**")
    and lines 24–27 ("Fresh Opus review of this Sol delta is required") have
    no historical label.
  - Conflict: the appended section says the next requirement is an Opus
    review of the clarification.
  - Impact: this fails closed (more gating, not less), but it misroutes the
    reviewer.
  - Fix: before publication, mirror the brief's status line, or label those
    paragraphs historical.
- **A2. The N4 fixtures conflict with the generic matrix assertions.**
  - The brief exempts only the 0640 fixture from "exact-0600" and only the
    hard-link fixture from one link.
  - A 0640 file also fails the matrix's "file remains private" and "no
    group/other permissions" assertions (line 516).
  - The length fixture holds the complete JSON plus one byte. That
    contradicts "complete JSON for … Verify errors" (line 520).
  - Fix: state each fixture's expected end state:
    - hard link: the bytes are the complete JSON, with two links;
    - 0640: the mode is exactly 0640, and the fixture is exempt from the
      private and 0600 assertions;
    - length: the bytes are the complete JSON plus the appended byte.
- **A3. The N5 test has no observation point.** The owner is internal to
  `create_with`.
  - Risk: a test of a parallel helper would duplicate the implementation and
    miss the "no capacity" mutation in `create_with`.
  - Fix: name a cfg(test) observer in the same serialization routine that
    `create_with` uses. It should report only a non-secret pointer and
    capacity (`usize`).
  - Also: `create_with` takes `address`, so run both endpoint strings on
    every CI platform. Then Unix CI also covers the backslash escaping.
- **A4. Checked overflow has no error class.** The N5 bound uses checked
  arithmetic, but the brief does not say which class an overflow returns.
  - With fixed endpoints the overflow is unreachable.
  - Fix: map it to Io before owner construction and RNG, with no fixture
    (the N1 treatment), rather than `expect` or a panic.
- **A5. PROGRESS does not pin the PR46 head.** It records that PR46 waits on
  Windows but not the head SHA.
  - The coordinator credits head ddd698e with a review PASS.
  - Fix: add the head SHA when PROGRESS is next updated.

## Validation and limits

What I ran:

- `git rev-parse`, `log`, `merge-base`, `diff`, `diff --check`, `show` and
  `ls-tree`;
- grep and sed text reads;
- an isolated `python -I` check on HEAD blobs: strict UTF-8, LF, trailing
  whitespace, and relative-link resolution. Anchors were not validated.

What I did not run or touch:

- no Rust build, test, mutation, process, pipe or native constructor;
- no owner, admin, service, SCM, LSA, privilege, account, VM, Lab Manager,
  private-state or experiment operation;
- no GitHub query or write. PR and run states are credited to the
  coordinator.

Status:

- The proposed tests and mutations are requirements, not executed evidence.
- Native, installed and dedicated paths are **unverified on real hardware**.
- M2 is incomplete.
- This PASS lets the coordinator accept and publish the docs clarification.
  It does not start implementation; every gate in brief §0 still applies.

Totals: Standards: 0 blockers, 2 nits (worst: stale "tag" wording). Spec: 0
blockers, 5 advisories (worst: A1, stale review language in the ADR header).
