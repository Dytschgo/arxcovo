# A4 populated-leaf fixture: unpublished source preparation

2026-11-15; OpenAI / gpt-6.1-sol / high, implementation sub-agent.
Base `240fce1`, dependent on reviewed PR104 helper source; fixture source
commit `ec350aa`. Source preparation follows merged PR102
`fa416937d49964ffa2491fc99d66bc0890ea6ee6`. Last merged PR remains #103
`ecc926b3db9423630e9f6194c20aa135238ca4c6`.

This is local source/evidence only. No push, publication, review delegation,
driver selection/delivery, dispatch, owner operation, network request or native
fixture/helper execution occurred. Exact-head independent review and normal
gates remain required. A4, M2, full F/G and real-Runner PA6 remain open.
Hardware/native paths are **unverified on real hardware**.

## Contract and source

Read AGENTS/KICKOFF, ADR0020 D9a (all five bindings), A5 seeder/fixture/model,
accepted E4 fork/execveat source, helper README/source/build evidence and
[independent Muse source review](m2-a4-fixed-helper-muse-review.md).
The existing E4 parent-image atfork exposure basis is reused as accepted by
D9a; this slice makes no new linked-runtime closure claim.

- `hierarchy/linux.rs`: cfg(test) `seed_a4` reuses A5's Created-only runs,
  exclusive literal `arx-0-0`, trusted no-follow handles and empty observations.
  Test-only controller preparation enables unit then runs, writes and reads
  back 67108864 / 0 / 1 limits before placement. `place_a4` reopens the literal
  leaf and compares device/inode to the retained handle before the PID write.
  Neither method writes `cgroup.kill` or removes an object.
- `disposable/a4.rs`: the sole native entry is ignored test
  `exec::disposable::a4::disposable_linux_a4`. It validates a fixed staged
  `/usr/libexec/arxcovo/a4-helper` via trusted component traversal, no-follow
  regular executable descriptor, size/hash and single link. Absence or mismatch
  fails before seeding or spawning. No PATH/environment override or build fallback.
- Parent queries inherited ignored SIGPIPE without changing its disposition;
  private pipe2 endpoints are nonblocking/CLOEXEC. A bounded parent FD inventory
  requires unrelated descriptors CLOEXEC; exactly the two child handshake ends
  are made inheritable around fork and restored in the parent. Stdio uses the
  verified root-owned character device 1:3 at `/dev/null`.
- The original monotonic deadline is computed before fork, plus 15 seconds,
  and travels as bounded decimal argv with the two FD numbers and empty env.
  The child branch uses the E4 prepared-pointer dup2/execveat/write/_exit shape;
  there is no Rust child wait, executable re-entry or runtime closure audit.
  Only the standalone image waits after exec.
- Parent retains owned child PID/reap responsibility, requires the R handshake,
  then places it. Before release and the single setup call it checks trusted
  sole leaf structure, limits, population, empty runs procs, sole parent in
  supervisor, live child, one actual numeric procfs task and leaf procs equal
  to that child; more than six seconds must remain both before and after G.
- `hierarchy/a4.rs` is reachable only through cfg(test) fake. Its shared parent
  orchestration calls unchanged A5 finish once: production setup, exactly one
  removal, runs_created=false, unit/runs_written=true, fresh S9 and literal
  absence. The native child reap then requires SIGKILL before original expiry.
  Success has no fixture kill. Drop cleanup signals only an unreaped owned
  child after failure, with ECHILD clearing ownership. READY/identity hold
  follows all postconditions. Driver's existing capture bounds are unchanged.
- Inactive inventory registers both test-only module declarations and the native
  test source. Historical A1/A2/A3/A5 fixture assertions are unchanged. Fake
  kernel model adds only bounded fixed-limit writes after controller exposure.
  No production, helper-owned, Cargo/package, ADR, workflow or driver file changed.

## Pins and exact deferred refresh

The fixture uses the current reviewed **Windows-to-Linux cross-image**:
source SHA-256 `770ab397264ab78deb5d54854bd00d4cfabb1b3660b0e975ee751472fbeada5a`,
image size 3696, image SHA-256
`fc814889c294db4216093e8f360201401be2c1b2dc6c4b01fd84617701f3a92e`.
Full compiler/linker/flags/symbol identity and compile-only recipe remain in
[helper evidence](m2-a4-helper-source-build-evidence.md) and unchanged PR104
helper files. This slice neither rebuilt nor launched that image. There is no
invented Linux-host recipe/pin and no delivered image. Future delivery must
stage exactly this reviewed image or obtain a separately reviewed replacement.

`tools/ci/e3-native/driver.py::SOURCE_SHA` remains deliberately unchanged at
`af82315a49d97d090086d2ef1cc0fe7069be8894ad597f70b3238f7c0b6e9479`.
After integration review its LinuxFs source pin needs the exact LF git-blob
SHA-256 `75b174ac10c7c3dd3be5984f507ba891ae4dccd953c49dfb12327007c0b907bd`
for this source commit. This slice does not refresh or bypass the guard.
S2b pinned source files are untouched. A4 remains outside the driver closed
mapping; helper delivery and native authorization are separate future work.

## Audited checks actually run

Before checks, traced test callers: new ordinary tests reach only mock ChildOps,
Fake/Manual, pure hashing/parsing and embedded source bytes. Native methods are
reachable only from the ignored Linux A4 fixture; no ordinary test calls them.

- `cargo test -p arxd --locked --offline --lib exec::hierarchy::fake::a4::`:
  6 passed. Covers source/image pin agreement and mismatch/size refusal,
  bounded single-task inventory and original deadline rejection, ordered
  controller/limit preparation with failed/wrong/oversized readback, populated
  S7 cleanup and one removal, 15 precondition refusals, absent kill effect,
  fresh residue and non-SIGKILL refusal. These are model assertions, not native
  kill attribution or a compiled native omitted-kill mutant run.
- Final pure model regression filter:
  `cargo test -p arxd --locked --offline --lib exec::hierarchy::fake:: -- --skip h15_panic_payload_subprocess`:
  37 passed, including existing A1/A2/A5. An earlier 38-test model filter also
  passed and included the existing Windows subprocess-only panic test; that
  subprocess exercises the fake model, not a Linux helper/native image.
- Exact `exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory`:
  1 passed. `cargo fmt --all -- --check`, `git diff --check` passed.
- `cargo clippy -p arxd --locked --offline --all-targets -- -D warnings`:
  passed on Windows. Initial pure run caught missing fake controller exposure;
  initial Clippy caught two boolean simplifications. Both were corrected and
  their relevant checks rerun successfully.
- Attempted `cargo check -p arxd --locked --offline --tests --target x86_64-unknown-linux-gnu`:
  blocked in bundled libsqlite3-sys before Arxd type checking: missing
  `x86_64-linux-gnu-gcc`. No installation or workaround attempted. New Linux
  adapter/spawn source has **not** been Linux type-checked, linked or run here.

No native acceptance, installed service behavior or hardware claim follows.

## Coordinator integration

PR104 merged as c87387ea6d5680b9347d43f88b8d280884d26353 at
2026-11-14T23:46:15Z after all five gates; squash tree equals reviewed 240fce1.
The coordinator refreshed only E3 SOURCE_SHA to the exact committed LF hash
75b174ac10c7c3dd3be5984f507ba891ae4dccd953c49dfb12327007c0b907bd.
Other pins, manifest, mutation/restoration guards and workflows are unchanged.
Portable python -I -B tools/ci/e3-native/test_driver.py: 25 tests,
one skip, zero failures. This does not execute the A4 helper or Linux fixture.
Fresh independent source review remains required before publication.
