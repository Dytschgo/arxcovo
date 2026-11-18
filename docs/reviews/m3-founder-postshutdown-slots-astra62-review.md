# Independent review of the proposed founder post-shutdown observation

**Verdict.** Static review supports requesting one separately authorized hosted
observation on a reviewed new source revision. No source or execution approval is
implied. PR178 stays held. No diagnostic or runtime fix credit is earned.

Astra 6.2 / medium, single independent reviewer, 2026-11-18.
Checkout HEAD: 1418a917b7e8fe8efec0a91fa233ea5a9bd744db.
Preparation base: 6e1b30c2b1dfb88ea104e332b2599d85442a9e66.
Unapplied docs/evidence/m3-founder-postshutdown-slots.patch, SHA-256:
fdc45d75c0e24bc1e0bc74c9182944fd718e89880b68c20c7b86cedc3545d7fd.
The two target source files match between those commits.

## Standards

One advisory on the proposed formatter test. Its non-Ok cases vary shutdown
alone. Removing router, store or log from the four-way eligibility predicate
would leave those assertions green. A table varying each of the four statuses
independently would guard this interpretation if the candidate is revised.
All four comparisons are present in the reviewed patch. Pure tests also cover
capture retention across all three calls and both outcomes, release after
snapshot drop, slot selection, original data preservation and shared capacity.
No blocking standards issue found. Compile, fmt, Clippy and tests unverified.

## Spec

No blocking spec findings. The observer and anchored proof have existing
cfg(test) boundaries. The only changed line of the proof replaces the refusal
emitter after router cleanup, bounded shutdown and both weak checks, immediately
before the saved founder panic. Ok still moves the founder leaf and uses the old
emitter. No new waits, polls, spawns, engine queries, deadlines or lookups.

capture_call still freezes all return words and records its end marker before
the optional clone. Only ProofInitial Err takes a strong reference. Slots owns
48 atomic words and an Instant, with no Drop impl or back-reference. Longer
recorder lifetime cannot keep a node, permit, log or store alive. QueryInitial,
F10Final and successful ProofInitial snapshots have the extra empty field.
The layout and predicate can affect instrumentation overhead; unchanged timing
is unproven. F6 uses a separate capture with slots=None and keeps its own output.

The late accessor loads slot 17 then 18 once each using Acquire. The old return
words and formatter are untouched. Both sections use one Sink<8192>, one
finish and one eprint. The late header labels sequential loads and prints
cleanup eligibility and Unknown interpretation. On timeout or failure the cut
follows the shutdown attempt; it does not establish completed shutdown.
Missing or truncated rows supply no evidence for eventual success.

For the specified matching red, 17/Ok means the owner observed a successful
engine reply; 18/Pending records intent before tx.send. Neither proves delivery
or dates commit relative to the original deadline. Shutdown may affect the
reply. An invalid/mixed cut or unclean cleanup leaves the prediction unresolved.

## Security

No security findings. The new formatter uses fixed ASCII, booleans, packed
integers and slot indices. Original error formatting, source of worker fields
and bounded sink remain intact. Its 13-byte reserve accommodates truncation and
newline. The optional Arc exposes no new business capability or sensitive data.

## Verification and retained evidence

Read AGENTS, CONTRIBUTING, kickoff, design brief and prior Astra disposition.
Inspected call sites, task-local state, slot writer and formatter, proof
cleanup, test module bindings and proposed tests against the actual hunks.
Checked the artifact hash, clean starting status, source equality between bases
and git apply --check --whitespace=error-all. Reconstructed both proposed files
in memory. Static checks rejected six textual mutations covering early emission,
an extra emission, either wrong index, replaced return words and bypassed sink.
These checks do not measure timing or exercise Rust, Raft or hosted CI.

Reused the prior disposition for published 363d5320684505c15b2e3284b7c6f5a4f9ea842b.
Windows selected WaitElapsed after 4.554767 s in leaderwait; 16/Ok and empty
17/18 at return leave eventual owner success unknown. Ubuntu and macOS each
recorded three identity passes; all three were unrun on Windows. No raw logs
were revalidated here. Reproduction frequency and runtime fit remain unknown.

Future application, compile admission and first-attempt all-OS CI require owner
authorization. Review the actual source revision before that observation.
Stop after the first attempt on every outcome. No old-head rerun.
Sol 3/3 exhausted; historical Opus 0/2 unchanged. M2 open, M3 unchecked.
Hardware **unverified on real hardware**.

Standards: one test-coverage advisory. Spec: zero findings. Security: zero findings.
