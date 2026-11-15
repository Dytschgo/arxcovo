# Dormant A4 driver and fixed helper delivery source

2026-11-15; OpenAI gpt-6.1-sol high implementation sub-agent. Last accepted
fixture merge: PR105 `fafb8d4f4bd460eff51328d781966d2e57f0fa22`;
working base `f86552d`. Read AGENTS/KICKOFF, ADR0020 D9a, helper README,
recipe/pin/source-build evidence and independent helper/fixture reviews,
native fixture helper admission, existing driver and F0/F0b guards/mocks.
No subagents, external calls, dispatch, publication or push.

The sole additional closed selector is
`exec::disposable::a4::disposable_linux_a4`, paired with Standard. Actual TEST
remains `exec::disposable::disposable_linux_a5`. The old A1/A2, A3 and A5
contracts remain covered. Workflow, Cargo/package, ADR, production and Rust
fixture/helper source are unchanged. No generic launcher is introduced.

Before reservation or privileged mutation, the A4-only branch reads fixed
checkout assets through root-down no-follow descriptors with bounded reads,
regular-file and single-link checks. Independently fixed source, recipe and
reviewed pin hashes precede Linux provenance and image admission. The image
must be exactly 3696 bytes, SHA-256
`fc814889c294db4216093e8f360201401be2c1b2dc6c4b01fd84617701f3a92e`,
matching the ignored fixture's held-descriptor pin. Bytes are retained in
memory for delivery; a path or environment value cannot select an executable.

Delivery checks root-owned, unwritable-to-group/other directory ancestry from
`/` through `usr/libexec`, then exclusively creates `arxcovo` and `a4-helper`.
Any existing arxcovo entry refuses without adoption or deletion. It retains
parent/directory descriptors and inode identities, checks root-owned 0755
regular single-link output and hashes the staged held descriptor. Short writes
refuse. Cleanup requires unit stop/cgroup disappearance first; replacement,
extra contents or uncaptured created identity quarantine. Only owned entries
are unlinked relative to retained descriptors. Existing protected sites,
schema/output/timeout/unit-profile/collision/runtime cleanup fences remain.

`build.py linux-candidate` is compile-only and rejects root before any compiler
or source access (the original build also rejects root). It uses unchanged
source/flags and exact Rust version identity with Linux host substituted,
records actual compiler/linker hashes and full provenance in the ignored
candidate file, then compares source/flags/image/size/symbols to the independently
reviewed pin. It never overwrites recipe.json or image-pin.json. The driver
requires this Linux record, including exact reviewed image/symbol/source/flags
identity; observed compiler hashes are evidence fields, never admission pins.
The existing Cargo harness build sequence is unchanged; no build or compiler
was actually invoked in this task. No helper is compiled as root by new tooling.

## Checks actually run

All execution below is Python pure/mock only. Exact commands from repo root:

```
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import mutation_check as m; m.main()"
python -I -B -c "import sys,unittest; sys.path.insert(0,'tools/unix-exec-validation/a4-helper'); import test_build; r=unittest.TextTestRunner().run(unittest.defaultTestLoader.loadTestsFromModule(test_build)); sys.exit(not r.wasSuccessful())"
python -I -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import test_f0_blockers as a, test_f0b_transport as b; a.mutations(); b.mutations()"
git diff --check
```

- Audited baseline: **89 tests passed**, including 13 A4 tests. Driver mutation
  checks: **36 assertion kills** (29 retained, seven A4). Compilation failures
  and test errors receive no kill credit; source hash unchanged
  `fa0961772c0a976ba87843a02d985d4314525c2098a01f469513cde83ac1add1`.
- Helper tooling: **nine tests passed**, using synthetic ELF/bytes and mocked
  compiler and filesystem operations. Both matching and differing synthetic
  Linux candidates tested; root refusal precedes compiler/source access.
- Retained F0: **23 assertion kills**. F0b: **36 assertion kills**, four
  equivalents without kill credit. Max measured internal/outer lines 757/2447.
- Diff whitespace check clean. Initial test iterations exposed missing Windows
  fchmod mocking, listdir side-effect modelling, stale mutation anchors and
  error/redundant-check masking in new mutation tests. Corrected mocks/assertions
  produced final green runs; failed attempts earned no credit.

## Pending boundary

This Windows session did not perform a Linux-host build or any compile.
No Linux pin is fabricated. Actual installed Linux compiler/linker identities,
source/flags record and candidate equality to the reviewed image remain pending.
If Linux output differs, delivery must refuse; stop for one independently
reviewed, narrowly scoped recipe correction rather than repinning an unknown
image or changing Rust fixture/helper source in this slice.

Independent other-provider source/security review is pending with coordinator;
none was started here per task instruction. No privileged filesystem mutation,
helper/image execution, ignored test, fork, proc/cgroup/systemd/sudo, owner action
or toolchain installation occurred. D7d remains consumed; no native authority,
A4/M2/full F/G/PA6 acceptance follows. Hardware paths are **unverified on real hardware**.

## Coordinator hosted compile-only gate

To resolve pending Linux-host image equality using ordinary hosted gates, the
coordinator adds one Linux-only verify step: nine mocked helper-tool tests, then
build.py linux-candidate as the standard runner account. This only compiles and
compares source/flags/image/size/symbols to the existing reviewed image pin.
No image execution, artifact publication, owner/provisioning action, native
selector, permission, timeout or required-check change. The image pin is not
overwritten; a differing output fails the step. Actual Linux result remains
pending this PR. This explicit CI delta supersedes the builder-only scope
statement above; it is included in the fresh independent review.

## First Linux compile-only refusal

PR106 run43657778405 job132640072365 passed109 pure guards and9
helper-tool tests, then refused at linux-candidate behind a generic fixed
message. The public rust-toolchain step matches the expected Rust1.95.0 commit
and LLVM22.1.2 identity; the candidate image is not credited. Coordinator adds
closed refusal labels only (no exception text/path/compiler output) to localize
the concrete failing gate on the next ordinary hosted build. No admission,
recipe/image pin or runtime behavior changes. Ten helper-tool tests pass,
including planted-private-marker suppression; diff checks pass. Pending hosted
result must be diagnosed before any repin or native disposition.

## Localized image comparison refusal

Headcf41d28 run43658154150 job132641266990 reports image-pin after
10 mocked helper-tool tests pass. This proves the Linux candidate reached
comparison and disagreed with at least one pinned image field; it does not
establish a correct replacement image. Coordinator adds a fixed closed
comparison report with five field-equality booleans, actual image hash/size,
symbol count and name-equality boolean. No paths, raw image, compiler output
or symbol names are emitted. Ten tool tests verify its schema and failure
behavior. Pin/recipe/flags/source and admission are unchanged; comparison
continues to fail on any mismatch. Assess the hosted report before a fix.

## Equal symbols, different file bytes

Headf4adf7c run43658446256 job132642186283 reports actual Linux image
cee895412d58718016dd0db9883526ef8e0b13f8ed00cf213c6f77536b206369,
3680 bytes, versus reviewed cross-image3696. Source,flags and full symbols
match; image hash/size differ. No replacement is admitted. Coordinator
rebuilds the Windows cross-image compile-only and reproduces fc814889...a92e.
Expected .comment is153bytes; executable .text is1332bytes SHA256
cc1a943206e25a739c49f02cf06a81ec7690e112578939891e2b281695cd812d.
Add bounded ELF section/header/load comparison facts to the failure report,
not admission. Load comparison masks only e_shoff (non-runtime section-table
location) and retains every other mapped byte/loader field. Expected entry
2102124; first mapped876bytes normalized SHA256
f784e75f73cc8661a0008502709a1e1da56a7afb1fc3472498383dab0c9074c2;
second mapped1332bytes has the .text hash above. Expected complete layout is
retained in ignored out/windows-layout.json. Ten mocked tests pass. Actual
Linux section/load comparison remains pending; no metadata-only conclusion
or image fix is claimed before that evidence. No helper image is executed.
