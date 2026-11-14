# A4 standalone helper source/build preparation

2026-11-15; OpenAI gpt-6.1-sol high implementation sub-agent. Base accepted
PR102 `fa416937d49964ffa2491fc99d66bc0890ea6ee6`, clean worktree. Read
AGENTS/KICKOFF, ADR0020 D9a binding 1, runtime-blocker resolution and its
independent Sol review. This implements the small source/build slice of that
settled amendment, without another design amendment or parent-runtime audit.

Source/build commit: `59ccdfba62b85106337e4082b3197730102748d4`.

Source and invocation details: [helper README](../../tools/unix-exec-validation/a4-helper/README.md).
One standalone Rust no_std source outside Cargo, no runtime dependencies; raw
close/clock_gettime/poll/read/write/_exit only. Pure tests compile out all native
code. Fixed ready/release protocol, fixed failure codes, strict decimal argv,
original absolute 15-second lifetime, EINTR-only bounded retries and fail-closed
clock/error handling are implemented. There is no successful helper exit.

## Actual checks

- `rustfmt --edition 2024 --check` on helper.rs/tests.rs: PASS.
- `rustc --edition=2024 --test -D warnings ...helper.rs`: PASS; test executable
  run: **7 passed**, including 15,001 deadline boundary samples, strict grammar,
  retry remainder sequence, release retaining original deadline, wrong byte,
  EOF, EAGAIN/error/HUP, late readiness, clock regression and close failure.
- `python -m unittest discover -s tools/unix-exec-validation/a4-helper -p test_build.py -v`:
  **7 passed**, synthetic ELF and mocked compiler/source refusals only.
- `build.py build`: PASS, actual compile-only static x86_64 Linux ELF using
  **installed Windows-host Rust 1.95.0 and bundled rust-lld**, Linux target
  already installed. No install, network, Linux execution or waiting-helper launch.
- `build.py verify`: PASS, source/compiler/linker/flags/image/size/full symbol
  inventory match committed pin. No image self-authorization: unknown source
  refuses before any image pin read, covered by pure test.
- A second compile-only build plus verification reproduced the same image pin.
  `python -m py_compile` on build.py/test_build.py: PASS.
- `git diff --check`: PASS before source commit.

One initial pure-test compile refused under `-D warnings` because the native
argv-exit constant was unused in test mode. Restricting that constant to the
native configuration corrected it; final compile/tests above passed. No failed
native run occurred.

Actual source SHA-256:
`770ab397264ab78deb5d54854bd00d4cfabb1b3660b0e975ee751472fbeada5a`.
Actual image SHA-256:
`fc814889c294db4216093e8f360201401be2c1b2dc6c4b01fd84617701f3a92e`.
Image **3696 bytes**, **10 defined symbols**, no undefined symbols or dynamic
loader/TLS/init arrays. Complete compiler identity, compiler/linker hashes,
flags and symbol names/values/sizes are in
[image-pin.json](../../tools/unix-exec-validation/a4-helper/image-pin.json).
This is a real cross-compile pin; it is **not** a fabricated Linux-host pin.

## Pending boundary

No Linux-host build pin is claimed. Ordinary Linux-host compilation/reproduction
with its separately reviewed compiler identity/pin remains pending if that host
is selected for artifact delivery. Current cross-compiled artifact is ignored
locally, not published. Independent different-provider source/build review is
pending; this sub-agent does not merge or start reviewers.

No fixture spawn integration, descriptor-bound hash/size check, /dev/null wiring,
FD inheritance setup, placement/task check, S7 ownership or native assertion has
been implemented/exercised in this slice. Those later fixtures must establish
the documented endpoint/signal preconditions and all unchanged D9a bindings.
No driver TEST/selection/workflow/Cargo/package/ADR/production changes. No helper
launch, native adapter, cgroup, systemd, sudo, owner steps, dispatch, network,
publication, push or subagents. A4, M2, full F/G and PA6 remain open; no native
authority or acceptance is granted. Hardware paths **unverified on real hardware**.
