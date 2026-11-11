# ADR 0012 independent review

Reviewer: Codex / gpt-6.1-sol, medium, separate from the Opus architecture author.
Scope: four-file docs delta from `39f9b0c` to `784fb16`; single-agent review.
Verdict: changes required before native implementation.

## Standards

No findings. All four changed files decode as strict UTF-8 without BOM or
replacement characters, code fences balance, and scoped git diff --check passes.
No application tests or privileged operations ran.

## Spec

1. **P1: decision 6 and the durability section overstate safety.** They allow
   loss/reordering of record publication while claiming it cannot authorize an
   unrelated revoke. Counterexample in that stated model: record `granted`
   exists; uninstall removes the right and publishes `none`; an external owner
   grants the right while `none`; power loss loses the last record rename and
   restores old `granted`; a subsequent uninstall removes the external right.
   This conflicts with the table's `none` plus external grant guarantee. The
   pure state-model assertion reproduces the contradiction; no physical NTFS or
   LSA power-loss behavior was tested. Narrow the guarantee and test model
   consistently, or specify and test how stale grant authority is invalidated.
   This review does not prescribe a new mechanism.
2. **P2: missing-account reason.** The external-change row groups deletion with
   a changed SID and says both give `account_identity_changed`. ADR 0009 maps a
   missing configured name to `account_missing`; identity-changed means the
   configured name resolves to a different SID/UID. Split these cases.

Counts: Standards 0; Spec 2, worst P1 (unsupported power-loss guarantee).
The owner record and native adapter are not implemented or accepted by this
review. Dedicated-account, installed-service and M2 acceptance remain open.
