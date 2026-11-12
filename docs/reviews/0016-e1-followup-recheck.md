# ADR 0016 E1 follow-up recheck (independent, focused)

Reviewer: Anthropic / claude-opus-5-5, acting as a cross-provider review sub-agent for Sol (OpenAI) follow-up commits. The delegation target requested high effort. This report makes no claim about the runtime effort, which is not observable. One reviewer, no subagents, single-agent review. Both axes were inspected directly and are reported separately.

Scope: only the follow-up range. This is not a fresh full E1 review. The full review stays `docs/reviews/0016-e1-independent-review.md` (report commit `42c923d`, which reviewed `31b939c`; PASS, no blockers). Architecture credit for Opus's original ADR 0016 still rests on the earlier Sol review (`0016-windows-install-contract-review.md`). This report does not count as Opus reviewing its own architecture.

## Target

| item | value |
| --- | --- |
| base | `42c923d` (docs: prior independent report) |
| head | `4a5919e61542ee38507a9ba461549a61e4b3a67d` (pinned object, read via `git archive`; the shared checkout was not switched) |
| commits | `bc9cacf` fix(service): preserve owner pin errors and verify retention; `4a5919e` docs: reconcile E1 independent review follow-ups |
| diff | `owner_windows.rs` (+15/−7), `owner_windows_tests.rs` (+22), `docs/PROGRESS.md`, ADR 0016 status, `docs/briefs/windows-install-provisioning.md` |
| spec | brief PR-E1 section (error layers, U3, S1), prior findings SP-1..SP-4 / ST-1..ST-2 (the coordinator calls them A1..A4), AGENTS.md, KICKOFF routing (line 92: Sol high for Windows install) |
| original layer reference | `pin_chain` at `4fb25ec` (pre-E1) |

## Verdict

- **Standards: pass.** No blocking finding. One informational note.
- **Spec: pass.** No new blocking gap. A1 and A2 are corrected; A3 and A4 are dispositioned honestly. One low, non-blocking residual from A1 (E1-R1).

## Spec

### Error layers against the original `pin_chain` (A2)

At `4fb25ec`, `pin_chain` used `structural(&f, directory)?` with `structural -> Failure::RecoveryRequired`, which becomes `Store(RecoveryRequired)`. Leaf and ancestor policy errors used `Error`, which becomes `Layout(..)`. `raw_open` returned `Store(Io|Busy)`, and a non-fixed drive returned `Layout(Layout)`. At head:

- `walk_chain` is back to bare `structural(&f, directory)?`. The binary `pin_chain` adds no mapping, so the binary guard and its ancestors get exactly the original layers.
- `pin_private_dirs` remaps only `Store(RecoveryRequired)` to `Layout(Permissions)`. I checked every fallible step in `walk_chain`. `native::components`, `windows_protection`, `check_leaf` and `check_ancestor` return `Error`, and `raw_open` returns only `Io` or `Busy`. So `structural` is the only source of `RecoveryRequired`, and the remap does not swallow anything else. Missing paths stay `Store(Io)`, and `Busy` and `Layout(Layout)` pass through.
- The brief now says the same thing. Structural rejection is `Store(RecoveryRequired)`, private admission maps structural and policy rejection to `Layout(Permissions)`, and missing paths keep `Store(Io)`.
- The new test `shared_walk_preserves_binary_and_private_structural_error_layers` runs on an owned-temp file used as a directory leaf. It asserts the raw walk gives `Store(RecoveryRequired)` and the private wrapper gives `Layout(Permissions)`. My mutations a2 and a3 below both fail it, so it pins both layers.

### A1 (SP-1) retention binding: corrected; residual E1-R1 (low, non-blocking)

S1 now requires the exact `WindowsOwnerStore::with_lock(root, layout, policy, pins, lock)?` sequence after the lock, in both `open` and `uninstall`. Mutation a4 (uninstall passes `Vec::new()`) fails S1, which reproduces the builder's evidence for one of the two methods.

**E1-R1.** The prior recommendation had a second half: "plus a check that `pins` is not rebound between pin and store". It was not implemented. Mutation a1 inserts `pins.clear();` after `enable_restore()?;` in `open` only. It compiled, and the focused suite still passed 22/22 (run `cf877489-6380-4231-a5da-dd02442bb87a`). Every private and binary pin would then drop before the session is used. The same applies to `let pins = Vec::new();` or `drop`/shadowing patterns. The current code is correct by inspection: `pins` is used only in `extend` and in `with_lock`. Optional fix: in S1, assert that `body[pin_end..store]` contains no `pins` token. Low and advisory. A token lexer cannot be exhaustive, and this is the same structural limit SP-4 recorded.

### A3 / A4

- A3: missing paths `Store(Io)` is unchanged. Code, tests and brief agree. The dispatch wording was a coordinator brief mistake, not a contract change. Agreed.
- A4: PROGRESS keeps "coordinator Sol medium" for the overlap test and these follow-ups, records it as a deviation from KICKOFF line 92, and does not relabel it. It credits the 227/21 results to the reviewer, and credits the architecture to the prior Sol review. It is honest.
- The ADR status says "library candidate; E2 provisioning and CLI wiring remain unimplemented", which is accurate. No native constructor, new public item or authority expansion is in the diff. `pin_private_dirs`/`walk_chain` stay private fns, and visibility is unchanged.

## Standards

- No documented standard is broken. The commits are conventional, a meaningful test was added, and the deviation is recorded.
- The `map_err` in `pin_private_dirs` carries an explanatory comment. Smells: none apply.
- Info: the private remap relies on `structural` staying the only `RecoveryRequired` source in `walk_chain`. If a later walk step adds `Failure::RecoveryRequired`, it would be silently mapped to `Permissions` on private paths. Both values refuse, so safety is unaffected. The new layer test would not catch this. No action needed now.

## Checks run by this reviewer

Ordinary user, a `git archive` export of `4a5919e` in a session scratch directory, owned temp and fakes only.

- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: pass. arxd was checked from the export path.
- `cargo nextest run -p arxd --locked -E 'test(owner_windows) | test(u1_uninstall)'`: 22/22. The baseline rebuilt run is `5afffb8f-a41f-4094-a838-0f14729179c7`. It is 21 plus the new layer test.
- Mutations: each was applied with perl, its rebuild was confirmed by a `Compiling arxd (…e1…)` line, and the file was then restored from `git show`. A byte compare against `4a5919e` confirmed every restore.

| id | mutation | result |
| --- | --- | --- |
| a1 | `open`: `pins.clear();` after `enable_restore()?;` | **not caught**, 22/22 (`cf877489`) → E1-R1 |
| a2 | `walk_chain` structural back to `.map_err(\|_\| Error::Permissions)` | layer test fails (`5dc9b744`) |
| a3 | private wrapper drops the `RecoveryRequired → Permissions` arm | layer test and U3 fail (`67ccb0aa`) |
| a4 | `uninstall` passes `Vec::new()` to `with_lock` | S1 fails (`a1c5fddc`) |

I did not repeat a full workspace suite. The coordinator's 228/228 (`e15b6725`) belongs to the coordinator and is not re-claimed here.

## Limits

- No production constructor (`OwnerGuard::acquire`, `WindowsOwnerSession::open`/`uninstall`, `Layout::system`, `WindowsScm::system`), real SCM (including read-only), LSA, privilege, account, owner, admin, service-manager, VM or Lab Manager call. No pipe open/connect. No ignored fixture was invoked directly.
- No push, PR, GitHub write, status, approval, merge, watch or dispatch. Local only, so there is no PR status. The shared checkout and other branches were not touched.
- Installed, native and dedicated M2 paths, SYSTEM-owned `ProgramData` and restore-enabled pinning are **unverified on real hardware**. There is no acceptance claim.

**Totals:** Standards has 0 findings and 1 info note. Spec has 1 low, non-blocking finding. The worst is E1-R1: S1 does not reject `pins` being cleared or rebound before `with_lock`.
