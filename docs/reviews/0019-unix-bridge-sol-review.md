# ADR0019 Unix bridge: independent Sol review

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-12. One independent reviewer of
Opus-authored design; no subagents or implementation. **PASS for the bounded,
inactive design.** Standards: PASS. Spec: PASS, with two implementation
advisories below. This grants no native execution, activation or M2 acceptance.

## Exact scope and evidence

- Prepared base: `a9a38713ee16e905f894cff719813d68c84594a5` on
  `t3/review-m2-cli-unix-bridge`; public-main reference
  `79b7879e7330e2f8de408723f15ddfac399dd75f`.
- Target B base: `07ae3c89e1d6f845a460bc13c5d112508ff7060c`.
- Target B head: `5ed795d9d2c3f20438c5f17e89ffc9c2815428dd`.
- Range: one commit `5ed795d`, adding only ADR0019 and its implementation
  brief. Read their complete pinned contents and base-to-head diff from local
  Git objects. These files are absent from the prepared main checkout.
- Corrected API/spec comparison: ADR0017 and executor at
  `a803c361147917241111f1f726d7b7c64de3a6aa`; full implementation review
  retained there and correction recheck at
  `7758f64f1a620b21ee0d2cf385fc3a2c987b872c`. The historical full-review
  blockers are closed by that recheck; they are not new blockers here.
- Origins: AGENTS, CONTRIBUTING, KICKOFF M2/routing/privacy, ADR0007/0009,
  ADR0012/0014/0015, accepted ADR0016 at E1
  `c4c010afac0fae085fa3f870f86a00b614c6a477` and E2
  `c73e5c7d2cc7df1f6084c25ebccb6f4434bca245`. Owner libraries do not grant
  this bridge any owner execution authority.
- Source fitness: runner/config/spec validation, Unix account/pre_exec/spawn/
  observation/output/cleanup sequence, cancellation types, service Request,
  corrected dispatcher Report/Creation/assessment/output handling, native
  unavailability and existing main routing. Source inspection only.

Proposal citations below refer to the Target B head; corrected Report semantics
refer to `a803c36`. Prior review run counts are historical evidence read, not
checks performed by this reviewer.

## Standards

No blocking documented-standard breach. The proposal is one-concern design,
labels hardware uncertainty, identifies provider/effort, keeps staged activation
separate and specifies unprivileged tests and mutations. Root-only sites are
explicitly deferred, not falsely covered by fake tests. No secret/owner data or
new approval flow appears. Post-v1 features are not introduced.

Fowler judgement: fixed phase enums replace fragile error-string matching.
The runner port and test-only bridge constructor have concrete callers and
security purposes, so are not Speculative Generality. The shim delegates to a
target because that is the identity/cwd boundary, not an unjustified Middle Man.
Exhaustive platform/phase matches are required fences. No smell finding.

## Spec

No blocking inconsistency with the corrected accepted origin was found.

- **Creation:** ADR0019 D1 and brief §3 never infer no child from absent
  observation. Every listed pre-call site is NotStarted; `Command::spawn` Err
  is SpawnFailed → `Failed { Unknown }` → G/quarantine. Successful spawn
  followed by identity-read/mismatch, pipe, drain or reap failure is AfterSpawn
  → `Failed { Created }` → G. Root pre_exec can fail before the ID drop and
  still creates a child; it is conservatively quarantined. There is no
  invented ProvenNone, errno classifier or cleanup proof. Distinct Created
  versus Unknown remains meaningful evidence even where row G is the same.
- **Identity/output:** RunResult's child-observed UID/GID become Unix evidence
  at PreExec; neither comes from Pin. Corrected `assess` computes a full
  UID-and-GID match; Windows SID/token-class checks remain intact elsewhere.
  The bridge moves captured bytes and leaves the release decision to the
  dispatcher. Full identity failure withholds both content and length; a
  full match with cleanup failure still releases under H. Discovery exposes
  tools only on N. No old Metadata/after_spawn report API is reintroduced.
- **Stop facts:** withholding an incidental code on timeout/cancel/shutdown
  avoids inventing Exited. Exited without a code stays invalid K/quarantine.
  Fixed 125/126/127 and 128+signal statuses describe the observed shim; the
  target-status collisions are documented rather than claimed distinguishable.
- **Shim:** every dedicated run uses the installed fixed binary. Agent cwd and
  target/tool work happens after the build-ID drop, with no-follow relative
  cwd resolution. Only clean absolute owner-PATH entries are considered, at
  most 64, and tools are fixed-table names. No service-context PATH search,
  privileged workspace lookup, config read or shim diagnostic output is added.
  Tool-probe runs from `/`, with bounded versions/output/time. Its structured
  discovery stdout is the intentional exception to exec-child's silence.
- **Shutdown:** one Cancellation type reaches the runner from dispatch;
  caller shutdown and runner-local shutdown both participate, without sharing
  the Execution token with Runner::drop. The final recheck is after cgroup and
  socket setup immediately before spawn, with shutdown priority. No cancellation
  is observed between creation and identity read. Existing synchronous-call
  deadline limits and best-effort error cleanup remain honestly stated.
- **Bounds/config:** the agent limit remains 128 arguments / 65,536 bytes;
  fixed shim overhead is four arguments / 8,206 bytes, not extra agent input.
  Constructor derivation is lexical/pure, records narrower Runner limits as
  refusal, and does not test workspace existence. Protected directories remain
  config and state. The bounded supervisor cgroup parser does not enable or
  mutate a hierarchy. No MCP/schema/capability expansion is specified, and the
  existing stdio/bridge double-JSON response bound must remain intact.
- **Platforms/staging:** only Linux standard is a future target; macOS,
  Linux admin and Windows stay MechanismUnavailable. The process-group boundary
  cannot masquerade as mac_uid_sweep. No production constructor, CLI/config/env
  selector, S4U third attempt or password fallback appears. The D1/D3/D4/D5
  Runner changes and fixed-shim interpretation are explicitly labelled against
  ADR0007/0009 rather than silently changing accepted capabilities.

### Advisory B-A1: define legacy error compatibility precisely

ADR D1 says “`run` stays as a wrapper that maps `RunError` back to `ExecError`,
so existing callers and tests keep their meaning.” But the proposed RunError
has no text or errno, while existing ExecError has `OperatingSystem(i32)` and
multiple `Unavailable(&str)` diagnostics. A reverse mapping cannot reconstruct
the original data. Before claiming compatibility, specify the fixed lossy
mapping and whether compatibility means success/fence behaviour and signature,
or exact error values. If exact diagnostics must remain, retain them privately
inside the runner and expose only the sanitized phase to the bridge. Preserve
the legacy argument limits for legacy `run` or explicitly document their
change; do not silently give arbitrary legacy arguments the shim allowance.
Use focused pure mapping/boundary tests. No need to expand the Report API.

### Advisory B-A2: caught panic classification is not log sanitation

Brief §3 wraps `run_phased` in `catch_unwind`; §7 also prohibits account/path/
errno data in logs or panic messages. The Rust primary documentation states
that a panic hook runs before a panic is caught:
[std::panic::catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html).
Thus returning a sanitized Unknown report does not itself establish log privacy.
Keep all reachable explicit panic/expect messages fixed, never format the
payload or RunSpec/RunResult, and make T4/T8 also check captured diagnostics for
a marker-bearing panicking fake. State the hook/payload limitation explicitly
if it is not enforced in this test-only slice. Avoid temporarily swapping the
process-global hook per run, which would introduce a concurrency hazard. This
is an implementation guardrail, not a claim that an existing native panic leaks.

## Regression plan, gates and limits

T1–T9, R1/R2 and S1–S6 cover the pure derivation and phase seams, dispatch rows,
privacy, identity/output pairing, cancellation/joining, inertness, shim grammar,
cwd refusal and discovery bounds. The named mutations would meaningfully catch
unknown-to-not-invoked downgrades, cleanup invention, wrong boundaries, unsafe
PATH/cwd resolution, timeout code inconsistency and accidental activation.
Require assertions on both MCP JSON copies/raw text and list_activity/database
bytes for privacy cases; retain the existing worst-case double-JSON bound test.
Mutation plans are not evidence of native pre_exec, migration or cleanup.

Implementation can proceed only under the coordinator's existing queue and
publication gates and independent review policy. Constructor remains cfg(test).
Activation separately requires report-review closure, runner/shim/bridge
independent implementation reviews, hosted green gates, separately reviewed
cgroup setup plus janitor/rights checks, and an accepted disposable experiment.
Both unit and runs/ controller enablement are prerequisites. ProtectHome,
PrivateTmp and migration under the restricted capabilities remain open facts;
this review neither invents successful deployment facts nor resolves them.

Experiment `43091339273` failed E_PROTECTION before acceptance; neither harness
success nor cleanup is established. No retry is authorized, and no reviewed-SHA
variable is restored here. Two failed S4U attempts remain two. The two reports
are reversible docs; activation rollback must preserve reconciliation and
quarantine for unfinished work, not claim that reverting code cleans processes.

Actual work: Git/text inspection, one read of official Rust documentation, and
the two report writes/commit. No cargo/build/tests or mutations ran; none is
claimed. No process backend, native resolver/preflight, production constructor,
owner/admin, SCM/LSA, privilege/account/service, system path/pipe, VM/Lab Manager,
ignored fixture or private-state operation occurred. No GitHub operation
occurred. All installed/native/dedicated paths are **unverified on real
hardware**; M2 remains incomplete. This report does not mark either ADR accepted.

Totals: Standards 0 findings; Spec 0 blockers and 2 advisories (legacy error
compatibility and panic diagnostics). PASS is confined to the inactive design.
