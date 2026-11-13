# ADR 0020 Slice E4: inactive descriptor-bound rights probe

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-13.
Local implementation only; no subagents. Fresh independent Opus 5.5 high
source/security review is **pending** before any publication.

## Checkout and dependency pins

Runtime cwd and `git rev-parse --show-toplevel` matched the supplied isolated
worktree outside canonical. Initial `git status --short` was empty at
`38b207d6df5f4722486afd23ba219df8a59a66e0`. The canonical checkout was not used.
Prepared dependency: reviewed PR61 head
`231c367c689224614d96c15a90fc3a9e98ba9a8c` plus local native evidence docs.
Supplied main: `c56b6b5c6af49462f2219a1adc5ff88daffbd774`.

Read AGENTS, KICKOFF, accepted ADR 0020 (especially D8/R5 and the exact rights
line), the whole implementation brief, existing shim/tests and payload scanner.
The settled architecture is unchanged. Splitting private helpers within the
existing shim module is the authorized implementation choice, not a new ADR.
E1/E2/E3/E5 source, Cargo, CI, policy, owner and service code are unchanged.
PROGRESS.md remains the coordinator's responsibility.

The task supplied E3 native run `43493588144`: PASS as UID 1001, eight native
mutation kills, L7/L8 pass and exact-head review PASS; Windows pending. These
are inherited coordinator facts, **not new runs or independently fetched
results here**. E3 merge and exact re-pin remain required later.

## Source pin

Source commit: `4967b53adb8cad574f90cd9f14c537e6eb6a4ce2`.
The evidence is committed separately. Hashes below cover committed LF bytes.

| File | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/stage_b_tests.rs` | `4f376c915abacbee7ce468148d6b23d2c367c91d` | `87fac259920ecac3f34215bd87f985f250b0d0c801d5d47922ee79c334dd6318` |
| `crates/arxd/src/unix_shim.rs` | `e70d0881964fdfc18f8a51b4b51875e569b374b1` | `5e7a01094733e882b1ac67376cba5e24cbb1ad1db771d06a944806b747361fd4` |
| `crates/arxd/src/unix_shim_rights.rs` | `b269e00622d48a2bb7dfd8143e1cdec7f25b5177` | `17310f10a8035d2a732c7b2e87a3be4dae1d4caabdee8dade8c6cea8a7017928` |
| `crates/arxd/src/unix_shim_rights_linux.rs` | `2902061b468a2e1e29bfcadba7f61195b266fa8c` | `03da57a6763b7047b7be1d3250288c01d2542eeb9a56358d671fa8a016182d3d` |
| `crates/arxd/src/unix_shim_rights_linux_tests.rs` | `da4d91fa81abbbd47e53137df3955fb2d9b138a6` | `f5e9b270b58a5ef82b45d141259823ba3728fec8c83085a74938584b6032caf0` |
| `crates/arxd/tests/unix_shim.rs` | `437e59e1bd5b847e3ca8df83a79a03a72b7cab5b` | `7229422cc0a5a8a5a098f0362c9a05b8ee785346de2627e8088c02097414b570` |

## Implemented boundary

The shim appends exactly one fixed-key rights line after TOOLS, with a 96-byte
line cap and the existing 4096-byte aggregate cap before stdout publication.
Hook-first silent catch, root refusal, tool resolution and tool output privacy
remain in place. Linux reads only its own bounded procfs status for NNP;
non-Linux Unix reports null/unknown without probing.

Linux opens the protected anchor and candidate components without following
links, validates directory/file type, owner and write modes, and classifies
all three candidates before execution. Only the two exact bin alias targets
are dropped. Unknown observation errors prevent execution too. The selected
O_PATH/NOFOLLOW/CLOEXEC descriptor survives pathname replacements. The forked
child uses prepared argv/environment/descriptors, fchdir, dup2, execveat with
AT_EMPTY_PATH and a CLOEXEC error pipe; its stdio is /dev/null. Parent polling
has a five-second deadline and an own-child kill/reap guard. Prepared descriptors
are kept above stdio even when inherited standard descriptors are closed.

Anchor/owner/deadline/argv/environment fields and the replacement hook exist
only under cfg(test). Production defaults are pinned. The scanner includes
both private helpers without the shim's hook exemption; token controls reject
path exec, Command (including generic/qualified forms), raw exec syscalls,
environment lookups, aliases to lookup functions and formatted panic payloads.

## Checks actually run (Windows host)

All commands below completed with exit 0. No red check runs occurred. No green
check was retried without a source change. No unrelated privacy test was edited.

Before the source commit, focused checks on the evolving E4 working tree:

- `cargo test -p arxd --lib --locked unix_shim -- --nocapture`: 3 passed.
- `cargo test -p arxd --lib --locked s13_ -- --nocapture`: 2 passed.
- `cargo test -p arxd --lib --locked e4_ -- --nocapture`: 2 passed.
- `cargo test -p arxd --lib --locked p_scan_`: 3 passed.
- `cargo fmt --all`: applied formatting during implementation.
- Initial `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS;
  later scanner/source additions justified the final run below.

Final checks at source commit `4967b53adb8cad574f90cd9f14c537e6eb6a4ce2`:

- `cargo fmt --all --check`: PASS.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: PASS.
- `cargo test --workspace --locked`: PASS, 366 passed and 3 ignored across
  workspace binaries/integration suites. arxd library: 327 passed, 3 ignored.
  Unix integration binary: zero tests on Windows. This was the single final
  normal workspace run after source settled.
- `git diff --cached --check` before source commit: PASS.

These results establish portable line serialization/bounds, production/source
pins and scanner checks. They establish **no Linux compilation or runtime
result**. No crosscompiler, WSL or toolchain was installed. No E4 Linux fake,
NNP child, Unix S6 system-sudo test or native mutation was executed locally.
No installed service, account, real cgroup, VM, private state or experiment was
used; no ignored native fixture was manually invoked.

## Prepared Linux coverage and outstanding evidence

The Linux-only owned fixtures copy this test executable into a private root,
then create ELF variants carrying a fixed helper mode byte. One fixed helper
reenters via the argv seam; marker files identify the actual executable object,
including after unlink/rename. The helper checks empty inherited environment
apart from its injected fixture variable, root cwd and null stdin.

| Requirement / mutation discrimination | Prepared named tests | Status |
| --- | --- | --- |
| S6 last line / bounds | `s6_rights_last_and_bounded`; portable S12 ordering pin; existing Unix S6 expectation amended | Portable PASS; Unix S6 unrun |
| S8 zero/nonzero/signal/deadline, exec error vs exit, kill/reap, script CLOEXEC, non-executable | `s8_outcomes_and_reap` | Linux unrun |
| S9 final/ancestor write modes, links, anchor trust, later-candidate failure blocking earlier execution | `s9_all_candidates_before_exec` | Linux unrun |
| S9 foreign final owner | `s9_final_object_owner_check` (direct same-inode trust rejection, no chown) | Linux unrun |
| S9b alias exact targets and foreign link owner | `s9b_exact_alias` | Linux unrun |
| S9c path-exec/file and directory replacement | `s9c_bound_object_survives_file_and_directory_swap` | Linux unrun |
| S10 no PATH consultation | `s10_path_is_irrelevant` (isolated child PATH contains only owned fake) | Linux unrun |
| S11 observed NNP vs hard-coded true | `s11_nnp_parser`, `s11_nnp_observed_in_child`; explicit inherited-NNP skip | Linux unrun |
| macOS null/unknown, no probe | `s11_non_linux_has_no_probe` | macOS unrun |
| S12 constants and cfg(test) seams | `s12_production_constants`; `s12_e4_portable_production_pins` | Native unrun; portable PASS |
| S13 descriptor-only exec and privacy bypass controls | `s13_e4_tokenized_source_and_bypass_controls`, helper inventory | Portable PASS |

Mutation rows describe intended discrimination, **not mutation kills**. None of
E4's native mutants was run. New ordinary Linux fixture execution belongs on a
future hosted ordinary-UID Linux runner; root is explicitly skipped. Linux
Clippy/tests, macOS checks, hosted required checks and fresh Opus high review
remain blockers. No CI workflow or required-check change is included here.

## Delivery limits

Local inactive E4 source is prepared, not accepted for publication. No push,
PR creation, remote CI run/watch/merge/settings change or babysitter change was
made. No E6/E7 production wiring or activation was added. Production remains
MechanismUnavailable; M2 remains incomplete and native paths remain
**unverified on real hardware**. E_PROTECTION and ExecutionFailed diagnosis
remain unknown, with no experiment retry. The known unrelated privacy flake
remains unresolved; this one green Windows run does not resolve it.

## First independent-review correction

Opus high review cf5f282 (retained41bb2bb) required Linux compile/lint B1
and unfenced integration-probe B2 repairs. Sol medium source correction
`cf8b9e795789f517ea65ec99c8fa79e574d4880c` casts both execveat pointer arrays to the pinned libc
signature, derives empty production Settings Default while retaining test
defaults, and removes the needless helper return. These are source corrections,
not a real Linux compile result.

The S6 integration command now sets PR_SET_NO_NEW_PRIVS in Linux pre_exec
and aborts spawn if setting it fails. The output must report no_new_privs:true
and sudo must not be exit_zero. Non-Linux output must be null/unknown.
No unfenced system probe was run. The test module is out-of-line through a
path attribute, so cargo fmt follows it; all formerly hidden hunks are formatted.
A deliberate misformat in that file now fails fmt and exact bytes were restored.

Windows fmt and locked workspace/all-target Clippy pass. Focused portable
selection passed8/8 (2d1d0b7b-af1b-4fa1-9d59-07ce6c8bfb8f). No full-suite
repeat for these corrections; no Linux runtime/native mutation credit.
Independent Opus recheck, real Linux/macOS compilation and native tests remain
required before publication/merge. Production remains inactive.

Updated correction manifest (committed bytes):

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/unix_shim_rights_linux.rs` | `080778591d6744018015cae1ab9457a58a028c0e` | `2eb9036e2d6800d1376e3fe7bfce9f88cb1d8e38e69ddca8bd2a7de1164e3efd` |
| `crates/arxd/src/unix_shim_rights_linux_tests.rs` | `32d55ad9354e7cf515920b4d975866de77f46a6c` | `ba73cff90373c19929aea9dd732a667af9b5e6dc3909b32641d0b9fc20517024` |
| `crates/arxd/tests/unix_shim.rs` | `a26e24c6f0d62c18ffc4c727046e099b5f489329` | `83dbfc32895704f4f6578755878550cc64c8681e2dde3cb638986f5953160297` |

## Publication checkpoint and Unicode fixture restoration

Opus high recheck063c4bf (retained737d623) closes B1/B2 and rustfmt coverage.
N1 was corrected in87ef559: both S1 Unicode literals are restored to their
original single-encoded character. No new production change. Fmt passed.
The root8-test selection was exactly:
`test(unix_shim) | test(s13_) | test(e4_) | test(p_scan_)`; it includes
`s5_injected_identity_refuses_root`, which accounts for the eighth test
beyond the reviewer's narrower seven. Neither run is Linux evidence.

Main af9289a has the same tree as reviewed E3 parent231c367, and the
local38b207d evidence record is retained. No new full-suite run is claimed
for corrections/reports. Exact-head integration review, real Linux/macOS
compilation/runtime and native mutation kills remain required before merge.
Production remains MechanismUnavailable, M2 incomplete.

Restored integration fixture (committed bytes):
- Git blob `4108b5c47238e5ef9be9e6c23dad1351cda90c9f`; SHA-256 `20d0f61736d7d57c973ad19a9b027fa7d38bd05e45857c3dcee199570d905b7a`.
