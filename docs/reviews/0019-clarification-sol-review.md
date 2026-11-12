# Retained independent review of the Unix bridge clarification

Reviewer: OpenAI / gpt-6.1-sol / high, single agent, read-only. The coordinator
retains the delivered review findings here; the reviewer wrote no file or
commit. This is not a new coordinator review or execution claim.

Target b68a73eb1df52813e83eaff29fcc786109139ab9; comparison
68a760ec1a06f2509b7de9ab78e928250d69c077; runtime source1d828ca;
original proposal5ed795d and retained review6a6faae. Reviewed all four target
documents and their diff through Git objects, excluding shared S1 edits.

Standards: PASS, zero findings. Spec: REQUEST CHANGES, one medium blocker.

B1: T3 says 128 args of512 bytes plus4096-byte path/cwd pass validate_spec,
while L2 correctly requires legacy validation to refuse that prefix/tail.
T3 must name validate_phased_spec. More importantly, L2 tests only validators,
so changing Runner::run's call site to the phased validator survives those
assertions. Add a test of actual legacy Runner::run with a valid shim prefix
and an agent tail exceeding legacy item or aggregate-byte limits. Require
exact ExecError::Invalid("argv bounds") before backend work. The wrong entry
point must compile and fail that assertion; keep the direct boundary matrix.

No other blocking architectural inconsistency was found. C1 preserves exact
legacy error values and errno with a private shared failure; C3 preserves the
early refusal and adds the final check. C2's fixed payload boundary, whole
per-run catch, separate shim hook, scanner/self-test and stderr negative
control are meaningful within their stated limitations. D1–D7 and staged
activation remain intact; no root experiment or retry is authorized.

The reviewer verified pinned Rust1.95 facts from primary documentation:

- [catch_unwind](https://doc.rust-lang.org/1.95.0/std/panic/fn.catch_unwind.html):
  hooks precede catching, and aborting panics escape it.
- [set_hook](https://doc.rust-lang.org/1.95.0/std/panic/fn.set_hook.html) and
  [take_hook](https://doc.rust-lang.org/1.95.0/std/panic/fn.take_hook.html):
  global hooks, default stderr and mutation while panicking.
- [Result source](https://doc.rust-lang.org/1.95.0/src/core/result.rs.html)
  and [NulError source](https://doc.rust-lang.org/1.95.0/src/alloc/ffi/c_str.rs.html):
  error formatting can expose the underlying bytes.
- [str source](https://doc.rust-lang.org/1.95.0/src/core/str/mod.rs.html):
  slicing diagnostics can include a truncated string excerpt and a character.
- [Cargo profiles](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html):
  tests ignore the panic profile, so release-unwind verification is separate.

The reviewer ran Git/text/UTF-8/LF/link/whitespace checks and public primary
documentation reads only. No build, test, mutation, native, system, GitHub,
file/index/working-tree change or installation ran. Source facts do not prove
absence of dependency, implicit, aborting or real-service log leaks. G-P1–G-P3
remain activation obligations. Installed/native/dedicated paths remain
**unverified on real hardware**; M2 is incomplete.

Cross-provider credit covers review of Opus's design; no implementation or
activation is accepted. B1 must be corrected and independently rechecked.
