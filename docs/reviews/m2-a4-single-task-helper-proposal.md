# A4 single-task helper correction

2026-11-14. Sol medium coordinator; proposed contract correction, no source or run authority.

Blocker `d8e5332` shows that ordinary libtest re-entry has a harness thread plus a worker. Preserve pids.max=1 rather than treating migration above the task limit as proof. Replace only D9a's executable-re-entry requirement with a fixed, test-only post-fork waiting branch; no extra executable, Cargo target, production entry or restart harness.

The Linux manual documents that fork creates a single-thread child and restricts a multithreaded parent's child to async-signal-safe operations before exec. [fork(2)](https://man7.org/linux/man-pages/man2/fork.2.html). The permitted-function list includes clock_gettime, close, poll, read, write and _exit. [signal-safety(7)](https://man7.org/linux/man-pages/man7/signal-safety.7.html). These are design constraints, not evidence from a launched helper.

Proposed binding:

- The ignored parent fixture creates exactly one child with libc fork, after trusted seeding/controller limits and before migration. No Rust process API, atfork callbacks installed by this fixture, custom harness or child exec is added. Parent retains the PID and cleanup/reap ownership. The child creates no thread or process.
- Allocate all state, pipe objects and a CLOCK_MONOTONIC absolute deadline in the parent before fork. Child code is one audited divergent unsafe function using only fixed stack scalars, bounds-checked-by-construction arithmetic and direct async-signal-safe calls: close, clock_gettime, poll, read, write and _exit. No allocator, Rust I/O, formatting, logging, panic/unwind, destructor, lock, environment lookup or arbitrary callback is reachable after fork.
- Close known unused pipe/capture ends in the child. Inherited descriptors are never used except the fixed handshake endpoints. There is no exec or external channel; no descriptor value or host identity is logged. Child remains inside the owned unit throughout.
- Child sends one fixed ready byte, then waits on the release pipe with poll bounded by the original monotonic deadline. Parent migrates it to the already limited leaf and verifies one child task, live membership/population and all existing parent/supervisor conditions before releasing it. Child then only waits until that same deadline. Wrong bytes, EOF, unexpected errors or deadline expiry call _exit with fixed failure codes; EINTR retries recompute remaining time, never extend the deadline.
- All D9a limits remain: 64 MiB, swap 0, pids 1 before placement, absolute 15-second child deadline, more than six seconds remaining before the single production setup, five-second S7 bound, only S7 kill on success, parent-held identity/waitpid proving SIGKILL, exact postconditions, failure-only cleanup and no-adoption rules. Expiry or early death never counts as success. Scheduling delay is not a hard physical-time guarantee.
- Pure tests exercise the deadline/handshake decision logic through a separate mock-only seam; they never fork or reach native wrappers. Review must inspect the full post-fork call graph and enforce that the native ignored fixture is the sole caller. Source compilation is separate from native validation; neither this proposal nor ordinary tests launches a child.

## Binding source-review clarifications

1. Enumerate every reachable pthread_atfork registration in the linked A4 test
   binary (libtest/std/transitive crates included), proving none exists or every
   handler is async-signal-safe; the fixture installs none and adds none. Unknown
   handlers block source acceptance; no assumption of an empty handler set.
2. Parent creates the two fixed handshake pipes with O_CLOEXEC before fork. Child
   closes only fixture-created opposite ends passed as stack FD integers; it never
   enumerates descriptors or touches another inherited descriptor. All other FDs
   stay open-but-unused until _exit; no handshake/harness EOF expectation may depend
   on those FDs. Source review must include a phase-by-phase ownership table. No FD
   values or host identities are logged.
3. The child entry returns ! and always terminates with direct libc _exit. All
   reachable operations use raw libc calls and stack scalars only: no std wrappers,
   unwrap/expect, formatting, implicit allocation/panic/Drop, locks, Rust TLS,
   environment access or callbacks. Only EINTR may retry; every retry recomputes
   remaining monotonic time with saturating arithmetic, never negative/infinite.
   The sole narrow errno exception is an immediate scalar libc errno read needed
   to distinguish EINTR; no string conversion or Rust TLS accessor is allowed.
   This follows the errno rule in the cited signal-safety manual, not a general
   permission to use TLS. Review must verify its exact accessor implementation and
   any signal-handler errno preservation; unknown safety blocks acceptance.
4. Fork happens in the parent's supervisor cgroup, outside the pids-limited leaf.
   Parent migrates exactly one child task into the already-limited leaf, verifies
   task count plus live membership/population and parent/supervisor preconditions,
   then releases the handshake. Process existence alone is insufficient. All D9a
   numerical bounds and expiry/early-death refusal remain unchanged.

Required ownership table (source may use equivalent fixed names):

| Pipe | Parent owns after fork | Child owns after fork | Opposite ends closed |
| --- | --- | --- | --- |
| ready | read | write, closed after fixed byte | parent closes write; child closes read |
| release | write, closed after fixed byte | read, closed after verified byte | parent closes read; child closes write |

This clarifies inherited-FD passivity; it does not authorize close-all enumeration
or any child operation on unrelated inherited FDs. Parent-side error paths retain
child identity and failure-only kill/reap responsibility.

After independent architecture/security review, merge only the narrow ADR correction through normal gates. Source work follows in a small independently reviewed PR; native execution still requires its own bounded disposition. No A4, M2, full F/G or PA6 acceptance is granted. Installed/dedicated paths remain **unverified on real hardware**.
