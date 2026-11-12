# ADR 0016 provisioning follow-up recheck (Sol-authored)

Reviewer: Anthropic / Opus 5.5 / high. A single-agent, read-only recheck that
covers only Sol's follow-up to `docs/reviews/0016-provisioning-correction-review.md`
(C1–C3, S-a, S-b). It is not a review of the original Opus architecture. Sol's
`docs/reviews/0016-windows-install-contract-review.md` covers that.

- Base `537bc55c3ace07aedb76ad5a84bfde27ad655200`, head
  `268a367775608ed4b6b668ed7b5553049326ca24` (`review/windows-install-contract`),
  linear. Commits: `c2dfd21`, `a7125e8`, `268a367`. Diff: ADR 0016, the brief
  and PROGRESS (+58/−21).
- Compared against pinned `54c43fb`: `owner_windows.rs` (`ProtectionPolicy`
  lines 85–139, `raw_open`, `pin_chain` lines 241–290, `open` lines 314–347,
  `WindowsOwnerStore` field order), `layout.rs` (`check_windows_ancestor`,
  `check_protection`) and `owner_windows_tests.rs` `session()`.
- Nothing was run: no build, Rust, privilege, SCM, LSA, owner, account,
  service, system-path, VM or Lab Manager operation. There was no push, PR or
  GitHub action. Installed native paths remain **unverified on real hardware**.

## Standards

No blocking findings.

- S-a resolved. The ADR status now reads "one blocking pinning finding and one
  fence-scope clarification".
- S-b resolved. The brief header credits both reviews and gates PR-E1 on this
  recheck.
- PROGRESS keeps `Current state` first and retains the old checkpoint as
  "Historical provisioning-base checkpoint". The follow-up entry records the
  provider, effort and outcome. It makes no M2, native or installed acceptance
  claim. The #25 SHA and time match local `63b6555` (18:59:09+01:00 = 17:59:09
  UTC). The "five required gates" claim was not verified here because GitHub
  was not queried.
- Smell baseline: not applicable. The injected ancestor checker serves a real
  test need, so it is not Speculative Generality.

## Spec

- **C2 resolved.** The ADR says "Missing directories refuse; they are never
  skipped or created by a session." The brief says "Missing paths refuse with
  the existing I/O mapping". That matches base `raw_open(OPEN_EXISTING)` →
  `Store(Io)`, and U3 includes the missing case.
- **C3 resolved.** The uninstall fault model now records SCM removal before
  the private-chain refusal, an untouched record and right, and a rerun through
  an absent service. This is consistent with `deregister` `Absent` →
  `Deregistered`.
- **C1 substantially resolved.** These parts are fixed: one shared no-follow
  walker with no second test walk, assignment to PR-E1, no runtime selector,
  observable rename and lock controls at the fake-right callback, a drop check,
  and a separate review of field/drop order. No new authority, public API,
  creation or repair path was added. Two low-severity gaps remain. Each needs a
  brief edit before PR-E1.

**R1 (low): the production call sites are still unobservable.** U3 says
"invoke the shared `pin_private_dirs` phase through the configure/uninstall
fixture paths". `open(guard)` and `uninstall(guard, proof)` require the
production-only `OwnerGuard`, and `uninstall_locked` starts after the store
exists. The "exact" API therefore defines no fixture path that runs the
production call. Suppose a `#[cfg(test)]` fixture calls `pin_private_dirs`
itself. U3 would still pass if `open` dropped the call or passed a permissive
ancestor checker, which is the original P1 regression. "Mutating away the
shared private-pin call must fail U3" can then only target the fixture's call.
Cheapest fix, with no new API: add an S1 scan item. In non-test
`owner_windows.rs`, `pin_private_dirs(` appears only in `open` and
`uninstall`, before `enable_restore` and the lock open, with the production
`ProtectionPolicy` and `layout::check_windows_ancestor`.

**R2 (low): the refusal token drifts.** The brief passes "the fixed production
`ProtectionPolicy`" as the leaf policy. `ProtectionPolicy::check` maps failures
to `Failure::RecoveryRequired`, which becomes `Store(RecoveryRequired)` (base
lines 97–131). Base `pin_chain` instead returns `Layout(Permissions)` through
`layout::check_protection`. The brief's token table still says "existing private
dir unsafe … `Layout(Permissions)`", and "N1–N10 change only their access path".
State that `pin_private_dirs` keeps the `pin_chain` mapping, and have U3 assert
the token.

## Disposition

Standards: 0 findings (S-a and S-b resolved). Spec: 2 low findings (worst: R1,
the production call site is not covered by U3 or S1). C2 and C3 are resolved
and C1 is mostly resolved. R1 and R2 are one-line brief edits within the
current design and authority. They do not reopen ADR 0016. They can be checked
when PR-E1 is reviewed. This report is reversible documentation. It grants no
merge, activation, feature-PR or privileged permission, and the queue-drain
pause stays.
