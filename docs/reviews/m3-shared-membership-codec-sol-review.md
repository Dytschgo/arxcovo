# M3 shared membership codec: independent Sol source review

Sol 6.1 / high; separate author from builder; 2026-11-17; no children.
Result: PASS for the bounded extraction. No blocking source findings.
Publication preparation may proceed under coordinator ownership; merge gates remain.

## Target and authority

- Reviewed head: `1117ba646368640f35f81f86da00959d829cf169`.
- Source commit: `d4694d575f186541c5d6338230ea1287b3e252e9`.
- Required prepared base: `9ddc1212deef0ea641f6707b8ffb23db73a660d1`.
- Source main: `fa7e3c019484bf2335bd26fbbafd2d65b207f480`.
- Contract/root review: `e22dc933634fc7be8d391f8d749898d7578f5a4c`.
- Read AGENTS, current KICKOFF routing, CONTRIBUTING, brief and clarifiers,
  root contract review, PROGRESS and complete shared-codec evidence.
- Current owner routing supersedes CONTRIBUTING's historical cross-provider rule.
  Separate-author Sol review is valid for this non-UI work.
- Used only this assigned isolated worktree for repository source and Git reads.
  Explicitly assigned builder/root artifact directories were read as evidence.
- Initial checkout was clean at the exact reviewed head. No source was modified.
- Prepared base is the merge base of the requested comparison. Main-to-base
  differences are only PROGRESS, the brief and root contract review.

## Standards

PASS; zero blocking findings. The four source paths match the allowlist:
`cluster/membership.rs`, `cluster.rs`, `cluster/consensus.rs` and
`cluster/consensus/tests.rs`, all beneath `crates/arxd/src`.
Source commit contains only these four paths; head adds the evidence document.

The shared module imports only std. Membership and unit MembershipError are
pub(crate), with private owned fields, validated construction, immutable accessors
and consuming into_parts. There is no unchecked production constructor or mutable
escape. Clone preserves invariants. The private Reader has no storage coupling.
Members, wrapper visibility/signatures and existing test imports remain usable.

Smell baseline assessed as judgments: the short names follow the existing codec
style and do not conceal a material invariant. Collections form one domain value.
No speculative abstraction, unrelated responsibility or scattered policy emerged.
The small conversion wrappers are justified engine boundaries, not needless
Middle Men. Separate checked Reader avoids Feature Envy toward enrollment/store.
Wrapper prechecks intentionally duplicate size fences to bound native cloning;
this is justified defense at the conversion boundary, not divergent validation.
No actionable baseline smell found; tool-enforced style is not a review finding.

Advisory A1: evidence is 498 lines, with noisy escaped/mojibake log decorations.
`git diff --check base head` also reports trailing whitespace at evidence lines
276 and 325. Source-only diff check passes. These are documentation readability
nits; raw log/hash/assertion integrity remains intact. No source fix requested.

## Spec

PASS; zero blocking findings against the accepted L2-A brief and clarifiers.

Version remains 1. Encoding retains ordered 1..2 configs, each with 1..16 voters,
plus 1..16 nodes. Nonzero u64 IDs include MAX. BTree collections order IDs within
sets/table; the config Vec itself is neither sorted nor deduplicated. Identical,
reverse-order and overlapping configs remain valid. Every voter requires node
metadata, and all learner nodes and addresses survive both conversions.

Addresses retain exactly 1..200 bytes of ASCII alphanumeric or `._:[]-`.
No lowercasing, trimming or endpoint interpretation was added. Existing odd
punctuation, case, localhost and mem:1 remain valid. Empty, long, invalid UTF-8,
non-ASCII, whitespace, slash and exclamation addresses refuse as before.

Input envelope remains nonempty through 4096 bytes. Maximum canonical size is
2 + 2*(1+16*8) + 1 + 16*(8+1+200) = 3605. Longer envelope inputs cannot become
valid by padding. The decoder requires complete consumption and exact encode-back.
Strictly ascending serialized IDs reject duplicates, descending order and zero;
config duplication is a different property and is correctly preserved.

Prior decoder's late encode validation is now shared constructor validation;
accepted bytes and refusal categories remain the same. Pure errors are the unit
MembershipError; wrappers normalize all failures to StoreError::Membership.
MembershipBlob itself still returns Corrupt on empty/>4096 creation. That API
and StateStore opaque acceptance/apply behavior are unchanged.

Actual byte comparisons confirm all consensus code from `fn to_stored(` onward
is identical to base. The old tests.rs is an exact complete byte prefix, with
exactly 54 appended synchronous wrapper-test lines. Old maximum/truncation/error
bodies remain intact. Prefix SHA256:
`0ea8b1055b47d1823e09cef5f6aef81ebb4c15bcb6a05c8e0ae856aeac2e6410`.

Reviewed assertions, not merely test names: independent literal U/J/D encode and
parts/equality checks, reverse/overlap/MAX/spelling, maximum/every prefix,
constructor limits and complete malformed payloads. U/J/D lengths are 32/59/31.
Wrapper goldens pin engine conversion, learners and duplicate configs; wrapper
errors pin short reads and invalid objects. No generated expected-byte oracle.

## Security

PASS for this extraction only; zero blocking findings.

Input size is checked before parsing/allocation. Each count is checked before
collecting its bounded collection; each length is checked before take/copy.
Address validity precedes ownership allocation. Checked split_at_checked and
fixed-size conversion refuse truncation; indexing follows successful take(1).
There is no unchecked untrusted offset, multiplication or input-sized reserve.

Native config/voter/node counts and address byte lengths are checked before
cloning engine sets/strings. Native count traversal allocates nothing. Thus even
invalid engine objects cannot trigger unbounded temporary copies. Constructor
validation then enforces IDs, metadata and syntax on bounded owned parts.

No shared-module OpenRaft, enrollment Reader, DB, StoreError, async or network
coupling. No new wire/schema/command, migration, StateStore acceptance fence,
apply decoding, arxj2, enrollment/PKI or security-policy change. Existing broad
address syntax is preserved, not promoted to endpoint authentication.

## Actual review checks and reused execution evidence

Actual: source/contract/test inspection; Git scope and ancestry; exact suffix and
prefix comparisons; all 65 recorded Git blobs recomputed, including 61 frozen
entries equal to base; all 24 recorded artifact SHA256 values recomputed.
Current membership source equals saved baseline bytes, SHA256:
`6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3`.

Actual: read saved runners, selection, logs, patches and assertion values;
reconstructed all three mutant recipes to verify changed hashes and patches.
Saved listing parsed: quarantine28/sample8/founder13/TLS14 and joint ignored;
founder13/TLS14 unignored. Sample identifiers cross-checked with frozen sampler
and guard. No engine execution inferred from inventory listing.

Reused builder evidence: initial fmt pass precedes one mechanical BTreeMap import
correction; initial Clippy E0433 is retained. Corrected locked/offline workspace
all-target Clippy passes before behavior. Listed focused11 = pure7 + wrapper2 +
old pure2; all 11 pass. Guard5 and sampler60 pass. No engine test ran.

Reused once-only compiled mutants: learner drop fails literal-U encode equality;
constructor dedup fails DIRECT configs assertion [{1}] versus [{1},{1}]; admitting
! fails DIRECT constructor refusal (Ok versus Err). Each selected one test,
compiled successfully, exited100 at its designated semantic assertion and restored
the exact baseline hash. No unwrap/compiler-failure or unrelated-red credit.

Reused root final-source fmt: D:/Temp/arx-l2a-root-wr93ssym/fmt.json records
exit0, 0.875s; actual empty fmt.log hash recomputes to
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
This settles final fmt with root attribution; builder did not claim final fmt.

Reviewer ran no Rust build/test, engine test, static test suite or mutation.
One inspection script stopped printing a saved log on console encoding; the
remaining static inspection completed with explicit UTF-8, without suite reruns.

## Readiness and retained limits

Reversible refactor; reverting source restores the previous codec with no DB
migration. No architectural deviation or ADR required. No publication/PR action,
status, merge, admin, VM, install or cache deletion performed by this reviewer.
Hosted new pure tests on all OS, all five normal required gates, exact-head review
binding, fresh green main and feature Windows <=660s remain required.
Root-supplied main CI44090513191 was green: 604s wall, Windows600s; not queried here.
M2 open; M3 unchecked; hardware paths **unverified on real hardware**.
Shared Sol3/3 and Opus0/2 remain unchanged; this review grants no repair attempt.
Standards: 0 blockers, 1 advisory; Spec: 0 findings; Security: 0 findings.
