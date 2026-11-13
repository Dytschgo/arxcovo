# M2 remaining gates: independent Sol proposal review

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-14. Single-agent review of
Opus's proposal; no subagents. **Changes requested. Proposal remains unaccepted
until root disposition.** This report authorizes no implementation or run.

## Scope and identity

Cwd equals Git top level in the assigned isolated T3 worktree, outside canonical
checkout, branch `t3/m2-remaining-gates-review`. Starting HEAD:
`fed96b6536fe2bc4066279cbbf0764fa1a98eca1`; initial tracked/untracked status
was clean. Canonical location was identified from Git worktree metadata only.

Target: `docs/reviews/m2-remaining-gates-proposal.md`, original `1642726`,
ported as `4662fb0` onto merged PR66 main
`1ac40479707718a045dbe6cfcc7d9b0cdf234622`. Original and current proposal
blob both equal `c997ddb01aa79ddb470fa1b4c9be54fffe750f68`.
Main and prepared `43ee910` both have tree
`d94f461d7ef66f7c0f5c90825e478c5406d86f6a`.
Current HEAD's tree differs: only proposal and PROGRESS differ from main.
The proposal's PR66-pending/main-5ae645e statements describe its historical
authoring pin, not current status. No current hosted-check result was fetched.

Read AGENTS, KICKOFF, installed dytschgo-review and active profile; accepted
ADRs 0017/0019/0020, brief 0020 privileged acceptance, E7 PA5/PA6 review,
discovery follow-up and latest drain review/evidence, setup checklist, and
relevant bridge/readiness/config/executor/Runner/Group source. Stage F phase-1
was read via `git show 20f39c0:docs/reviews/unix-protection-phase1-evidence.md`;
its blob equals `afe5434` (`26a0500631e1f77aad1486db0d9df31af1ce9adc`).
Historical workflow, driver and disposable fixture remain unchanged: scoped
`git diff --stat 5b4f6b2 HEAD` shows only the validation README.

## Standards

**0 blockers; 2 advisories.** Repository rules take precedence over smell
judgements; no actionable Fowler smell in this design-only patch.

- **ST-A1 — Pin fresh evidence explicitly (F0).** AGENTS requires bounded,
  private-data-free evidence and truthful claims. Add fixed schema/version,
  reviewed source/workflow SHA and run/attempt association to the sanitized
  record or its equally bounded envelope. Bound predecessor count and total
  input as well as the 4 KiB output; reject overflow/malformed records rather
  than truncate into a replay claim. Test real predicate evaluation, both
  initial/final exception orderings, hostile payload suppression and bounds.
  Synthetic tests remain mechanism evidence until an actual captured refusal
  reproduces through the real predicate. No historical red replay exists.
- **ST-A2 — Retain failure accounting and source qualifications.** Add an
  explicit sentence preserving prior failure counts across S1/S2/F0 and new
  SHAs: three failed fixes require other-provider handoff with evidence;
  KICKOFF's both-provider stop condition also remains. No blanket waiver or
  reset follows from slicing work. Correct the historical current-state
  paragraph on any accepted revision; retain old test/CI credits at their
  original pins. Latest drain review closes SP1/SP2 but retains its waiting,
  autonomous-release and inspection-only seam advisories. They are not new
  hardware proof or a reason to rerun parked work.

## Spec and security

**2 blockers; 1 advisory.**

- **SP-B1 — F2 cannot declare itself “not a retry” (proposal F2, lines
  196–204).** ADR 0019 D7 says “no retry is authorized”; brief 0020 requires
  privileged acceptance designed/reviewed after diagnosis. A new SHA plus F0
  review does not discharge either restriction. Replace F2 with: “No dispatch
  is authorized. Root must disposition the no-retry restriction and evidence
  gap explicitly; any proposed observation/experiment needs its own reviewed
  scope, source pins, fresh disposable environment, technical fences and
  owner-only dispatch. Diagnostic observation is not A1–A13 acceptance.” Keep
  F0 artifact/replay before hypotheses/fixes; a fresh refusal must reproduce
  before diagnosis. Unknown cleanup permanently excludes that environment.
- **SP-B2 — U-b does not prove its claimed gate after the recommended
  latch (lines 131–134, 144–155).** `UnixBridge::preflight` checks readiness;
  immediate panic latching makes the next ordinary dispatch refuse before
  entering the slot. If the panic went through dispatch, ADR 0017 row-G
  quarantine also masks the gate. Replace U-b with separate assertions for
  ordinary early refusal and a deterministically pre-admitted request held
  before slot acquisition, then released after residue publication. Observe
  the actual gate and zero Runner calls without resetting quarantine,
  readiness or the monotone flag. Require a gate-omission mutant to fail this
  assertion. Keep U-a a direct real-Group Drop/bridge decomposition test and
  U-c a partial lexical guard; neither is real Runner/root/cgroupfs proof.
- **SP-A1 — Specify S1's live resolver closure (lines 74–80).** Pass an owned
  `Fn() -> Result<(), Unavailable> + Send + Sync + 'static` closure invoking
  `execution_preflight(&layout, &FakeResolver)` per preflight, matching
  `configured`; do not pass its eagerly computed Result or NativeResolver.
  Assert the refusing check's exact token. Missing Location still means no
  derive/factory/setup, `fail(Setup)` before handles, no daemon row, refused
  agent row and no later Ready. Public Setup mapping remains
  `MechanismUnavailable`, with existing check precedence preserved.

## Contract disposition and remaining gates

Prefer the concrete accepted D6 contract: store **and latch before slot
release**, including unwind. Recommend the proposed conditional Containment
latch in the catch arm as a separately reviewed inert implementation change;
do not change code here or silently weaken D6. Flag publication plus the next
in-slot gate can support the numerical bound under the existing AcqRel and
one-domain assumptions, but does not satisfy the accepted latch-timing text.
Keep `Failed { creation: Unknown }`, row G/cleanup_unconfirmed/quarantine,
identifier withholding and withheld output unchanged. Catching is not panic
privacy: C2 fixed payloads, P-SCAN, normal parent-owned subprocess evidence
and release-unwind activation checks still apply; never inspect/log payloads.

S1 is suitable as cfg(test)-only PA5 work after correction; production
construction, capability and discovery pins must remain. S2 earns only
PA6-local decomposition credit. **Real-Runner PA6 stays OPEN** until separately
accepted native evidence. Proposed A13 is a sensible additional Stage F gate,
pending its own design after the diagnostic gate: real run_core/Group,
fixed-literal panic, unconfirmed cleanup, flag and latch before return/slot
release and refusal afterward, with unchanged reports. No fence-bypass
constructor may enter privileged run_core. A13 is not accepted by this report.

No owner decision/action is needed merely to review S1/S2/F0. Later explicit
decisions are whether to authorize a separately bounded observation despite
the historical restriction, its fresh host/setup and reviewed scope, and the
installed M2 acceptance OS/host. The current dispatch fence additionally
requires owner actor **and triggering actor**, main ref, matching source and
workflow SHAs and the reviewed-SHA variable; only the owner may set, dispatch
and clear it after applicable gates. This is not a request or preapproval.

Native acceptance needs a fresh disposable cgroup-v2 host with separately
owned root/service/account/delegation setup and technical fences, meeting the
brief's systemd/kernel floor. Agents cannot perform owner admin steps. Dedicated
standard account, disk cap and owner CPU/RAM reserve apply to VM-host scope per
KICKOFF; they are not a substitute for Linux root/delegation setup or universal
requirements for hosted pure tests. An owner's local guest is not implicitly
exempt from “never the owner's machine.” Stage G remains separate architecture
review and required gates **after accepted native evidence**, including C2
G-P1–G-P3 and capability readiness/hardening. Production remains
`MechanismUnavailable` on every OS.

## Verification and delivery limits

Only read-only Git/source/document checks were performed before writing this
report. No builds, tests, mutation runs, native/ignored fixtures, experiments,
owner/service/account/proc/sys/cgroup/VM/private-state operations, installation,
external requests, GitHub actions or settings changes. Old Lab Manager,
`agent-windows-test` and their state were not touched. Only this report is
staged/committed; no PROGRESS, proposal or source edits. Documentary diff checks
are recorded by the report-only delivery commit; no product pass is claimed.

M2 remains incomplete; hardware paths are **unverified on real hardware**.
Privacy flake, `E_PROTECTION` and `ExecutionFailed` remain unknown: no diagnosis,
fix or retry. Standards: 0 blockers/2 advisories (evidence provenance/bounds).
Spec: 2 blockers/1 advisory (unauthorized F2 inference and masked gate proof).
