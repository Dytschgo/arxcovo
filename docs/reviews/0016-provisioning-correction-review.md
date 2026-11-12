# ADR 0016 correction review (Sol-authored fixes)

Reviewer: Anthropic / Opus 5.5 / high, single-agent read-only review. Both axes
were inspected directly and are kept separate. This is the other-provider review
of the **Sol-authored correction only**. Independent architecture review of the
original Opus-authored ADR 0016 (`f29c0bcbb0a2597698f302ffc98e77bbd61f8447`,
base `54c43fbcc9b9a6050f067a3e437e58a018b96b00`) is credited to Sol's report
`docs/reviews/0016-windows-install-contract-review.md`, not to this review.

- Base `452fe0271f3099cefb6cff79cd1d6280acd33e26`, head
  `ff7dd1741d6452ca2127492acdc78db61486a719` (`review/windows-install-contract`).
- Commits: `199bc56` (preserve owner namespace pins), `ff7dd17` (session pin and
  lock release order). Diff: ADR 0016, the build brief, PROGRESS (+44/−14).
- Compared with pinned `54c43fb` sources: `owner_windows.rs` (`open`, `pin_chain`,
  `raw_open`, `WindowsOwnerStore` field order, `with_lock`, `cleanup`),
  `owner_windows_tests.rs` (fixture `session`), `owner_scm.rs` (`deregister`),
  `layout.rs` (`private_dirs`), ADR 0012 session preconditions and sharing rules,
  SCM brief §13.
- Nothing ran: no build, test, privilege, owner, SCM, LSA, service, account or
  system-path operation. No push, PR or GitHub action. Every installed native
  path remains **unverified on real hardware**; M2 acceptance remains open.

## Standards

No blocking findings. The correction records the deviation in the ADR table,
updates PROGRESS with provider/effort/outcome, keeps the unverified labels and
uses conventional docs commits. No baseline code smell applies to this docs-only
scope.

- **S-a (nit, wording).** The ADR status calls the review outcome "two contract
  ambiguities". Sol's P1 was blocking: a literal implementation would drop the
  private-directory pins. "Two review findings (one blocking)" would be accurate.
- **S-b (nit, stale status).** The brief header still reads "after independent
  Sol review of ADR 0016". It should also name the Opus review of the
  correction (this report) as the gate before PR-E1.

## Spec

Verified against Sol's P1/P2 and the requested checks:

| Check | Result | Evidence |
| --- | --- | --- |
| `private_dirs` pin loop before privilege, lock, staging, record and right access (configure) | yes | ADR d8 bullet 1; brief PR-E1. Matches base `open` lines 317–333 (`private_dirs` loop before `enable_restore`). |
| Same for uninstall | yes | ADR d8 bullet 2: "same private-directory pinning before steps 4–7". |
| Guard stays binary-only; fresh install keeps create-or-verify | yes | ADR d1 "binary-only part"; brief `pins: binary chain + image`; d4 unchanged. |
| Proof consumed before the uninstall store phase | yes | "takes the non-Clone proof by value first". `deregister` returns `Ok(Deregistered)` on `Absent` (base `owner_scm.rs:441–450`), so a refused uninstall can be retried. |
| Pins retained through the transaction; lock closes last | yes (contract) | Matches base `WindowsOwnerStore` field order (`_pins` before `_lock`, so pins drop first and the lock last) and ADR 0012 "lock is released last". |
| Installer-only fence leaves ADR 0012 checked cleanup and publication intact | yes | d3 scope sentence; new paragraph after d8; deviation row; I7 renamed. Base `cleanup` (`DELETE` after `PrivateFile` check) stays valid. |

Both of Sol's findings are resolved in substance. Remaining findings:

**C1 (medium, fix before PR-E1): U3 has no production seam and can pass while
the regression returns.** The brief's API section is labelled "exact", but it
provides no fixture-injectable entry point for the private-pin phase.
`uninstall_locked` starts after the store exists. Base `pin_chain` hard-codes
production `check_windows_ancestor` and `check_protection`. The policy-taking
form `pin_chain(path, leaf: Option<Policy>)` arrives only in PR-E2. Existing
tests validate the fixture parent with a test-local loop (`owner_windows_tests.rs`
`session()`), not with production code. A U3 written the same way would test
fixture code, not the loop that `open` and `uninstall` actually run, so P1 could
come back undetected. U3 also names no PR. Required: name one private-pin phase
function that both `open` and `uninstall` call, with the policy and ancestor
rule injected (the ancestor rule is skipped only under `#[cfg(test)]`, as with
`Namespace`). Assign U3 to PR-E1. For pin lifetime, define an observable check
the test asserts, for example N4-style rename refusal of a pinned private
parent during the right call, plus a structural check that the lock field drops
after the pins. "Released last" cannot be observed after the drop.

**C2 (low, fix with C1): "every existing `layout.private_dirs()` chain".**
`private_dirs` is `{base, config, state}` (`layout.rs:84`). Today a missing
directory refuses: `raw_open(OPEN_EXISTING)` maps it to `Store(Io)`. "Existing"
can be read as "skip missing ones", which would weaken base behaviour: configure
would continue with no `state` directory pinned. Say "every" chain, and state
that a missing private directory refuses as today. Add that case to U3.

**C3 (low, document): uninstall validates private chains only after SCM
deregistration.** The order is guard, then `deregister()`, then
`uninstall(guard, proof)`, then the pins. An unsafe private chain therefore
refuses after the service is already removed, leaving the grant and record
untouched. This is safe and convergent: a rerun after the owner fixes the chain
gets `Deregistered` from `Absent`. But the ADR fault model only covers install.
Add one line to the uninstall fault model describing this partial state. A
pre-deregister read-only pin is optional and not required.

No unrequested behaviour was added. No new owner authority, repair path,
approval flow or token was introduced.

## Disposition

Standards: 2 findings (nits; worst: status calls a blocking finding an
ambiguity). Spec: 3 findings (worst: C1, U3 lacks a production-path seam).
Sol's P1 and P2 are corrected. Make C1/C2 brief edits (and the C3 line) before
PR-E1 starts. They do not reopen the design. This report is reversible
documentation. It grants no merge, activation or feature-PR permission. Queue
priority (21, then 25, then 27) is unchanged.
