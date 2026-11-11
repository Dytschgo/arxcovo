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

Disposition: Codex corrected P2's two reason cases in the ADR after this review.
P1 remains open; the architecture contract is not accepted.

P1 correction (Anthropic / claude-opus-5-5, high, architecture author, base
`68614bc`): the ADR now narrows the guarantee instead of adding a mechanism.
Decision 6 separates three fault classes. Process termination is inside the
guarantee, and so are lost LSA, registry or config writes while record writes
persist. A lost or rolled-back record write is outside it: power loss, an OS
crash, a snapshot revert or an administrator restoring the record. The
reviewer's counterexample is now the documented example. P6 is restricted to
the crash model, P6b covers LSA-only loss, and P6c pins the limit and
characterizes it as needing a stale `granted` value. The durability section,
the state invariant in decision 2, the external-change table, the limits, the
build brief and the owner guide follow the same model. A scratch exhaustive
model (depth 8) found no unsafe revoke under crash-only or LSA-only loss. Under
record rollback, every unsafe revoke read a stale `granted`; the shortest one
is the reviewer's trace. Two seeded engine mutations were caught under the
crash model. No physical NTFS or LSA power-loss behavior was tested. P2's split
is retained. This correction is not independent review, and P1 stays open
until a fresh Sol recheck accepts or rejects it.


## Independent Sol recheck of b522125

Codex / gpt-6.1-sol, medium, read the complete three-file correction from
68614bc to b522125. P1 is resolved for this documentation contract: decisions,
durability, external-change cases, test matrix and owner guide consistently
exclude record rollback from the guarantee and retain the counterexample as a
characterized limitation. P2's missing-name/different-SID split remains correct.
Standards: no new findings. Spec: no remaining blocking findings in the contract.

Executed checks: strict UTF-8/no BOM/replacement characters, balanced code
fences, scoped diff whitespace, and the supplied scratch model at depth 6.
That bounded model reports no unsafe revoke for crash-only or LSA-only loss;
record rollback reproduces the documented stale-granted failure. It uses a
three-transition rollback lookback and is not a physical NTFS/LSA proof or the
future implementation's required complete test suite. No native API, account,
right, installation or privileged operation was exercised. Acceptance here is
of the scoped contract, not of the old Journal implementation it supersedes,
the future native adapter or M2.
