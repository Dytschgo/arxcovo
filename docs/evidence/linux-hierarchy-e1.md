# ADR 0020 E1 current-stack local port evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13; implementation sub-agent,
no delegates. Local inactive E1 port only. Ready for fresh source review;
**never independently reviewed** at the old source pin or this new pin.
Opus 5.5 high review will follow this port. No publication or integration.

## Pins, isolation and source comparison

- Initial clean HEAD / contract stack: `fc1a6648515ef2227b59623df75e599da3bc9abe`.
- Actual main Stage D baseline: `f91ae5a7d95645fb2057f70411675d6f1e07d1df`.
  `git diff --stat f91ae5a HEAD` before port contained documentation only.
- New source commit: `55c22200e52d40ccb16d89d307c8764312cfa65c`.
- Prior Sol source: `2bbc42790781a3046bfe1a84bcbe5f1fc47789f9`.
- Prior evidence: `59aff0b47f5aa05404e9a8c0562353f1dcb2bdb5`.
- Prior source base: `4272291aa199c4f32a5bcecbfeedbfd76349c2db`.

Runtime working directory and git top-level matched the runtime-prepared
isolated worktree, distinct from the canonical worktree. Initial status was
clean at the requested stack. No checkout was created or switched. Read
AGENTS, KICKOFF, ADR 0020 and brief E1; current accepted inactive contract and
local sequencing govern this work. PR56 is still waiting for gates per the
coordinator's supplied state; no GitHub refresh or operation was performed.
Stage C/D are already merged. Publication/integration waits for PR56 merge,
final exact re-pin (squash tree equality may prove it), and independent review.

Explicit `git show` byte comparisons established that the three existing
source paths matched the old base before the port. Both new parser paths were
absent. All five ported paths now equal the prior source's committed bytes
and blobs exactly, including tests. No additive module adaptation, divergence
or unrequested correction was necessary. The source commit changes exactly
these five paths (+647/-23). No old PROGRESS or unrelated history was ported.

The old-base/current-stack source diff lists only `crates/arx-guest/tests/cli.rs`;
that current guest guard/policy blob is unchanged by this port. Every path
outside the five-path allowlist stays equal to the current stack in the source
commit, preserving Stage C/D and all current policy. No E2-E7 or E5 plumbing,
constructor, activation, dependency, packaging or CI change was made.

## Exact new source manifest

Git blob IDs and SHA-256 of committed LF bytes, verified against the old source.

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/hierarchy.rs` | `42272616e9015037bda4ee88778e9e34b70ecfcd` | `fc4a6588b092061fe7369d3deceb131cfe15e037b1cc7910b6044e4cc34aa5e9` |
| `crates/arxd/src/exec/hierarchy/parse.rs` | `3917a2ece37fc6db80fb961e8456ecdb8a30911e` | `df8667bc90e359ab4920a19d7506089fd3bdd900c31d126b8603d63ec0ea0b0b` |
| `crates/arxd/src/exec/mod.rs` | `8969090ff74062f3b29bf1cc48d14aef7452f080` | `587af0bb02cc76d0bb3d0965c5c4676d227f7e00ac83c0c44e29227118e88b17` |
| `crates/arxd/src/service/unix_bridge.rs` | `4233dc46cdc4a95b4d4a291a9882154eef376156` | `c668ee01016c1c1d632f7a4371e8e424927ee6d33c9244dc99ef4df0d08dbc77` |
| `crates/arxd/src/service/unix_bridge/tests.rs` | `fc9b8ce04e77f7837d6017120026839fa0115d38` | `d2f223390e7e38930e21702c20e33e6fc57c1d05cfcb255920aac9e6a9595571` |

The module declaration is the only exec/mod.rs delta. The bridge's unused
private parser is replaced with the shared locate import under cfg(test).
Its T2 test now delegates to locate and intentionally refuses foreign unit
components: `arxd.service` is mandatory under ADR 0020 A1. The existing
accepted path remains accepted. No real runner or production path was called.

## E1 acceptance coverage

All names below are in `exec::hierarchy::parse::tests` unless stated otherwise.
These unchanged assertions exercise the E1 contract; the table records scope,
not an independent security review.

| Brief | Named test | Coverage |
| --- | --- | --- |
| P1 | `p1_location_and_single_parser` | Valid unit/slices, invalid bytes/paths/bounds, one parser source pin; bridge T2 tightens foreign-unit refusals |
| P2 | `p2_leaf_grammar` | Canonical decimal leaf fields and digit/name limits |
| P3 | `p3_events_strict_keys` | Multi-key events, order, future keys, duplicate keys, malformed and over-bound input |
| P4 | `p4_controller_tokens` | Exact memory/pids tokens, substring refusals and bound |
| P5 | `p5_pid_decimals` | Empty/PID list, canonical decimals, u32 overflow and byte bound |
| P6 | `p6_exact_hardening` | Exact c1/NNP, subset/superset/case/duplicate/missing/bounds refusals |
| P7 | `p7_strict_rights_json` | Five sudo values, three NNP values, strict keys/types/duplicates and bound |
| P8 | `p8_exhaustive_rights_table` | All 60 combinations against separately written literal table |
| P9 | `p9_portable_directory_types` | Linux ABI types as pure data, unknown/stat errors, symlinks/special files/control grammar |
| P10 | `p10_exact_bin_alias` | Exact two aliases, suffix/path variants and bound |

`bounded_valid_inputs_and_total_byte_data` additionally covers accepted exact
bounds and all byte values through bounded inputs. This is smoke coverage,
not exhaustive fuzzing. E1 performs no filesystem/process/environment reads.
P9 runs on Windows as pure data; it proves no native directory listing.
The future P-SCAN inventory extension remains E6 scope.

## Fresh current-stack verification

Host: Windows / x86_64-pc-windows-msvc. rustc 1.95.0
(59807616e 2026-04-14); cargo-nextest 0.9.151 (6b3f99457).
Commands ran on the exact source bytes committed as
`55c22200e52d40ccb16d89d307c8764312cfa65c`; no source changed afterward.

- `cargo fmt --all --check`: passed; no formatting rewrite required.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed,
  dev profile completed in 48.57 s.
- `cargo nextest run --locked -p arxd -E 'test(exec::hierarchy) | test(service::unix_bridge) | test(exec::stage_b_tests)'`:
  **35 passed, 301 filtered/skipped**, 2.059 s;
  run ID `20f14da2-9fa0-4f7a-8912-cfd780ea80b8`, default profile, no retries.
  Includes P1-P10 and boundary smoke, affected bridge fakes, scanner inventory
  and negative controls, and production-inert source assertions.
- `git diff --check` and `git diff --cached --check`: passed before source commit.
- Byte/blob comparisons above: passed. Final tree/diff checks recorded below.

No fresh full suite was run. No CI run, rerun, push or PR was requested or made.
Windows checks do not prove Linux/macOS compilation or native execution;
Unix-only tests not compiled locally receive no new credit. The selected
bridge/scanner cases use fake ports/source data. No ignored native fixture was
invoked directly, and no actual runner or production path was called.

## Historical checks credited only to old source

The following is retained from prior evidence
`59aff0b47f5aa05404e9a8c0562353f1dcb2bdb5`, about source
`2bbc42790781a3046bfe1a84bcbe5f1fc47789f9`, not new execution here:

- fmt and locked workspace/all-target Clippy passed on that old source.
- Focused selection: initial 34/34 (`d894f3bf-64cd-4c84-8797-f56e652d1384`),
  final 35/35 (`fdcb9f3e-7013-445e-977d-68dfb67411fc`, 301 filtered).
- Exactly one old workspace run: `cargo nextest run --workspace --locked`,
  `a5b291e6-9797-4a20-9e17-62b20d63a884`, **334 passed, 3 skipped**, 11.776 s.
  This is old-source evidence only, never a fresh full-suite claim.

## Historical 13-mutation ledger (not rerun)

Preserved verbatim from the old evidence. Each used
`cargo nextest run --locked -p arxd -E 'test(exec::hierarchy::parse::tests::<prefix>)'`.
All 13 compiled and failed the named Rust assertion (exit 100); compile failures
were not credited. No mutation was rerun because all five source blobs,
including tests, are identical. Credit remains with the original runs.

| Mutation | Test prefix | Nextest run ID |
| --- | --- | --- |
| Remove exact arxd.service comparison | `p1_` | `0eb7c2e3-7399-4438-baa2-81fdb22502b9` |
| Restore a bridge-local cgroup_parent helper and locate wrapper | `p1_` | `041a87f0-526a-45f2-bb12-c5919296af64` |
| Remove canonical leading-zero check | `p2_` | `747eb342-c412-4ff7-b8b9-2e5eadbb9aa7` |
| Replace events with any populated-zero line | `p3_` | `bb8a7a57-fcdf-4567-a769-b052f5b598c7` |
| Replace events with whole-file populated-zero equality | `p3_` | `69b75707-410f-457b-a1f9-346b54b2aa90` |
| Match memory by substring | `p4_` | `dfbdbedf-533f-43c3-9044-b483ae2daf48` |
| Accept bounding-set subsets of c1 | `p6_` | `5a0beb11-fe48-4b46-ad6c-6b81c12c6cb4` |
| Classify DT_LNK as Child | `p9_` | `fb212eda-099a-42da-b6ac-90bfbce49308` |
| Classify DT_LNK as Control | `p9_` | `dcc29c3d-b633-4596-a89a-1e443024816f` |
| Accept any usr/bin suffix | `p10_` | `a4b6354b-b0e6-49dc-9344-9d0d169fd6e8` |
| Accept unknown in standard rights row | `p8_` | `d09c91ac-a85b-40d8-856a-dbbc7926e05a` |
| Accept untrusted in standard rights row | `p8_` | `ebba759c-f4c2-48eb-92ec-92092befe5a1` |
| Replace false with true in admin/none exit-zero success row | `p8_` | `147bcd27-4abc-4b1e-8734-48be723784d8` |

Old harness history remains in the prior evidence commit: quoting/needle and
ledger-recognition failures had no behavior credit, and the first bridge-helper
mutant failed compilation without credit before the corrected compiled kill.
The old driver restored/compared original bytes after each executed mutant.
No old raw-log artifact is claimed as newly produced or inspected here.

## Boundaries and handoff

No root/account/service/cgroup/VM/owner/experiment operation, private-state read,
ignored native fixture, real runner or production path was invoked. No old Lab
Manager installation/state or VM was touched. No owner-account path is recorded
here. PROGRESS, GitHub and CI remain coordinator-owned and unchanged.

M2 remains incomplete. Native, installed and dedicated paths are
**unverified on real hardware**. ExecutionFailed and E_PROTECTION remain
unknown/unresolved: no fix, waiver or retry. Production remains
MechanismUnavailable. This evidence grants no publication, integration,
activation or acceptance; fresh Opus high source review is next.

Final verification: recursive committed tree comparison confirmed exactly the
five authorized source paths differ from the current stack at the source
commit; every other tracked blob/mode is unchanged. All five worktree files
still match old/new source bytes. The 13-row ledger matches old evidence
verbatim. Evidence is the only additional path in the final two-commit range.
`git diff --check` and staged whitespace checks passed for evidence; final
status is clean after the separate evidence commit.

## First source-review test correction

Coordinator OpenAI / gpt-6.1-sol / medium, 2026-11-13. Source review e77cda7
(retained 88f4f2f) found masking of locate trust-anchor tests. Test-only commit
ddc89fc adds invalid bytes before the intact arxd.service/supervisor suffix,
a multi-line v1/v2 case and a prefixed service name. It also covers maximum
leaf length, DT_UNKNOWN socket/block/FIFO, mixed-case events with populated
present, and hardening-line suffixes. Production parser bytes before cfg(test)
are identical to reviewed 105380f; no implementation behavior changed.

Baseline and restored final focused parser/bridge runs each passed26/26.
Final fmt and locked workspace/all-target Clippy -D warnings passed. No full
workspace repeat. Five independent guard edits compiled and failed assertions;
original bytes restored and checked after each:
- nul: dd2b5788-dcd6-4299-aa8d-562fc261c2e4
- backslash: 007943d6-38e8-449d-8c26-67c4d67165d8
- newline: 21f9a03e-50b9-4708-84bc-a3090fb1046c
- CR: 4f417f65-7cba-4fbb-847b-dc347036f6f0
- unit-suffix: 04f327ef-422c-42a3-b6e9-c3e3e25f0409

Final focused run: 16213a53-68dd-4330-96fd-e98213f271e1, 26 passed, 310 filtered/skipped.

The cheap advisory fixtures are added, not credited with new mutation kills
for those advisories. S1 correction requires fresh independent recheck. No
publication or native action. Contract PR56 remains a prerequisite.
