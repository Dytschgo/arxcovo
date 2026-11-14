# A4 fixed helper source/build slice: independent Muse review

2026-11-15; Muse Spark (Muse review sub-agent). Independent review of the
fixed-helper source/build slice against base `fa41693` (accepted PR102).

- Scope: `git diff fa416937d49964ffa2491fc99d66bc0890ea6ee6...f54eada`
  (10 files, +1053/−0): 8 files under
  `tools/unix-exec-validation/a4-helper/` (commit `0c27222`), plus evidence
  [m2-a4-helper-source-build-evidence.md](m2-a4-helper-source-build-evidence.md)
  and a 2-line `PROGRESS.md` entry (commit `f54eada`).
- Base/head pinned: base `fa416937d49964ffa2491fc99d66bc0890ea6ee6`,
  head `f54eada5fe9f7f5edb92b953839b3cc18e`. Working tree was clean
  before and after this review; this review adds only this file.
- Spec: ADR0020 §D9a binding 1 (standalone prebuilt helper clause), the
  [runtime-blocker resolution](m2-a4-runtime-blocker-resolution.md), and the
  prior independent [Sol review](m2-a4-runtime-resolution-sol-review.md).
- Method: full read of `helper.rs` (250 lines incl. native module),
  `tests.rs`, `build.py`, `test_build.py`, `recipe.json`, `image-pin.json`,
  `README.md`, `.gitignore`, and the evidence doc; independent re-run of all
  permitted pure checks plus a compile-only build/verify reproduction.
  The Linux image was never executed; no fixture/spawn/native run occurred.

## Standards — PASS, no findings

- One-concern small commits with conventional subjects (`test(a4):`,
  `docs(a4):`); docs-only evidence kept separate from the source commit.
  No production, driver, selector, workflow, Cargo/package, ADR, or
  `PROGRESS.md` (beyond the slice entry) changes: `git diff --stat` shows
  only the 10 in-scope files, and no `a4-helper` reference exists anywhere
  outside its own directory, so nothing is wired into discovery, selection,
  or Cargo.
- Every behavior ships with a test: 7 pure Rust tests drive the shared
  parser/deadline/handshake logic through a mock `Ops` seam with the
  **entire native module compiled out** (`#![cfg_attr(test, ...)]` plus
  `#[cfg(test)] mod tests`), and 7 Python tests cover the ELF inventory
  rejections and the pin-ordering refusals. `rustfmt --check`,
  `rustc --test -D warnings`, and `python -m unittest` all re-ran green
  in this review (7 passed / 7 passed).
- Fowler smell baseline: no finding. Names are literal (`remaining`,
  `wait`, `run`, `inventory`, `regular`); the shared Rust handshake prefix
  is extracted once (`prefix()`); no speculative abstraction (fixed argv,
  fixed flags, fixed exit codes); no message chains, middle men, or
  shotgun edits (all helper logic lives in the one source file, all build
  logic in one script).

## Spec — PASS, no findings (D9a binding 1, source/build slice only)

Verified clause by clause against the amended D9a binding 1 text:

- Tiny `no_std`/`no_main` x86_64 image outside Cargo, entered by exec:
  confirmed. `global_asm` `_start` (`mov rdi, rsp; and rsp, -16; call
  entry; ud2`), raw stack parse (`argc == 4`, `argv[1..4]`, bounded
  20-byte NUL scan, no environment access). No `Cargo.toml` in the
  directory; no libc, allocator, TLS, or startup library.
- Only direct async-signal-safe syscalls `close`/`clock_gettime`/`poll`/
  `read`/`write`/`_exit` (numbers 3/228/7/0/1/60 on the raw `syscall`
  ABI, negative errno returned directly, `nostack` clobbers only
  `rcx`/`r11`): full native entry graph audited — `syscall`, `exit`,
  `Native::now/poll/write/read/close`, `arg`, `entry`, `panic` guard
  (`exit(69)`). No `fork`/`clone`/`execve`, no thread, no atfork
  registration, no commands, no network/storage/probes, no env reads.
- Strict args/FDs/deadline/handshake: `decimal` (canonical ASCII,
  ≤19 digits, no signs/whitespace/leading zeroes, checked arithmetic
  against caller max); `arguments` (exactly 3 args, FDs distinct and
  ≥3, deadline 1..=`i64::MAX`); entry refuses expired deadline or more
  than 15 s (`LIFE = 15_000_000_000` ns) remaining — the original 15 s
  bound preserved.
- Constant failure exits, no success exit: 64 argv / 65 clock /
  66 expiry-or-invalid-lifetime / 67 I/O / 68 protocol / 69 panic guard;
  `Ok(())` from `run` exits 68 (`PROTOCOL`). Expiry is always failure.
- EINTR-only retries with recomputed remainder: `wait` and the
  read/write loops retry only `EINTR` (`-4`); every retry passes through
  `remaining`, which re-reads the monotonic clock and returns a ceiling,
  saturating, never-negative, never-infinite millisecond timeout against
  the **same** absolute deadline. Timeout/spurious returns recheck the
  clock; close is never retried (avoids closing a reused FD); a
  regressing/invalid/failed clock refuses fail-closed. Nonblocking-pipe
  and ignored-`SIGPIPE` dispositions are correctly stated as *future
  fixture preconditions*, not helper probes.
- Build/source/linker/hash/size pins plus exact recipe: `recipe.json`
  pins source SHA-256, full compiler identity, and every flag;
  `build.py` refuses unreviewed source/flags **before** any image-pin
  read (covered by a pure mock test), resolves only the pinned
  toolchain's bundled `rust-lld`, rejects linked/reparse paths, bounds
  reads to 1 MiB and compiler invocations to 60 s, and never executes
  the image. The ELF reader is bounded (no loader) and rejects dynamic/
  interpreter/TLS, writable-executable segments, relocations, init/fini,
  undefined symbols, forbidden substrings (`fork`/`exec`/`thread`/
  `atfork`/`malloc`/`tls`), and any entry other than recorded `_start`.
  The residual "symbol names are not behavior proof" limit is openly
  documented; closure rests on single source + fixed compiler/linker +
  artifact identity, which is the accepted bounded tradeoff, not a gap.
- Cross-compiled Linux image, twice reproduced, never run: `image-pin.json`
  records host `x86_64-pc-windows-msvc` explicitly — no fabricated
  Linux-host claim. Image `3696` bytes, SHA-256
  `fc814889c294db4216093e8f360201401be2c1b2dc6c4b01fd84617701f3a92e`,
  10 defined symbols, no undefined symbols. This review independently
  reproduced it a third time on this host: `build.py build` then
  `build.py verify` both PASS ("fixed source/compiler/flags/image/size/
  symbol pin verified; image not run"). No Linux-host pin is claimed;
  that reproduction remains pending per the evidence boundary.
- Slice boundaries honored: no fixture spawn/descriptor-bound check,
  no FD-inheritance wiring, no placement/task check, no S7 ownership, no
  native assertion, no helper launch. `out/` stays ignored; no binary
  committed. No private filesystem paths in either pin file (hashes and
  the compiler identity string only; sysroot-derived linker path is
  resolved at run time, never logged or pinned).

## Security — PASS, no blockers

- MCP argument/path hygiene: `build.py` validates inputs, uses fixed
  argv with no shell, `lstat`-based link/reparse rejection over the path
  and all parents, bounded reads/slices, and fail-closed refusal
  (`UnicodeDecodeError` is a `ValueError` subclass, so non-ASCII symbol
  names also refuse rather than bypass). `struct.error`/`KeyError`/
  `StopIteration` all map to refusal, never to a default-allow.
- No secrets or owner-private data in source, pins, logs, or MCP traffic:
  pins carry only hashes, flags, sizes, and symbol tables.
- No new execution route: nothing outside the helper directory references
  it; the descriptor-bound spawn, S7 kill attribution, placement, and all
  D9a bindings 2–5 remain future fixture work, correctly marked pending.
- No speculative audit infrastructure and no extra approvals: the review
  accepts the resolution's stated tradeoff (E4 spawn-window basis
  disclosed, not re-proven) and adds none.

## Actionable findings

None. No Standards, Spec, or Security change is requested before merge.
Permitted next scope is unchanged: coordinator merge through normal gates,
then fixture slices under the same D9a bindings with a separately
authorized bounded disposable-host run.

A4, M2, full F/G, and real-Runner PA6 remain open. Native/hardware paths
are **unverified on real hardware**.

Standards: 0 findings. Spec: 0 findings. Security: 0 blockers.
