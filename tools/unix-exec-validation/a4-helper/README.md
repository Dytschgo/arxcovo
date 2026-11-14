# Fixed A4 waiting helper — source/build preparation only

This directory is outside Cargo and is not a tool-discovery asset. Nothing in
the driver, selectors, workflows, package or production code references it.
Binding: ADR0020 D9a binding 1, effective with PR102 merge
`fa416937d49964ffa2491fc99d66bc0890ea6ee6`. No fixture spawn is implemented here.

`helper.rs` is the entire executable source. It uses `no_std`, `no_main`, one
assembly entry and direct x86_64 Linux syscalls. No libc, startup library,
allocator, threads, fork, exec, atfork handlers, commands, network, storage,
probes, environment reads or logging are used. It only operates on the two
provided descriptors and the monotonic clock. It cannot exit successfully.

Protocol (fixed, no options): argv[1..4] is ready-write FD, release-read FD,
absolute CLOCK_MONOTONIC deadline in nanoseconds. Exactly three arguments;
canonical unsigned ASCII decimal, at most 19 digits, no signs/whitespace/
leading zeroes. FDs are distinct, 3 through INT_MAX. Deadline is 1 through
INT64_MAX; entry refuses an expired deadline or more than 15 seconds remaining.
The parent must compute the original deadline before spawn; this entry check
does not prove when the parent started its clock.

The helper polls writable, sends exactly `R`, closes ready, polls readable,
requires exactly `G`, closes release, then polls with zero descriptors until
the **same** deadline. Timeout rounding is upward to milliseconds, saturating
and never infinite. Poll timeouts recheck the clock. Only `-EINTR` retries
poll/read/write, each returning through a fresh monotonic remainder; no retry
rebases the deadline. Regressing/invalid/failed clocks refuse. clock_gettime
failure, including unexpected EINTR, refuses rather than retrying without a
fresh clock. Linux close errors refuse without retry (avoids closing a reused
FD). EOF, wrong bytes, HUP/ERR/NVAL, unexpected readiness and other errors fail.
Exit codes: 64 argv, 65 clock, 66 expiry/invalid lifetime, 67 I/O, 68 protocol,
69 panic guard. Deadline expiry is always failure. Scheduling/kernel delays
are not a hard physical-time guarantee.

Future fixture must supply private, nonblocking pipe endpoints (no competing
reader/writer), an inherited ignored SIGPIPE disposition so EPIPE reaches the
fixed error exit, `/dev/null` stdio and exactly these two surviving handshake
FDs; unrelated FDs CLOEXEC. These are spawn preconditions, not helper probes.
Parent keeps opposite endpoints and child identity. After migration it must
verify one task/live membership/population and every D9a precondition before
release, with >6 seconds remaining. S7 alone kills on success; parent must
prove SIGKILL by waitpid. All placement, limits, ownership and failure cleanup
are **pending fixture work**, not established by this source.

## Exact compile-only recipe

Prerequisites already installed for the recorded build: Rust 1.95.0,
`x86_64-unknown-linux-gnu` target and bundled rust-lld. No installation step.
From repository root:

```text
python tools/unix-exec-validation/a4-helper/build.py build
python tools/unix-exec-validation/a4-helper/build.py verify
```

`recipe.json` pins source SHA-256, full compiler identity and every flag.
`build.py` fixes the source/output paths, rejects linked/reparse paths, bounds
reads to 1 MiB and compiler invocations to 60 seconds, resolves only the pinned
toolchain's bundled linker and never executes the image. Its compiler argv is:

```text
rustc +1.95.0 <recipe.flags verbatim> -C linker=<sysroot>/lib/rustlib/<compiler-host>/bin/rust-lld[.exe] helper.rs -o out/a4-helper
```

The build runs in this directory. `build` writes only an ignored candidate pin;
it never updates the reviewed `image-pin.json`. `verify` first checks source,
flags and compiler identity, then compares installed compiler/linker hashes,
image SHA-256, size and the complete symbol inventory to that independent pin.
An image hash cannot authorize unknown source. The bounded ELF reader rejects
dynamic loader/linking, relocations, TLS, init/fini arrays, writable executable
segments, undefined/forbidden symbols and an entry other than recorded _start.
Symbol names alone are not a proof of behavior; the single source plus fixed
compiler/linker and artifact identity supply the reviewable closure.

`image-pin.json` records a real **Windows-to-Linux cross-compile**, not a Linux
hosted build or execution. Compiler host is explicitly Windows. No binary is
committed or distributed. A Linux-host compiler has a different identity and
must receive a separately reviewed recipe/pin, not overwrite this one or
pretend its output is identical. Artifact delivery and descriptor-bound
SHA-256/size verification before spawn remain later fixture work.

## Pure checks

```text
rustfmt --edition 2024 --check tools/unix-exec-validation/a4-helper/helper.rs tools/unix-exec-validation/a4-helper/tests.rs
rustc --edition=2024 --test -D warnings tools/unix-exec-validation/a4-helper/helper.rs -o tools/unix-exec-validation/a4-helper/out/pure-tests.exe
tools/unix-exec-validation/a4-helper/out/pure-tests.exe
python -m unittest discover -s tools/unix-exec-validation/a4-helper -p test_build.py -v
```

The test build excludes the **entire native module**, including entry/syscalls.
Its mock drives the same parser/deadline/handshake implementation; it never
launches a waiting helper, fork, adapter or cgroup. Python uses synthetic ELF
bytes and mocks. `out/` is ignored; build it before the rustc test command.
These checks grant no A4, M2, full F/G or PA6 acceptance or native authority.
Hardware paths remain **unverified on real hardware**.
