# ADR 0016 E1 independent implementation review

Reviewer: Anthropic / claude-opus-5-5. The dispatch named no effort level;
AGENTS.md routes Opus reviews at high. This is a single-agent review with no
subagents, as the dispatch required. Both axes were inspected directly and are
reported separately. It is the first full other-provider review of the
Sol-built E1 code and of Sol's ADR 0016 corrections. It is not a review of
Opus's own original ADR 0016 architecture, which Sol reviewed in
`0016-windows-install-contract-review.md`.

## Pins

- Reviewed head: `31b939c6266216190eb3dd9c6dd39a77765be226`. I verified it
  before starting. The isolated checkout's branch is named `t3/bf1fa445`, not
  `review/windows-install-contract`, but its HEAD SHA is the requested one.
- Runtime base: public reviewed service interface
  `5c3c01e3ee11d8b97ebe3dd249f8d9bc88300b7b`.
- Original E1 range:
  `ce300603f5a70c0f0375498a98e4a46694b0c01c..ff5a1980815c6b4c09a4c85548fe6d171b81eb9e`
  (`c3332b7`, `ff5a198`).
- Scope checks (all executed):
  - `git diff 5c3c01e HEAD` outside `docs/` and the four `owner_windows*` and
    `owner_scm*` source and test files is empty. Runtime outside those files
    equals 5c.
  - The four files at `ce30060` equal those at `5c3c01e`.
  - The diff of `owner_windows.rs`, `owner_scm.rs` and `owner_scm_tests.rs`
    from 5c to HEAD is byte-identical to the E1 range diff (same md5).
  - The only code change after `ff5a198` is the additive
    `overlapping_private_chains_allow_publication_and_pin_directories` test
    (+47 lines, `31b939c`).
- Docs read in full: AGENTS.md, KICKOFF, ADR 0016, the build brief, the three
  0016 review reports, the E1 evidence, and the 5c→HEAD diffs of PROGRESS,
  `service-foundation.md` and `service-install.md`.

## Standards

Sources: AGENTS.md, KICKOFF working style and the Fowler smell baseline.

- **ST-1 (low): a token change has no test and no record.** AGENTS.md requires
  a meaningful test for every behavior and a record of concrete deviations.
  `walk_chain` now maps every `structural()` failure to `Layout(Permissions)`
  with `.map_err(|_| Error::Permissions)`. Structural failures are reparse,
  wrong type, link count and failed `GetFileInformationByHandle`. Base
  `pin_chain` returned `Store(RecoveryRequired)` for them (`structural(&f,
  directory)?` at 5c). The change also reaches the binary chain in
  `OwnerGuard::acquire` and every ancestor. The E1 evidence does not mention it,
  and no test covers the binary path, which is production-only by design. The
  axis-independent facts are under SP-2.
- **ST-2 (low, routing): the additive test ran at the wrong effort.** PROGRESS
  records the overlap test as "Sol medium". KICKOFF routes Windows install work
  to Sol high. The change is test-only, so this is advisory.
- Smells considered and dropped because the repo contract wins:
  - Duplicated Code: the eight-line pin, restore, lock and store prologue
    repeats in `open` and `uninstall`. Brief S1 requires two literal production
    call sites, so the duplication is intentional.
  - Speculative Generality: `uninstall_flow` is dead in production
    (`cfg_attr(not(test), allow(dead_code))`). The brief mandates it as PR-D's
    seam. The `allow` should go when PR-D wires it.
  - Nothing else applies. Conventional commits are used.
  `cargo fmt --check`, `git diff --check` and clippy (locked, all targets,
  warnings denied) are clean.

## Spec

Spec: ADR 0016 decision 8, the brief's PR-E1 section, U1–U3 and S1, and the
recheck's R1/R2.

### Critical checks

| Check | Result | Evidence |
| --- | --- | --- |
| `open` and `uninstall` pin every private chain before restore, lock, staging, record and right access | yes | `owner_windows.rs` `open` and `uninstall`: `pins.extend(pin_private_dirs(layout.private_dirs(), &policy, layout::check_windows_ancestor)?)` comes before `enable_restore()?`, before the `owner.lock` open and before `with_lock`, which runs cleanup first. Mutations caught: uninstall restore moved before pins, a subset of private dirs, an accepting ancestor checker (all S1). |
| Production policy and checker are used; no runtime selector | yes | `ProtectionPolicy::production()`. The fixture field is `#[cfg(test)]`, and S1 forbids `cfg` and `return` before restore. |
| Missing path refuses and is never skipped or created | yes, as `Store(Io)` | `raw_open(OPEN_EXISTING)` maps to `Failure::Io`. This matches the brief ("existing I/O mapping") and U3. See SP-3 about the wording in the dispatch. Skip-missing mutation caught by U3 (and N3). |
| Unsafe leaf, untrusted ancestor, wrong type or reparse → `Layout(Permissions)` (R2) | yes | U3 asserts the token for four cases on both paths. Leaf-policy-disabled mutation caught by U3. |
| Pins live through the transaction; lock released last | yes by inspection | `WindowsOwnerStore` field order is `staged`, …, `_pins`, `_lock`. Declaration-order drop releases the lock last. Session is `store` then the ZST `right`. S1 binds the field order (lock-before-pins mutation caught). Behavior: rename refusal and second-lock Busy during the fake-right callback, and success after drop. The production hand-off of `pins` into the store is not bound (SP-1). |
| `uninstall` needs an unforgeable `Deregistered` before any store or right access | yes | `Deregistered(())` has a private field and no Clone or Copy, and is `#[must_use]`. It is built only inside `owner_scm::deregister`. `WindowsScm::new` is private, and only `system` (NativeScm) is `pub(super)`. `fixture_deregistered` is `#[cfg(test)]` and goes through the absent fake. Removing the proof parameter would break test compilation of `uninstall_locked` callers (by inspection; not executed). |
| A failed deregistration never reaches the closure | yes | `Ok(then(scm.deregister()?))`. U1 injects failures at every call, malformed replies, sticky states and a foreign image. The mutation substituting a proof on error was caught by U1. |
| Recover, then `uninstall_right` only | yes | `uninstall_locked` contains exactly those two calls. The sole non-transactions `uninstall_right(` call is there. Skip-recover mutation caught by U2 and S1. |
| The API cannot hand out raw grant or revoke | yes | `parts()` is removed. `WindowsOwnerStore` and `WindowsBatchRight` are private, as are the session fields. Public surface: `read_config`, `observe_batch` (opaque `BatchObservation`), `configure`, `uninstall`. `owner_scm*` references no store, right or transactions items (grep rc=1). |
| No E2, CLI or native activation | yes | No `owner_install_windows*` exists. Nothing outside `owner_windows.rs` references `OwnerGuard` or `WindowsOwnerSession`. Runtime outside the four files equals 5c. |

### Findings

- **SP-1 (low, advisory; recommended before publication).** The production
  pin hand-off in `open` is not test-bound. Mutation: in `open`, replace
  `with_lock(root, layout, policy, pins, lock)` with
  `with_lock(root, layout, policy, Vec::new(), lock)`. Then every private and
  binary pin drops when `open` returns, before any later `configure` or
  `read_config`. The focused suite still passed 21/21 (exit 0). S1 binds the
  call, its arguments and its order, but not that the same `pins` reach the
  store. Current code is correct by inspection. The fix is one S1
  `sequence(body, "WindowsOwnerStore::with_lock(root, layout, policy, pins, lock)")`
  per body, plus a check that `pins` is not rebound between pin and store.
  `uninstall` keeps `pins` alive to the end of the function either way.
- **SP-2 (low): mapping drift.** The brief and the R2 disposition say to
  "preserve the original `pin_chain` error layers". E1 changes the structural
  layer from `Store(RecoveryRequired)` to `Layout(Permissions)` for the binary
  guard, all ancestors and private leaves. The brief's token table already
  lists reparse as Permissions, so the code now matches the table rather than
  the base. Both tokens refuse, so safety is unaffected. Record it in the
  evidence or the ADR deviation table, or restore the base mapping for
  `pin_chain`. Either is acceptable. Leaving it unrecorded is not.
- **SP-3 (info): the missing-path token.** The dispatch's phrase "missing paths
  refuse Layout(Permissions) as contract" does not match the settled brief,
  which says "Missing paths refuse with the existing I/O mapping". The code and
  U3 return `Store(Io)`, which follows the brief. If the coordinator intends
  `Layout(Permissions)`, that is a contract change for Opus/Sol, not an E1
  defect.
- **SP-4 (info): what U3 and S1 actually protect.**
  - U3 runs the shared `pin_private_dirs` and `walk_chain` through
    `private_fixture` and `WindowsOwnerStore::locked`, with the same order as
    production (pins, lock, `with_lock` cleanup). It does not run `open` or
    `uninstall`, which is impossible without the production guard. Production
    binding rests on S1's token-level structure, as R1 accepted.
  - The S1 lexer withstood comment, string and conditional decoys in my
    mutations. It is structural, not semantic. SP-1 is the gap I found.
  - U3's "replaced" case swaps a file in for the directory. A same-type
    directory swapped before pinning cannot be detected without an expected
    identity, and the ADR asks for none. The protection is the post-pin rename
    refusal, which the safe control and the overlap test show.
  - The "untrusted" case uses a fixture checker that rejects every ancestor.
    It does not exercise `check_windows_ancestor` logic.
- **New overlap test.** It retains the owned root in both leaf (R|W share) and
  ancestor (R share) roles, plus the `config` leaf. Record and config
  publication inside `config` succeed. Renaming `config` refuses while pinned
  and succeeds after drop. This matches production geometry, where the base is
  both a leaf and the ancestor of `config` and `state`. It caught the
  ancestor-pins-dropped mutation. The leaf-without-write-share mutation broke
  it and 10 other tests.
- **Docs.**
  - The `service-foundation.md` reading guide and E1 status, and
    `service-install.md`, label current versus historical status truthfully.
    Installed, LSA and SCM paths stay **unverified on real hardware**, with no
    M2 claim.
  - Spot-checked against code: console exit code 10 (`NotServiceManager`);
    SCM `register` leaves demand start, `start` selects auto plus delayed, and
    `stop` returns to demand only after `stopped()`; macOS `owner_query`
    `Unknown` refusal.
  - Not verified, because GitHub was out of scope: PROGRESS claims about
    hosted gates, the four exact-head reviews, merged PR numbers and the run
    queue.
  - ADR 0016 status text still describes the correction as needing Opus
    review. This report and the two prior Opus reports now supply that.

## Executed verification (ordinary user, owned temp)

The caller's administrator-role check returned `False`. A grep found no test
reference to `OwnerGuard::acquire`, `Layout::system`, `WindowsScm::system`,
`NativeScm::new`, `WindowsOwnerSession::open` or `uninstall`, or
`enable_restore`, apart from S1's string literal.

- `cargo fmt --all -- --check`: pass.
- `git diff --check 5c3c01e HEAD`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass.
- `cargo nextest run --workspace --locked`: **227 passed, 2 skipped**, run
  `535ebf4f-9816-4abc-8860-2a94e70a7805`, 10.479 s. This is the first full run
  at `31b939c` (226 plus the overlap test). The two skipped are direct-only
  ignored fixtures exercised through their parents; none was forced.
- `cargo nextest run --locked -p arxd -E 'test(owner_windows) | test(u1_uninstall)'`:
  21/21.

### Source mutations

Each mutation was applied by a scratch script, then the focused 21-test set
ran. Both source files were restored with `git checkout`, and `git status` was
clean afterwards.

| Mutation | Result |
| --- | --- |
| `open` passes `Vec::new()` instead of `pins` to `with_lock` | **not caught**: 21/21 pass (SP-1) |
| `uninstall` enables restore before the private pin phase | S1 fails |
| `pin_private_dirs` skips missing paths | U3 and N3 fail |
| Private leaf policy replaced by `Ok(())` | U3 fails |
| Only leaf handles retained (`.pop()`) | overlap test fails |
| Leaf pin without `FILE_SHARE_WRITE` | 11 fail (N1–N4, N7, N8, U2, overlap, …) |
| `uninstall_locked` skips `recover` | U2 and S1 fail |
| `uninstall_flow` substitutes a proof on error | U1 fails |
| `_lock` declared before `_pins` | S1 fails |
| `open` pins `private_dirs().iter().skip(1)` | S1 fails |
| `open` uses an accepting ancestor checker | S1 fails |

## Limits

- No production constructor, real SCM (including read-only), LSA, privilege,
  account, service manager, admin, VM, or old Lab Manager call ran.
- No push, PR, GitHub write, approval, watch or dispatch occurred. No
  `agent-review` status was posted. This is an unpublished local candidate.
- Elevated-token behavior is **unverified on real hardware**: SYSTEM-owned
  private directories under real `ProgramData`, pin sharing with restore
  enabled, and the binary-directory policy. M2 remains incomplete.
- This review is reversible documentation and grants no merge or activation.

## Disposition

No blockers. E1 implements the narrowed API, the shared pin phase, the proof
gate and recover-then-uninstall as specified. Hosted gates are still required
before any merge.

Standards: 2 findings, both low. The worst is ST-1, an untested and unrecorded
structural token change. Spec: 2 low findings and 2 informational notes. The
worst is SP-1: production pins in `open` are not bound to the store by S1, so
a mutation that drops them passes every test.
