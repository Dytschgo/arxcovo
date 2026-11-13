# ADR 0020 E2 local inactive implementation evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13. Implementation sub-agent;
no delegates. E2 only, over a fake CgroupFs. Independent Opus 5.5 high review
is **pending and required before publication**. No activation authorization.

## Isolation and pins

- Initial clean worktree HEAD: `ab9ea634ff698397a6e98e9773905aa77b19ae5e`
  (E5 PR59 parent, supplied as reviewed with Windows CI pending).
- Accepted local contract: `fc1a6648515ef2227b59623df75e599da3bc9abe`;
  merged contract: `3875466129dd16d0dfa60387a3015726b17ee17a`.
  The two ADR/brief files compare equal at these contract pins.
- Merged E1 main: `2fbc6476fde3aaa5c1e94ad46872e46315d73018`.
- Source commit: `b40e81ab5e088e99db7f8790a630d3281fe7c2cf`.

Get-Location and git rev-parse --show-toplevel matched the supplied isolated
worktree, outside the canonical checkout; git rev-parse HEAD matched the
requested initial pin and git status --short was empty. No checkout was
created, switched or operated on elsewhere. Read AGENTS, KICKOFF, the accepted
ADR 0020, the whole implementation brief, E1 parser/API and existing Stage B
payload scanner. The direct task overrides coordinator/babysitter/PROGRESS
duties in the general kickoff. No GitHub state was refreshed.

The source commit changes only hierarchy.rs plus new hierarchy/setup.rs and
cfg(test) hierarchy/fake.rs. Recursive git tree comparison against the initial
pin lists only these three paths. Every E5, E1 parser, service, policy, Cargo,
CI and PROGRESS byte is preserved. No native adapter, identity observation on
the host, real filesystem adapter, shim/probe, readiness, executor or service
wiring was added. No architectural deviation is claimed.

## Source manifest

Git blobs and SHA-256 of committed bytes (also compared with worktree bytes):

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/hierarchy.rs` | `f37f9d0f9edb033af6fc60729c42d811c864bca5` | `d66127a5ed0fbdc98d1cdedaf1011c72e8451c0ebac8184dcf05a49e56bf552f` |
| `crates/arxd/src/exec/hierarchy/setup.rs` | `b33bed7cd5d7022f30c039e761c544567304d28c` | `ab04270eb077ef8fe5c4ae6accc7d053b926a0465aa75fa31ed626f47347c179` |
| `crates/arxd/src/exec/hierarchy/fake.rs` | `536bad25845aa42d52b487567c780b8b8bf2c503` | `3110adb6d7f328414aa5a508d374e6ad1e3c9e21dfb6f05ea21eb2e2addbc495` |

## Contract coverage and implementation limits

The driver uses opaque associated Dir handles and fixed FsError variants.
It returns only SetupOutcome/SetupFailure and u8/bool SetupEvidence. Its cause
mapping covers the setup subset; bridge refusal/readiness integration remains
E6. Ready is an outcome here, not a production readiness transition.

S0 precedes identity. Every anchor completes before the unit write. Unit and
runs enables write exactly +memory +pids and re-read subtree_control. Setup
validates an existing runs directory, kills the whole subtree, polls strict
multi-key events, validates all leaves before any removal, and uses fresh
verify and S9 listings. Shutdown is checked between steps, during waits,
during leaf validation/removal and immediately before returning Ready.

Port obligations are explicit: trusted no-follow root/ancestor binding, bounded
proc reads (4096/8192), bounded xattr (2 bytes), bounded control reads and full
fresh listings. ProcFile/Vec and xattr Vec represent returned bytes; equality
and parser guards reject invalid/oversized values at the driver. These types
alone do not prove a future adapter bounds allocation or honors no-follow.
The fake bounds its control reads/listings. Real descriptor behavior is E3,
including L9 fresh open descriptions; no native claim follows from this fake.

The test-only model computes child controllers from parent subtree_control;
checks controller availability before applying the complete enable write;
rejects enablement with resident processes; recursively clears tasks after
configured events polls (including frozen tasks), or never; refuses removal
of populated/nonempty directories; lists kernel controls, dots and typed
entries. Per-operation fixed errors, logs and before-operation hooks support
state changes between named operations. Manual Clock advances 5 ms with no
real sleeps; a fixed test-only upper bound stops a deadline mutant hanging.

| Brief | Test in exec::hierarchy::fake | Evidence |
| --- | --- | --- |
| H1 | h1_fresh_top_down | Exact writes, mkdir mode, kill and top-down order, typed evidence |
| H2 | h2_prepared_idempotent | Two setups on existing tree, created false, repeated writes |
| H3 | h3_leftovers_kill_wait_validate_remove | Two populated leaves; single subtree kill before listings, all checks before removal; model enforces EBUSY |
| H4 | h4_deadline | Never-clearing subtree, KillTimeout/Janitor, no removal |
| H5 | h5_removal_busy | Injected EBUSY maps LeafRemoval |
| H6 | h6_all_names_before_removal | Invalid later siblings, zero partial removal, recursive kill confirmed |
| H7 | h7_nested | Nested descendant killed but structure refused |
| H8 | h8_types_and_trust | Refused entry kinds in runs and leaves, failed unknown stat, bad owner/magic/mode |
| H9 | h9_bounds | 65 children; exactly 512 records passes and 513 fails, dots included; 4097-byte type refused |
| H10 | h10_anchors_zero_mutations | 21 independent anchor failures with exact fixed outcome and zero writes/mkdir |
| H10b | h10b_hardening_first | Non-root plus unhardened gives Hardening without identity call |
| H11 | h11_enable_unit_reread | EINVAL or missing pids on re-read; no negative write token |
| H12 | h12_runs | Threaded, non-root, writable, resident process runs refused |
| H13 | h13_swap | Missing swap control is Postcondition |
| H14 | h14_shutdown_boundaries | Wait, S5/S6 and S9/S10 cancellation; Stopped, no inappropriate mutation |
| H15 | h15_panic_payload_subprocess | Isolated re-executed test; rmdir panic becomes Panicked; fixed literal exactly once, state marker absent in stdout/stderr |
| H16 | h16_own_pid_reuse | Own-PID leftover removed |
| H17 | h17_frozen_recursive_kill | Frozen processes killed and leaf removed |
| H18 | h18_verify_fresh | New valid-looking child before verify gives VerifyFailed/Janitor |
| H19 | h19_verify_populated | Population changes before verify refused |
| H20 | h20_postconditions_fresh | Fresh S9 child/refused entry rejected; normal multi-key files pass H1/H2 |

model_kernel_contracts independently tests all-or-nothing EINVAL, inheritance,
resident EBUSY and populated/nested removal EBUSY. step_failures_are_fixed
covers additional per-step I/O errors, malformed events, oversized events,
procs, subtree_control, proc status and wrong status magic.

## Verification on Windows

Host: Windows, x86_64-pc-windows-msvc; rustc 1.95.0
(59807616e 2026-04-14), nextest 0.9.151 (6b3f99457). No toolchain installs.
Final source bytes were settled before the final normal full workspace run.

- Initial `cargo nextest run --locked -p arxd -E 'test(exec::hierarchy::fake)'`:
  22 passed, 338 filtered/skipped; `c0560de2-4c6b-4f61-85e4-0b25a564ccab`.
  That earlier run emitted unused Io warning; the later fault coverage uses
  this variant and final Clippy is clean. It predates the extra fault test.
- Final focused `cargo nextest run --locked -p arxd -E 'test(exec::hierarchy) | test(service::unix_bridge) | test(exec::stage_b_tests)'`:
  **58 passed**, 303 filtered/skipped, 2.147 s;
  `d4e43613-466c-48c1-8065-4ccd857fee20`.
- Final `cargo fmt --all --check`: passed.
- Final `cargo clippy --workspace --all-targets --locked -- -D warnings`:
  passed, 2.98 s warm check (initial check also passed, 52.32 s).
- Exactly one final `cargo nextest run --workspace --locked`:
  **360 passed, 3 skipped**, 12.514 s;
  `c3e5869b-b957-477b-a616-7f16cf1ce4da`. No retries.
- `git diff --check`, staged whitespace checks and source byte comparisons:
  passed. Source tree was clean after the source commit.

The coordinator supplied an unexplained baseline failure at full run 65762554
in service::executor::tests::adr0017_dispatch_durable_matrix (unix_windows),
with 11 focused passes and a later 337/3 complete pass. It did not recur in
this run. No unrelated service code, test expectations or retry policy changed.
No raw database/assertion payload was printed or retained from the full run;
only fixed summary lines were retained. Nothing here diagnoses that flake.

### Payload/panic audit

Executed a standalone audit in ignored target/e2-audit.rs: extracted the existing
Stage B tokens/scan/scan_with_hook functions unchanged, compiled with
`rustc --edition=2024 target/e2-audit.rs -o target/e2-audit.exe`, then ran
`.\target\e2-audit.exe` after source restoration. hierarchy.rs, parse.rs and
setup.rs each produced an empty production payload inventory. Nine lexical
negative controls rejected panic_any, resume_unwind, aliased set_hook, unwrap,
formatted panic, stderr, aliased tracing, macro_rules and failure conversion.
Token checks found no kill/killpg/setpriority calls. The fixed fake panic is
excluded through cfg(test), not a production allowlist addition.

Manual review additionally checked the new production driver for formatting,
logging, diagnostic imports/aliases, hook changes, panic wrappers and payload
access: none added. catch_unwind matches Err(_) without inspecting or formatting
the payload. H15 verifies default-hook output with synthetic state in the fake;
it is not a claim that catch_unwind suppresses arbitrary payloads. No service
hook changed. The durable shared scanner inventory extension remains E6 scope;
this standalone audit does not claim that unmodified CI scanner covers E2 yet.
No token/source scan proves all possible Rust lexical bypasses or native safety.

### Compiled mutation ledger

Each row ran `cargo nextest run --locked -p arxd -E
 'test(exec::hierarchy::fake::<prefix>)'` on one production-driver mutation.
Every row compiled, reached its named Rust assertion and exited 100.
No compiler failure or irrelevant/uncompiled platform test receives credit.
Original setup.rs bytes were restored and compared byte-for-byte in a finally
block after every row. The complete ledger passed before final audit/checks.

| Mutation | Named prefix | Run ID |
| --- | --- | --- |
| early-write | `h10_` | `f0a8f907-22e8-4402-8ead-48e3d13ddfea` |
| per-leaf-kill | `h3_` | `409c9673-f59f-472b-a835-253eaf96d704` |
| remove-before-wait | `h3_` | `7c95968a-98c5-479b-811f-fc5c10c60f27` |
| unknown-name-accepted | `h6_` | `11940634-8bca-45df-afc4-c2e487df7370` |
| partial-removal | `h6_` | `dd936220-9d38-4565-b902-68d6f85d56ce` |
| ignore-symlink | `h8_` | `3baaed45-5ce7-46a9-8d4f-1b56d4f90555` |
| dots-outside-budget | `h9_` | `f2f30d28-ce1d-40d5-9086-cca4a52e6339` |
| skip-verify | `h18_` | `5061a4b9-89ac-464f-a2a5-3205c6cf3adf` |
| reuse-listing | `h18_` | `3b189a20-765b-4df1-883d-056b652a197a` |
| ignore-deadline | `h4_` | `69be9d14-0461-47ee-8d15-261ddd91b816` |
| stopped-ready | `h14_` | `bc2e3d3c-f754-4bc6-9e1a-79382ce0d5e3` |
| negative-controller | `h11_` | `937c29ab-24fd-48f6-97b7-69eaba910919` |
| runs-before-unit | `h1_` | `b16b4838-76d9-44e7-b681-9fc7d4fa2ef8` |
| identity-before-hardening | `h10b_` | `1cd7d645-80ba-426b-8e7b-ddd2d860318b` |

Evidence-field privacy is enforced by the declared u8/bool field types and
reviewed by reading the declaration and typed ready helper; no payload-field
mutation is credited as a behavior kill. The stale-listing mutant substitutes
a cached/assumed-empty child result instead of the fresh verify listing.
The early-rmdir mutant fails against the fake's populated EBUSY behavior;
the deadline mutant is bounded by the manual clock guard, yielding Panicked
instead of the required KillTimeout. No hung test or timeout is counted.

## Boundaries and handoff

No native cgroup, /sys, /proc, account, identity check on the host, service,
owner/admin action, VM, experiment, private state, old Lab Manager, ignored
native fixture, production runner, push, PR, GitHub or CI operation was invoked.
Only the current test executable was re-executed for H15. PROGRESS is root-owned
and unchanged. CI gates and the parent PR59 Windows check remain unverified
here. Linux/macOS compile and runtime remain pending; Windows fake tests give
no credit for those targets. Native paths remain **unverified on real hardware**.
M2 remains incomplete. E_PROTECTION/ExecutionFailed remain unknown; no retry,
fix, waiver or diagnosis was performed. Production stays MechanismUnavailable.

Next: independent Opus high exact-source review, then coordinator-owned
publication/integration only after dependency and required gates pass. This
local implementation/evidence does not grant activation.

## First source-review correction

Opus high review dd4dad8 (retained3bc285e) passed Standards and required
Spec B1/B2 tests-only repairs. Sol medium correction `70729f68a649aaad5f7b2282a4c7fa90b93188af` sets the H14
kill delay to never complete, and adds a valid-but-different cgroup location
case requiring Locate with zero mutations. Production setup.rs is unchanged.

Fmt and locked workspace/all-target Clippy passed. Three targeted tests passed
(eaabfd60); final E2 fake selection passed 24/24, run
`b23eb3e4-1197-43ad-9641-aade301bef27`. No full workspace repeat.
Both production mutants compiled and failed the intended test assertion:
- wait-cancel: `640ea4e8-b015-44c4-8570-70078b0903d5`, exit100.
- location: `67492d14-55c4-4f98-8d9e-9f2ee09a1906`, exit100.
All mutated setup.rs bytes restored exactly. Original 360/3 full suite
credit stays at b40e81a. Narrow independent recheck pending; no publication,
hosted run, native action or activation. Advisories remain disclosed in the
review, including unproven mkdir mode and fake/driver guard distinctions.

Updated committed fake.rs manifest:
- Git blob `0de9e4e95ee4101f195b06e5115203035e7ac078`; SHA-256 `69838972dbd426c3c227ba0613a7054a609d0b8dc34c912780f52d60043ed90f`.

## Publication checkpoint

Opus high recheck352a8c1 (retained12578aa) passes Standards and Spec, closing
B1/B2. Current main064113a is unchanged since the root port; source delta
remains exactly hierarchy.rs/setup.rs/fake.rs. Production setup.rs is identical
to b40e81a, and fake.rs matches corrected70729f6. Retained review notes are
unchanged. No extra full-suite run for report/docs changes. Exact-head
integration review and all hosted gates remain required.
