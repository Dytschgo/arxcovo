# Anchored pre-proof local source: stopped WIP

Sol 6.1 high implementation child, no children, 2026-11-17. Assigned isolated
worktree only. **Incomplete WIP; no publication or behavior acceptance.**
M2 open / M3 unchecked; hardware **unverified on real hardware**.
Historical Sol3/3 and Opus0/2 remain unchanged; no Windows repair credit.

## Pins and scope

Authorized unpublished base `be3598241007038764207d5a77a24e1ce0ab4b64`;
actual main `59a665c9cda602d0b2939613656f2c98399f51f1`.
Source WIP `9b6c7e0` (full SHA in local artifact manifest).
Read AGENTS, full KICKOFF, engineering skill/behavior safeguards/active profile,
full anchored brief, normative root R1/R2, independent Sol recheck and latest
ADR0005 amendment. Root-supplied main CI44114182696 attempt1 success is reused
only:609s wall/Windows604, all92 tracked cases per OS/Unix fixtures28.
Windows U6 Stop/Orphaned non-gating failure earns no cause/fix/re-enable credit.
No remote query was made.

Source changes are confined to the ten allowed paths: new anchor, preproof,
preproof tests and anchored founder test; enrollment module/delegation and proof
byte accessor; pki module/identity visibility; consensus test binding; additive
quarantine guard. Existing tests, peer verification bodies, Cargo/lock,
workflows, policies, timeouts, retries, permits and PROGRESS are unchanged.
No public endpoint/query, discovery, signing authority, enrollment schema or
production engine behavior was added. JoinCode::proof remains trusted internal
API; the wrapper only establishes its own connection-owned path.

## Mandatory stop

The first scope/UTF8 classifier asserted that every untracked path belonged to
the source allowlist. It included its generated `.task-evidence/inventory.json`
(currently untracked evidence) and failed before reaching UTF8 byte comparisons.
This is a classifier failure, not observed out-of-allowlist source mutation.
Per explicit natural static-suite stop rule, no correction or rerun occurred.
No pure Rust behavior test or mutant ran. No mutation kill or restore credit.
The inventory operation had already started before this failure; its final
state is recorded below. No new verification operation was started after stop.
`git diff --check` after stop was an additional read-only preservation check
(exit0); it is disclosed rather than counted as the prescribed verification.

## Actual commands and correction accounting

All Cargo compilation uses task-specific `target/anchored-preproof`, locked and
offline flags (fmt has no dependency resolution). Logs/caches are retained in
`.task-evidence` in this assigned worktree; no installs, network, admin, native,
VM, old Lab Manager, protected VM or cache deletion operations occurred.

- `cargo fmt --all` applied formatting; `cargo fmt --all -- --check` exit0 before
  first Clippy and again before inventory.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`
  first exit101: incorrect `super::found` import; unused production `encode`.
- Mechanical correction batch1: import `issue::found::founder_init`; annotate
  encode's intentional no-production-query-caller dead code outside tests.
  Same Clippy command then exit0,14.05s. One of two build correction batches
  used; no behavior-driven fix, counter reset or algorithm chase.
- `python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`
  exit0,7 tests,0.301s. Includes additive required binding, omission, rename,
  ignore, Unix hiding and helper/start-count negative controls.
- `python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py`
  exit0,60 tests,0.264s. Old sampler unchanged.
- Inline Python scope/UTF8 classifier exit1, AssertionError on generated JSON;
  mandatory stop. Scope/UTF8 byte comparison acceptance **UNRUN**.

Preparatory read commands: PowerShell rejected Unix brace expansion in an rg
argument (parser error); corrected read used explicit filenames. A guessed
`.config/nextest.toml` path was absent; another rg glob was invalid on Windows.
These read-only command failures changed no source and ran no behavior checks.
They are separate from the single compile/import/lint correction batch.

Before the first behavior checks, pure quota/gate controls and founder cleanup
ownership were completed. Clippy covers their final source. New tests cover
actual TLS plaintext phases, synthetic CA attacker lists/keys and a mismatched
CertificateVerify resolver; none has runtime pass credit. Quota tests include
pre-admission, aggregate/overflow, caller-buffer drains and partial-write fatal
controls; none has runtime pass credit. Independent review must assess coverage
and implementation fully; static construction is not proof of those claims.

## Remaining work and proposed PROGRESS (not applied)

Root disposition is required before any classifier correction, natural pure
round or designated M1/M2/M3 execution. Exact M1/M2/M3 patches, runtime logs and
restore hashes do not exist because no mutant was authorized after stop.
All natural anchor/preproof TLS cases, existing pure pki cases/TLS10 and exact
literal_anchor_and_proof_binding remain **UNRUN**. No local founder/consensus
engine case ran. New anchored_preproof_single_voter first runtime evidence must
come from ordinary hosted all-OS CI. Existing fixture28, sample8, founder13,
TLS14 and atomic issuance1 runtime/listing acceptance is not inferred from
source or guard passes.

Independent full source and exact-head Sol review, normal five gates, hosted
all-OS proof, feature Windows<=660s and fresh exact main remain mandatory.
No push, PR, comment, status, watch, link, merge or protection change occurred.

Proposed PROGRESS: Sol6.1 high anchored-preproof source9b6c7e0 WIP; fmt/Clippy
pass after one mechanical batch; guard7/sampler60 pass; scope classifier stopped
on generated JSON before UTF8 checks. Natural Rust cases/mutants unrun; root
disposition, independent source/exact-head review and hosted gates pending.
M2 open/M3 unchecked; hardware unverified on real hardware; counters unchanged.

## Retained artifacts and final ownership

Already-started `cargo nextest list --workspace --all-targets --locked --offline
--message-format json` finished exit0. Its inventory.json is101589 bytes and
inventory-build.log6375 bytes. No test executed. No inventory classification
was attempted after the mandatory stop, so protected count/ignore pinning is
still unverified for this head. The pending listing's shell and child processes
finished; all three owned Cargo sessions returned completion.

Source full SHA: `9b6c7e060e54d9ec8b44fc12e0715ab92412a3bb`.
Source patch SHA256: `d7bc1e6cb7d3a20e003132809cc2d273c89a7b93d341bada6a693739d1ada002`.
Local artifact manifest `.task-evidence/artifact-hashes.json` SHA256:
`11bfeae6bf5407097eb3505cfa62ba64b9f6055b4d8e9de755a27929b1d477d2`. It pins both Clippy logs, both fmt logs, guard,
sampler, listing/build output, recorded scope failure and full source patch.
No mutation or restoration hashes are claimed. Logs/caches remain in the
assigned worktree; generated evidence files are retained locally, uncommitted.
The source stop was preserved without a classifier fix or natural-suite replay.
