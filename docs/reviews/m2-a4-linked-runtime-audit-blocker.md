# A4 linked-runtime audit: source preparation blocked

2026-11-15; OpenAI / gpt-6.1-sol / high; implementation sub-agent.
Supplied head: `f230bc1edf3b46b89add068db4e9fb5d0bba803a`, containing merged
PR101 `9d5be079adc92c9426ed0f5054e2e0e964cfb9dd` (ancestor check passed).
Outcome: stopped before implementation at the explicitly required at-fork/errno
audit. This is an artifact gap, not a finding that fork is necessarily unsafe.

Read AGENTS/KICKOFF, brief0020 A4, ADR0020 D9a, the single-task helper proposal
and accepted Muse fork-contract recheck. The settled post-fork contract remains
unchanged. Historical re-entry blocker evidence is retained unchanged.

## Concrete evidence

- The checkout starts clean and has no `target` directory or linked Linux A4
  test executable. Tracked-file searches for atfork, libc, link-map and sysroot
  artifacts found none. Cargo.lock describes packages, not the selected linked
  objects, runtime libraries, registrations or signal dispositions.
- The installed toolchain directories are Windows-hosted `1.95.0` and `stable`.
  The pinned 1.95.0 toolchain **does** contain Linux cross-target libraries:
  `libtest-3710056540f41481.rlib`, `libstd-d1237ef7159db0a2.rlib` and
  `libstd-d1237ef7159db0a2.so`. Their presence does not identify the libraries
  linked into the absent test executable or its eventual loaded libc.
- The pinned toolchain has no `lib/rustlib/src/rust/library` source tree or
  `share/doc/rust/html/src` tree. Stable's local documentation reports Rust
  1.95.0, commit `59807616e1fa2540724bfbac14d7976d7e4a3860`, and includes
  `src/test/lib.rs.html`. Its HTML source inventory is not a complete matching
  Linux runtime implementation: the inspected `src/std/sys/pal/unix` directory
  is absent. No complete libtest/std/native-runtime closure was established.
- A recursive cached-registry source search for `pthread_atfork` and
  `__register_atfork` found libc declarations/re-exports and rand 0.8.x
  registrations among its results. The latter are **not** evidence of a linked
  registration: this lockfile contains rand 0.9.2 and 0.10.3. Neither positive
  cache hits nor absence of other literal hits establishes linked reachability.
- Locked `libc` 0.2.191 declares `pub fn __errno_location() -> *mut c_int` in
  `src/unix/linux_like/linux_l4re_shared.rs:1725`. This is an extern declaration,
  not the Linux libc accessor implementation. No matching Linux libc source,
  sysroot implementation or loaded-library identity was established from the
  available artifacts. Direct TLS access without locks/allocation is unproven.
- Locked `signal-hook-registry` 1.4.8 has a non-Windows handler with
  `ErrnoGuard::new()` at `src/lib.rs:390–391`; the guard saves/restores errno
  at lines 430–441. The handler also chains previous handlers and actions.
  This is partial preservation evidence only. It does not enumerate the
  handlers inherited at the proposed fork point, including libtest/std/native
  runtime handlers, or prove preservation for all of them.

Read-only checks used `git status --porcelain=v1`, `git rev-parse HEAD`,
`git merge-base --is-ancestor`, `git ls-files`, directory inventories,
`Test-Path`, `Get-Content` and `rg` over repository, installed toolchain and
cached registry sources. No binary was launched to inspect runtime behavior.
Unknown registrations and unknown accessor/handler safety remain unknown;
there is no empty-handler-set or blanket inactive-source exemption.

## Minimal other-provider handoff

Supply an offline, exact Linux build artifact bundle for the intended ignored
test binary: compiler/target identity, selected features and link inputs/map,
matching libtest/std source, transitive Rust/native source, and the identified
Linux libc/loader implementation (including `__errno_location`, fork machinery
and internal handlers). It must support tracing every reachable registration
and inherited signal handler at the fixture's fork point. Review the handler
bodies and errno preservation, rather than treating symbol absence as proof.

The smallest practical alternative is to place this unchanged source task with
the other provider where that Linux build/source bundle already exists. If a
linked artifact must be produced, separately authorize compile/link-only
preparation there; no ignored fixture launch or native cleanup experiment is
needed to obtain that artifact. This handoff neither delegates work nor grants
run authority. Retain the accepted single passive branch and all D9a bounds;
no new helper framework, architecture proposal or contract relaxation is needed.

## Validation and retained boundaries

`git diff --check` passed after this document was added. No source was changed;
no red/green tests, build, fmt, Clippy or mutants were run because the callgraph
gate was not established. No source acceptance or independent review is claimed.
Only this evidence document changes; PROGRESS and historical records are retained
under the task's one-evidence-document scope.

No helper/native ignored test launch, fork, cgroup/proc/systemd/sudo/owner
operation, network request, dispatch, publication or subagent action occurred.
Production, driver TEST selection, workflow, Cargo/package/policy and ADR files
are unchanged. Native A4 remains unrun and **unverified on real hardware**;
A4/M2/full F/G/real-Runner PA6 remain open pending evidence. No crash-lifecycle
or kill-attribution credit is granted.
