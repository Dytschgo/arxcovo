# ADR0018 correction 1: independent Sol recheck

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-12. Single independent
reviewer of Opus documentation; no subagents and no implementation.
**CHANGES REQUIRED.** Standards: changes required (A-ST1 remains partial).
Spec: changes required (A-SP1 remains partial). This report does not accept,
activate or implement the proposal. The coordinator must record the verdict.

## Pinned scope

- Prepared branch HEAD before this report:
  `3264613ef9576f60807ec79f222af66e52129d00`, initially clean.
- Public-main documentation checkpoint:
  `79b7879e7330e2f8de408723f15ddfac399dd75f`.
- Corrected target head: `90649cf0caa6507614e80e6e0cd48b5d4f381d23`.
- Comparison base: `5a7edf4cb56684a39de5593261796ac269c0593e`.
  Its ADR0018 and brief blobs equal original proposal
  `ed208f27f01077a88993dc88feb8f01ce306b126` byte for byte.
- Range contains one commit, `90649cf`,
  `docs(adr): correct proposed ADR 0018 for Sol review blockers`.
  Actual delta: modified ADR0018 and brief; added Opus correction report.
  The fourth scoped file, retained Sol review, is unchanged. Its bytes equal
  both original review commit `6a6faae7a43831fa58a896a4b0cc722107ef9195`
  and this prepared checkout's report.
- Read all four files at the target through pinned Git objects, the complete
  original Sol report, and the actual base/head diff. The target files were
  not substituted with prepared-root files. Only this report is written.

Contract references below are to these target objects:
[ADR0018](../adr/0018-owner-cli-wiring.md),
[brief](../briefs/owner-cli-wiring.md),
[retained review](0018-owner-cli-sol-review.md), and
[Opus dispositions](0018-owner-cli-opus-correction.md).
The first two links describe target paths; they need not exist in this
prepared tree before the coordinator integrates the proposal.

Origins: AGENTS, KICKOFF, CONTRIBUTING and the original review's accepted
ADR0009/0012/0014/0015 facts. Read accepted ADR0016 at fresh context
`780ca4164494f6c3da00a340f4c4c67e3ac9d305`.
Accepted E1 source pin is `c4c010afac0fae085fa3f870f86a00b614c6a477`;
fresh E1 context is `b79786c27ad231578325e7cd72399e1941fa4eaa`.
Accepted E2 source pin is `c73e5c7d2cc7df1f6084c25ebccb6f4434bca245`;
fresh E2 context is `780ca4164494f6c3da00a340f4c4c67e3ac9d305`.
Compared blobs: E1 `owner_windows.rs`, E2 `owner_install_windows.rs`, and
`config.rs`/`owner_plan.rs` across each accepted/fresh pair are identical.
Read fresh session/guard/install entry points, parse/revalidate/workspace and
configure-plan call sites, Windows private helper and binary usage vector.
For unchanged dispositions, retain the original source findings; no new
regression execution or fresh implementation review is claimed.

## Standards

### A-ST1 — CHANGES REQUIRED: coverage still stops before native failure branches

Original finding: "the credential failure contract lacks meaningful regression
coverage." Repository rule: "Every behavior needs a meaningful test."

New brief section 3 says "each preceded by `before(tag)?`" (line 231) and
"Any `Err`, from `before` or from the native call ... -> `Partial`."
Section 7 (lines 441 onward) injects an error at each tag and checks exact
order, Partial/no hash, private retained bytes, unchanged parent, no extra
entry, fixed stderr, empty stdout and exit 1. It adds a genuine Unix negative:
"at `Verify`, `before` creates a hard link ... and returns `Ok(())`."
These are substantial corrections. The seam is private, nonselectable in
production and requires no elevated constructor. A closure can capture the
known temp filename to create the hard link; it does not need a handle passed
by the hook. The Unix check is therefore reachable through `create_with`.

The remaining gap is material. A pre-step error returns before the actual
native operation fails. An implementation can map hook errors to Partial but
map native fchmod/write/flush/fsync/check errors to Io, or suppress native
verification failure, while passing these injected cases. The Unix hard-link
case addresses one real verification predicate only. On Windows the brief
explicitly says "The injected `Verify` fault is the required coverage there"
and makes a real deny/foreign-ACE fixture optional. Consequently, accepting a
deny ACE or ignoring a native verification rejection can survive the stated
Windows tests, despite the required deny-ACE and failed-Verify mutations.
No required fixture actually presents a deny ACE to that checker.

Specify a shared fallible operation-result path that the hook and native calls
both traverse, with tests/mutations that exercise that shared classification;
cover a write that has written a prefix before failing as well as pre-write
failure. Require an unprivileged handle-check fixture or private checker seam
for Windows rejection predicates, including the deny ACE, reachable without
NativeOps, OwnerGuard, session or elevation. This can remain within the
existing private test design; it requires no new production authority.
Mutate native-result handling and native verification, not only hook handling.

Also clarify the Mode-fault fixture: a hook before Unix fchmod leaves the
creation mode subject to umask. With a restrictive umask it can be private
without being exactly 0600, contrary to the unconditional assertion in test 9.
Use an explicit fixture assumption or distinguish privacy before Mode from
successful exact-mode verification. None of these proposed tests was run.

No separate Fowler smell or unrelated standards finding is raised. The thin
facade and private hook serve specified authority/testing boundaries. The
conventional documentation commit and reversible report scope are appropriate.

## Spec

### A-SP1 — CHANGES REQUIRED: correct hash order, incomplete wipe lifetime

Original finding: "hash before clearing the token, with one unambiguous order."
The new step 3 correctly says `digest = sha256(&token)` before JSON, borrows
that same ASCII token in serialization, and returns only the retained digest.
Test 9 uses `raw[i] = 0xA5 ^ i`, compares file token text and its hash, and
rejects hashes of raw bytes, zero bytes and ASCII zeros. Clear-before-hash and
wrong-raw-byte mutations are required. Those parts PASS as a design correction.

However, ADR decision 7 promises clearing "on every failure after the RNG."
Brief line 183 first executes `random(&mut raw)?`; only line 194 then says
"put `raw`, `token` and `json` into one owner whose `Drop` overwrites all
three ... Every return after this point ... drops it."
A permitted random closure can fill some or all of raw and return Err. The
question-mark return then skips the wipe owner entirely. The owner is also
installed after serialization rather than before secret-bearing fallible work.
The correction report's claim of clearing on every return after RNG is broader
than this exact algorithm. Best-effort clearing does not excuse skipping the
explicit buffer wipe on an ordinary error return.

Install the wipe owner around initialized raw storage before invoking RNG,
then fill/encode/serialize in its owned storage, retaining only the non-secret
digest outside. Explicitly cover partially filled RNG Err and all ordinary
post-RNG early returns, without adding a secret-output test interface. Preserve
the corrected ASCII hash order and file-text comparison. This is a bounded
lifetime correction, not a crypto redesign. No wipe or mutation test was run.

### A-SP2 — PASS: exclusive creation is the partial-result boundary

Original finding: "the exclusive-create boundary and partial errors disagree."
ADR decision 7 now says "Every failure after it is `client-config-partial`."
Brief steps 6-8 enumerate Unix fchmod, write, flush, parent fsync and handle
verification; hook and native errors must return Partial. They specify private
exclusive creation from the first byte, retained descriptors/ancestor pins,
no path reopen, no hash or stdout on failure, and no unlink, delete disposition,
delete-on-close, truncate or rename rollback. This resolves the contract
inconsistency. Coverage remains blocked separately under A-ST1; PASS here is
not implementation evidence.

### A-SP3 — PASS: old-config revalidation and refusal retained

Original finding: "full replacement claims conflict with old-config validation."
ADR decision 4 now states "The CLI parses it with the full
`ServiceConfig::parse`" and "`configure_plan` then calls `old.revalidate`
again." Brief section 4 preserves both, disallows parse_inventory/None
substitution, and adds counting-resolver Windows/Unix cases in test 5.
Old workspace missing/new workspace valid with the same pinned account must
return Workspace / `service-workspace`, exit 3, with no observe/configure or
execute mutation; the valid-old control checks the prior-byte expectation.
The manual owner remedies are restore the old workspace, or uninstall,
manually delete config, install and configure. No agent deletion command,
new authority/API or weakened library is introduced. Pinned parse and plan
sources support this refusal. No counting-resolver test was executed.

### A-A1 — PASS: retained credential claim narrowed

Original advisory: "that token is unusable" overstates the property.
ADR now says "an owner who can read it could compute its hash" and "That is
not an authorization fence." It promises no supplied registration hash or
completed-credential claim on Partial, with manual deletion/rerun as remedy.

### A-A2 — PASS: explicit strict allow-only rule, using handles

Original advisory: the helper permits deny ACEs despite "no other ACE type."
ADR decision 7 and brief step 5 require a present DACL, every ACE
ACCESS_ALLOWED_ACE_TYPE for TokenUser/SYSTEM/BUILTIN Administrators only,
and refusal of deny and all other types. The current helper's deny-skipping
behavior is explicitly distinguished. Production checks the pinned parent and
created handle, never the path-based security::private helper. The helper is
only a test oracle. The rule is clear; meaningful deny-rejection coverage is
still required under A-ST1.

## Unchanged constraints and gates

The actual diff preserves pre-Tokio owner routing, bounded argv/UTF-8/flag
rules, secret-free fixed outputs, exhaustive exit classes and code-4 guard,
fixed limits, proof-gated config-free uninstall, and inventory-only start
without workspace/build-account execution validation. Windows guard strictness
remains labelled. No new approval/product recovery flow, MCP/HTTP owner entry,
account creation, password or S4U fallback appears.

Brief section 6 explicitly makes the install-extra usage-vector change the
first S3 commit, before wiring, preserving today's usage expectation until
routing changes it. This addresses intermediate elevated-test safety. Valid
owner composition tests use RecordingOps/fakes only; no NativeOps or valid
owner process is permitted. Unmanaged service-run coverage remains.

Status stays proposed. No CLI implementation may begin before accepted
contract AND hosted-green prerequisite libraries and the other section 0
gates. Per supplied coordinator context, E1 PR40 head `190c1c7` has independent
PASS with hosted CI pending; this review did not query GitHub or verify that
live status. E2 is HELD after unexplained ExecutionFailed run
`bdbf032b-658a-4123-a279-303cb40566ed`. The prepared diagnostic checkpoint
records three instrumented 249/249 passes with no failure/cause/fix; these do
not waive the original red or establish an uninstrumented repair. N3
publication and contract next actions remain coordinator work.
ADR0019's prior PASS/inactive report is unchanged, outside this target, and
supplies no code or activation authority.

## Commands, document checks and limits

Ran only bounded Git/text inspection and report-writing/checking commands:
`git status --short`, `git rev-parse` for the pins above,
`git log --oneline 5a7edf4..90649cf`,
`git diff --stat/--name-status/--check 5a7edf4 90649cf`, the full scoped diff,
`git show <pin>:<scoped-path>` with text selections,
`git ls-tree -r --name-only` for context path discovery, `rg --files docs/reviews`,
and Get-Content for instructions/original report/diagnostic checkpoint.
Read the dytschgo-review skill and active profile. A Python subprocess/text
check decoded all four target blobs strictly as UTF-8, confirmed LF-only
bytes and resolved every relative file link against the target Git tree;
all passed. Blob equality checks above passed. Anchor IDs were not validated.
An initial line-label print hit Windows cp1252 UnicodeEncodeError after the
checks; repeated with ASCII-escaped output successfully. An initial guessed
ADR0016 filename was absent; read the actual tree-listed path successfully.
Target diff whitespace check passed. Final report UTF-8/LF, links against the
target tree, staged whitespace and single-file scope were checked before commit.

No Cargo/build/native/owner/service/SCM/LSA/account/privilege/pipe/system-path,
VM/Lab Manager, ignored fixture or private-state operation ran. No GitHub
write/read, push, PR, watch, merge, rerun, dispatch, cancellation or settings
change occurred. Proposed behavior tests and mutations were **not run**.
All native/installed/dedicated paths are **unverified on real hardware**.
This documentation review is reversible; future credential/service mutations
retain their stated partial-state limits. No milestone acceptance is granted.

Totals: Standards 1 remaining blocker (A-ST1); Spec 1 remaining blocker
(A-SP1), with A-SP2/A-SP3 and both advisories PASS. No additional independent
redesign or unrelated-code findings.
