# Stage F A3 driver source preparation

2026-11-14; OpenAI / gpt-6.1-sol / high; implementation sub-agent.
Source: `9a78227ba0af7901425dc8c4c614ac72cb5ac9cb`.
Contract: plan `6306d54` retained `8c81a45`; independent Muse plan review
`89689b9` retained `b37bc517072c07d06a8017b7e2d21f026179146a`.
Both original/retained document diffs checked empty.

Three closed TEST values derive profile presence; A1/A2 remains the source default.
A3 was selected only in in-memory mocks, never in the source constant or execution.
Runtime copies preserve packaged bytes, 0600 files and the 0700 drop-in directory.
No weakening drop-in, alternate fixture name, alias or independent profile switch.
The expected systemctl `DropInPaths` representation is exactly (no brackets,
quotes, leading/trailing whitespace or additional paths):

- A1/A2 and A3 Standard: `/run/systemd/system/arxd.service.d/50-arxcovo-standard-rights.conf /run/systemd/system/arxd.service.d/90-disposable-a1a2.conf`
- A3 None: `/run/systemd/system/arxd.service.d/90-disposable-a1a2.conf`

Attribution checks exact manager identity, file sets, bytes and modes before start,
while held and before owned cleanup. The monitor also checks the sole supervisor
PID before ack. Existing prebuilt libtest, protected staging, execve/capture,
reservation, collision and cleanup lifetime remain. Rust callers retain
postconditions -> READY -> extracted hold-only helper; A3 adds no setup.
F0/F0b edits only mock the new attribution read boundary; historical assertions,
feedback schema, bounds, stop-finally snapshot and replay evidence are unchanged.

Checks actually run on Windows:

- `python tools/unix-exec-validation/mutation_check.py`: 72 audited pure/mock
  tests passed; 23 Python assertion kills; three exact pure Rust fake-port tests
  passed and four compiled Rust assertion kills. All compiler/error outcomes
  reject credit; in-memory mutants leave source hashes unchanged. The generated
  Rust harness excludes native adapters and uses individual `--exact` filters.
- `python tools/unix-exec-validation/test_f0_blockers.py --mutants`: 23 assertion
  kills, source hash unchanged.
- `python tools/unix-exec-validation/test_f0b_transport.py --mutants`: 36 assertion
  kills, four explicitly equivalent/no-credit edits, source hash unchanged.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::parse::tests::<name> -- --exact`:
  one pass each for `a3_packaged_profile_status_fixtures`, `p6_exact_hardening`,
  `bounded_valid_inputs_and_total_byte_data`. Read call graphs are pure parsers
  and compile-time package text; no native observations.
- `cargo clippy -p arxd --tests --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.

No ignored fixture, native API, product CLI, service/account/owner/root probe,
network, GitHub, CI or dispatch operation ran. The Linux-only ignored fixture
is not compiled by Windows checks: Linux compilation/execution remains unverified.
No PROGRESS, ADR, workflow, Cargo, package or production source changes.
Fresh independent Muse source review remains required; no delegation was allowed
in this slice. No execution disposition or acceptance credit is granted.
A3, remaining F/G, M2 and real-Runner PA6 remain open;
**unverified on real hardware**.
