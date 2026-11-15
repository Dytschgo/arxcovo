# D9b controlled A7 policy fixture: source-only evidence

2026-11-15; OpenAI gpt-6.1-sol / high, implementation sub-agent.
Base checkout `9398f72` contains merged PR121 main
`d6986171d16077f05bf088f1054128ab94c1b887`. The supplied reviewed
`251c9c3`/tree `d4e4499` and five green gates are coordinator evidence, not
rerun here. Read AGENTS/KICKOFF, ADR0020 D9b, brief0020 A7, decision
`dec7f9b` with corrected case1 exclusion, independent Sol review, and delivery
contract amendment/four binding corrections before editing.

## Scope and call graph

Only the four source/test files below and this evidence file change. No
PROGRESS, ADR, brief, Cargo, helper, package, profile or workflow changes.
Before tests, traced `run_body`/runtime installation, dormant account preparation,
delivery/build/cleanup, `prepared_delivery`, both Runner admissions and legacy
qualification. The active driver remains the A4 omitted-kill selection; no
`run_body` edge, CLI/environment selector or service installation edge reaches
controlled staging/override. Portable tests use mocked effects only.

`sudo_nnp_accounts` shares the existing owned account/root lifecycle through a
bounded two-item generator. Only the legacy `prepare_sudo_nnp` wrapper retains
the dormant host-policy writer and visudo syntax check. The controlled preparer
calls fixture staging and shared accounts only; a transitive AST check and
poisoned legacy-writer mutation protect that separation. Existing account cleanup
still checks captured identities, stop confirmation, live UIDs and owned inodes.
Legacy policy-parent protection now uses the existing root-down nofollow opener;
its additional close is explicitly asserted by the partial-policy mock test.
The ordinary F0 protection-site inventory remains unchanged and passes.

Controlled preparation/staging remain behind `sudo_nnp_eligible(host, False)`
with actual `sudo_nnp_host_attested() == False`. Generated names and fixed stage
paths are validated before staging effects. Root-owned private `fx`/`sudoers.d`
directories are exclusive 0700; the four literal files are exclusive 0440,
root uid/gid, nofollow, regular, single-link. Each held descriptor and reopened
name binds dev/inode, mode/ownership and SHA256; exact directory entry sets and
literal hashes are rechecked. Partial/uncaptured creation or mismatched cleanup
retains the fixture; no guessed ownership or foreign deletion. Descriptor-backed
staging state stays with the caller on failure.

The only override additions are the exact four BindReadOnlyPaths mappings from
`<stage>/fx/{sudoers,sudo.conf,nsswitch.conf,sudoers.d}` to their `/etc` peers.
No optional source prefix or extra mapping is accepted. Templates are a single
includedir with no grants, exactly `10-a7-case0` granting only generated case0
NOPASSWD `/usr/bin/true`, explicit local policy plugin, and files-only
passwd/group/sudoers NSS. Case1 has no grant. No arbitrary input paths or shell.

Dormant test-only controller admission, before Runner construction and each
Runner invocation, reuses the fixed delivery admission and checks the effective
systemd mapping property (bounded 4096 bytes/5 seconds), bounded mountinfo,
exact read-only target mounts (including statvfs), source/view inode equality,
root ownership/modes, literal bytes and full include/source entry sets. It calls
the existing bounded libc NSS account/group lookup for both pinned accounts.
The pure mapping/byte/mount seams compile separately without native adapters.

The plugin path `/usr/libexec/sudo/sudoers.so` is a fixed future candidate only.
Installed C1 ELF, plugin digest/default-path equality, host floor and profile
proof remain **UNATTESTED**; no host hash is invented. Both controller profile
admissions remain `None`. The old injected policy collector remains false and
unchanged, with no dependency from the controlled path. Artifact hashes, same
libtest/controller copy checks, exact OwnerConfig/RunSpec, 20s/4096 bounds,
NNP/descriptor/identity and cleanup discriminators remain intact.

## Checks actually run (Windows, offline)

- `python -I -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`:
  PASS, 145 tests, including 9 controlled tests and unchanged ordinary guards.
- `python -I -m unittest discover -s tools/ci/e4-native -p 'test_*.py'`:
  PASS, 22 portable E4 tests; no E4 native command invoked.
- `python tools/unix-exec-validation/mutation_check.py`: PASS; existing 91-test
  audited baseline, 7 extracted Rust setup tests and prior assertion mutants;
  controlled 9-test baseline, 6 Python assertion mutants and 5 actual extracted
  Rust literal/mapping/read-only-mount assertion mutants killed. Source bytes
  unchanged. Covers missing/extra/optional masks, wrong case/rule/extra entry,
  read-only status, source inode, stop/partial cleanup and host-policy poison.
- `python -c "import sys; sys.path.insert(0, 'tools/unix-exec-validation'); import mutation_check as m; m.sudo_nnp_source_checks(); m.sudo_nnp_delivery_checks()"`:
  PASS. Existing A7/A11/S14 scalar and bounded record/RunResult predicates,
  wrong-identity/capture/cleanup mutants, 9 legacy account mocks, actual strict
  delivery schema/layout and binding mutants, 6 delivery mocks and copy/budget
  mutants pass. Only extracted pure Rust is compiled/executed.
- `python -I scripts/test-service-artifacts.py`: PASS, 5 tests, 2 platform skips.
- `cargo test -p arxd --lib --locked --offline --no-run`: PASS on Windows;
  supplies existing locked dependencies for the extracted pure schema checks.
- `cargo fmt --all --check`: PASS.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  PASS on Windows.
- `git diff --check` and UTF8/LF validation: PASS.

During implementation, tests caught portable path rendering, a Windows-only
mock attribute, an additional legacy descriptor close, and an old mutation scope
anchor after inserting the fixture functions. Corrected and reran the checks.
The mapping-count mutant initially survived because the test only duplicated a
known mapping; added an unrelated extra mapping discriminator, then killed it.
An initial isolated `python -I .../mutation_check.py` invocation could not import
its sibling module; the successful command above uses the script's existing
normal import contract. No failed invocation counts as a pass.

## Exact source blobs

| Path | Git blob | SHA256 |
| --- | --- | --- |
| `tools/unix-exec-validation/driver.py` | `7dbb9a9bedaa2f03f642326c08ce5608282f19a6` | `c8b0d7d79790eebc93f6610872610bba20d85e80bbdfdd30c8063980d7dce935` |
| `tools/unix-exec-validation/test_driver.py` | `9738eaa77270288d12374266b589f16952b3007b` | `e92543af45e63056c3cc9934e8f285bfdc3710a878232f40e37fdc31d6c42727` |
| `tools/unix-exec-validation/mutation_check.py` | `850ebbe29b244dbdb9f52553ce77ad4f164e6f3c` | `84fd4a2649709beb53bc11eb69fc4345cd10e38b05a8ff41c2a01d186e2115e8` |
| `crates/arxd/src/exec/disposable/sudo_nnp.rs` | `fbac1272104d84947bdb4062c40d5c8de3e8eecf` | `dd8e9c2ffbb9f088a9a1a0f4dd71efae0211878f3e46cdb15f1ea1d5e67fc0ee` |

The enclosing commit pins this evidence file; source hashes above avoid a
self-referential evidence digest. All five files are UTF8 with LF line endings.

## Limits and handoff

Linux-only integration remains **uncompiled locally**, including the actual
systemctl property rendering and native view adapters. No Linux toolchain probe
or installation was attempted. No actual view check, host/root-policy read,
/proc read, LinuxFs, ignored/native test, sudo, account, admin, network, install,
dispatch, variable, push, PR or subagent action ran. Tests that name such actions
mock them; extracted Rust ports do not link those adapters.

No current host is qualified, no TEST gate opened, no native grant exists.
A7/A11/M2/full F/G/real-Runner PA6 remain **unverified on real hardware**.
Fresh Muse exact-head source review is required before publication; this is an
implementation handoff, not independent review or native acceptance. Fresh-host
plugin/default-path/floor proof and a separately bounded native grant remain
required before any activation or actual host behavior can be claimed.

## Independent review and S1 resolution

Muse `bce660f` passed Standards/Security and found a fail-closed mapping-form concern. Coordinator checked primary systemd v254 parser/printer sources: default recursive config is printed with :rbind, matching the current Rust property predicate. No source or parser widening. [Resolution](../reviews/m2-a7-controlled-policy-mapping-resolution.md) retains actual-host rendering and all mount/profile/native gates as unverified. Reviewer reran ControlledPolicy9 and SudoNnp15 mocks green.
