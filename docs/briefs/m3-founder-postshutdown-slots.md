# M3 founder post-shutdown slots: prepare-only preregistration

Sol 6.1 / high / design sub-agent / 2026-11-18; no children.
Base: `6e1b30c2b1dfb88ea104e332b2599d85442a9e66` in the assigned isolated worktree.
`D:/Code/Arxcovo` is the repository origin location, not this worktree's HEAD.
Artifact: [unapplied patch](../evidence/m3-founder-postshutdown-slots.patch).
Patch SHA-256: `fdc45d75c0e24bc1e0bc74c9182944fd718e89880b68c20c7b86cedc3545d7fd`.
Only this brief and that patch are committed. No active source was changed.
This is a future API/implementation proposal, not accepted production design.
AGENTS/KICKOFF read; Engineering/Debug guidance restricted to preparation:
natural-red evidence is retained; reproduction, hypothesis and runtime phases
are deferred by explicit owner scope. No deterministic reproduction is claimed.

## Evidence and question

Read [Astra disposition](../reviews/m3-pr178-founder-deadline-astra62-disposition.md)
(131 lines; Astra review a60df7dd) and
[retained Windows evidence](../evidence/m3-pr178-windows-founder-deadline.md).
PR178 published head `363d5320684505c15b2e3284b7c6f5a4f9ea842b` stays held.
Windows required anchored_preproof_single_voter selected WaitElapsed with
proposal pending, 16/SubmissionReply Ok, and 17/18 absent at their return loads.
Leaderwait occupied 4.554767s; proposal entered with 344.675ms remaining.
Engine receipt/commit/apply/outcome and underlying cause remain unknown.
Ubuntu/macOS each have three identity PASS records; Windows identity three unrun.
No raw logs re-read or rehashed here; those facts are retained evidence.
Question: after that refusal, does the owner later observe an engine success?

## Proposed wiring and ownership

Future allowlist is exactly crates/arxd/src/cluster/consensus/found.rs, within
its existing #[cfg(test)] observation module, and anchored_join_tests.rs, which
is a test-only module. No engine/propose writer, lookup, drop, assertion, clock,
deadline, timeout, filter, setting, concurrency, quarantine or dependency changes.
Snapshot gains private Option<Arc<Slots>> populated only for ProofInitial Err.
It retains existing 48 AtomicU64 words and the immutable scalar Instant origin.
Slots has no custom Drop, state/store/engine Arc, task, callback, permit or key.
No extra allocation, business clock sample or worker write is introduced.
The recorder allocation alone lives longer on refusal. This is the intended
scalar-only ArcSlots interpretation; it retains no live engine resources.
Snapshot's original words/events/markers/decision/result stay immutable.
On ProofInitial Ok the option is None: no Arc clone or new reads; founder leaf
moves exactly as before. QueryInitial, F10Final and F6 retain no new handle.
Their capture callers and emissions are unchanged, including F6 task-local rules.
Existing scalar Slots and original task-local/worker Arc drops remain in place;
only the optional refusal recorder survives. Snapshot has no custom destructor.

Only the proof's existing refusal emission changes to emit_proof_refusal.
Its location remains after router clear/drop, bounded shutdown and both existing
weak checks; the saved original founder panic remains immediately afterward.
Private postshutdown_cut performs exactly one Acquire load of 17/EngineReply,
then one of 18/OwnerSend. No loop, wait, poll, engine query, state read, full freeze,
clock sample or new ownership enters that accessor. Formatting does not re-read.
This is sequential once, not an atomic view or an engine shutdown timestamp.
Reads occur even if shutdown returns Err or times out. No cleanup unwrap added.
The unchanged original format_snapshot is emitted first, with capture_return label;
then aftershutdown sequential=1 and two packed words, absence/saturation flags.
Both cuts share ONE existing Sink<8192> and ONE finish/emission: total <=8192
bytes per victim including newline/truncation footer. Static ASCII/enums/integers
only; no keys, sessions, command contents or error payloads. No new output sink.
Missing handle emits present=false. All cleanup statuses must be Ok to label
cleanup_eligible=true; interpretation stays Unknown even then. Eligibility is
not a success result. Any non-Ok cleanup forbids crediting a successful outcome.
Truncation/missing rows are unresolved evidence; do not expand the cap to recover.

## Frozen prediction and one future carrier

Astra's prediction is adopted unchanged: on a matching ProofInitial WaitElapsed
red, proposal pending, 16/Ok, empty 17/18 at return, later 17/Ok + 18/Pending after
clean shutdown supports eventual owner success across these two loads. It dates
neither engine success nor commit relative to the original deadline. OwnerSend
Pending records send intent, not receipt by caller. Shutdown may affect outcome.
Forward, ReplyError or ReceiverClosed at 17 refutes the narrow success prediction
for that call. Still absent 17/18 supplies no successful-owner evidence. Mixed,
saturated, invalid, incomplete or unclean cuts leave the prediction unresolved.
Green means no reproduction; another branch is a different result. Unrelated red
is unresolved. Reproduction rate is unknown; this diagnostic grants no fix credit.
Future carrier, ONLY after separate grants: one new reviewed diagnostic revision,
existing normal all-OS full-suite CI, first attempt only, unchanged proof assertion.
No rerun of failed head, narrowed selection, second cell, manual dispatch, timing
adjustment, quarantine change, local Windows engine, retry or repair is authorized.
Stop after first attempt in every outcome, including compile failure or missing rows.

## Prepared tests and future review plan (DO NOT EXECUTE here)

Patch includes pure recorder/cut/formatter tests; no Raft/store/engine fixture.
1. Pair test writes scalars after an original freeze, verifies immutable old zeros
   and exact new 17/18 packed values; poisons 19 to expose accidental selection.
   Exhaustive Slots destructuring with exact field types rejects hidden ownership.
2. Retention test covers each Call with Ok/Err and Weak probes; only proof Err
   retains the scalar allocation, which disappears after snapshot drop. Existing
   F6 opt-in isolation test remains unchanged. No business resource is retained.
3. Formatter test preserves original primary/cut, emits only the two new slots,
   checks clean/Failed/Timeout/Unknown shutdown eligibility, ASCII/newline/cap,
   and shared-sink truncation. Same original snapshot is compared after formatting.
Future static wiring audit must verify one refusal call after shutdown/weak checks
and before original panic, no call in success branch, and exactly loads 17/18.
Reject synthetic wiring mutants: moving call before shutdown, calling on Ok,
changing either load index, changing original snapshot words or bypassing sink.
Compare source before/after: original Slots/Handle/writers/capture/F6 and old
formatter byte-equivalent; original snapshot initialization changes only by field;
proof differs only at refusal emit. Compare non-test source after erasing the
whole existing cfg(test) observer and existing test module: byte-equivalent.
Recheck cfg scope and resource drop ordering independently; textual checks do not
prove MIR equivalence, compile validity or timing neutrality. No tests executed.

## Preparation checks, remaining grants and PROGRESS proposal

Performed: git apply --check --whitespace=error-all against the base; in-memory
source comparison/scope inspection; UTF-8 LF/whitespace checks; tracked crates
unchanged. No Rust/Cargo/fmt/test/engine/network/provision/cache-delete action.
Exact artifact hash identifies this candidate; compile and tests remain unverified.
Separate grants required for actual source application, compile admission,
independent Astra exact-diff review, and one first-attempt hosted CI observation.
No execution permission is implicit in this artifact or future test plan.
Risks: diagnostic overhead/longer scalar allocation lifetime may affect a rare red;
post-shutdown sampling cannot reconstruct deadline-time engine state; pair loads
can race independently; cleanup may itself change the reply. API/format choices
are proposed, and need independent review before acceptance. No production API.
PROGRESS proposal ONLY (file unchanged): Sol6.1/high prepared unapplied two-slot
candidate; static apply/encoding checks only, source/compile/Astra/CI grants pending.
PR178 held; last merged PR176 unchanged; Sol3/3 exhausted, historical Opus0/2
unchanged. M2 open/M3 unchecked. Hardware **unverified on real hardware**.
No PR/public writes/watch/admin/VM work; babysitter and exhausted stop rules intact.
