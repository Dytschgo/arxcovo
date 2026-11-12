# ADR 0016 E1 local implementation evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-11. Implementation sub-agent;
no delegation, publication, PR, review approval, merge, watch or dispatch.

Base: `ce300603f5a70c0f0375498a98e4a46694b0c01c`.
`git diff aafaa3ccfa6aaf190fbc0a97cc28a3f5d4579511 ce300603f5a70c0f0375498a98e4a46694b0c01c -- crates`
was empty. The prepared base therefore retains main's runtime sources.

## Narrowed API

- `OwnerGuard::acquire()` holds the fixed layout, binary chain and exact running
  image identity; no privilege enable, private store or lock. Private fields;
  neither guard nor session derives Clone, Copy or Debug.
- `WindowsOwnerSession::open(guard)` pins every private chain before restore,
  lock, staging cleanup and recovery. Wrappers: `read_config`, `observe_batch`,
  `configure`. `parts` is removed; concrete store/right adapters are private.
- `WindowsOwnerSession::uninstall(guard, Deregistered)` requires the moved proof
  before any private store phase. `uninstall_locked` recovers before calling
  `uninstall_right`; `owner_scm::uninstall_flow` uses `deregister()?` before
  invoking its closure. Its only test proof factory obtains the proof from
  the existing absent fake SCM, without a new proof constructor.
- `pin_private_dirs` and the binary pins share the extracted no-follow walk.
  Private policy/ancestor refusals use `Layout(Permissions)`; missing paths use
  `Store(Io)`. No creation, skipping, repair or runtime selector was added.
- Store field order remains staging, metadata, pins, lock. Rust's declaration
  drop order releases staging/pins before the lock. This was inspected directly
  and asserted structurally, separately from the post-drop behavioral controls.
  Existing staging, publication, CAS and file-sharing code is unchanged.

## Executed verification

Caller administrator-role check returned `False`. All new behavior tests used
owned temporary fixtures and fake rights/SCM; no production constructor was
called. Existing native fixtures ran only through their normal parent tests.

- `cargo fmt --all -- --check`: passed.
- `git diff --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo nextest run --locked -p arxd -E 'test(owner_windows) | test(u1_uninstall)'`:
  20 passed. Includes existing N1–N10 and U1/U2/U3/S1.
- `cargo nextest run --workspace --locked`: final full run 190 passed,
  2 skipped, 15.779 seconds. The known ExecutionFailed flake did not recur;
  this does not resolve it. No retry-to-green or speculative flake fix.

U1 injects failures at every call in the successful deregistration trace,
malformed query responses, sticky active/pending/unknown states and a foreign
service. Every resulting error prevents the closure. An absent fake service
also exercises the successful continuation.

U2 uses fixture records and fake rights: granted revokes once; intent recovers
to unproven before uninstall (retained, no revoke); malformed recovery refuses
before a right call. U3 runs configure/uninstall fixture paths through the
shared walk. It checks missing, unsafe leaf policy, untrusted ancestor,
a directory replaced by a distinct file identity, and a junction. Refusals
leave staging sentinels and store/right logs untouched. Safe callbacks observe
rename refusal and Busy on the second lock open; after store drop both succeed.

Fixtures retain their owned private leaf handles. The shared walk validates
ancestors, then the fixture releases shared-temp ancestor handles, preserving
the existing sibling-fixture isolation contract. Production retains every
ancestor and leaf. No second fixture walk exists. Initial test development
caught shared ancestor pin interference with sibling fixture release and an
incorrect assumption that active fake services could never stop; both fixture
issues were corrected before the passing runs above.

S1 lexes comments, quoted/raw strings and character literals away, then checks
balanced impl/function/call structure, direct production methods and exact
policy/checker arguments before restore and lock acquisition. It also checks
private adapters, removal of parts, the sole uninstall_right call and field
order. Four independent on-disk mutations each ran
`cargo nextest run --locked -p arxd -E 'test(s1_production_private_pin_calls)'`:

| Mutation | Result |
| --- | --- |
| Remove real open private-pin statement | S1 failed, exit 100; call count 1 instead of 2 |
| Replace open ancestor checker with accepting closure | S1 failed, exit 100; production argument sequence absent |
| Remove real uninstall private-pin statement | S1 failed, exit 100; call count 1 instead of 2 |
| Replace uninstall ancestor checker with accepting closure | S1 failed, exit 100; production argument sequence absent |

Each mutation was restored in a finally block. No mutations remain.

## Remaining obligations

Fresh independent Opus review is required before any publication or merge.
This is E1 only: no E2 provisioning, installer, Namespace, CLI, Plan, MCP or
backend implementation. No PROGRESS edit; coordinator records delivery.
Installed SYSTEM/LSA/SCM and privileged paths remain **unverified on real
hardware**. No M2 acceptance, owner administration, account/right/service
mutation, VM work or old Lab Manager access was performed.
