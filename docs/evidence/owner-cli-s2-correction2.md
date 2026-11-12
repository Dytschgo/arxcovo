# S2 native Verify adapter correction, attempt 2

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12.
Base `38becc7` retains the Opus recheck report from `a873c2a` on `d3cf449`.
Only the Windows test in client_config/tests.rs changes under crates. No
production statement, S1 file, lib.rs, dependency, workflow or routing changes.

SP-3R: the new test calls NativeFileOps::verify itself, using a synthetic
credential in an owned private temp directory. A normal read handle succeeds
at the exact length. Wrong expected length fails through the real checker;
an attributes-only handle makes the real GetSecurityInfo acquisition fail.
Both errors traverse the actual Partial classifier. Bytes and privacy remain
unchanged. No production create or ClientConfig::load is called, and no SID,
token, pointer or path is printed. This is ordinary-user filesystem work,
without owner/service/admin/account/native-executor operations.

Validation on the final test source in D:/Code/Arxcovo:

- Focused exact-name filter: 1 passed, 287 skipped, run
  `73b28012-1361-4c4f-8334-3202a8e57ab3`.
- Ignore native verify_with result: compiled and failed at the fixed
  checker-rejection assertion, run `7a5296dc-a54a-441f-b59c-8bfa0e6f5e9b`.
- Ignore native facts acquisition failure: compiled and failed at the fixed
  acquisition-failure assertion, run `7dce4b99-c5b2-4bcc-8909-b53a9d147098`.
- Each mutant restored the exact captured windows.rs bytes in finally;
  restored SHA-256 is
  `85aac020e5e5343c5b269fa47a6488a4b3d2f88bcb4c5d19a0427876f6f582d5`.
  Local ignored logs are target/s2-c2-ignore-verify.log and
  target/s2-c2-ignore-acquisition.log. No noncompile kill is claimed.
- `cargo fmt --all -- --check` passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings` passed.
- Exactly one final normal default-concurrency workspace nextest run:
  `3dfa9110-e373-4a8b-9a7b-863ae7c707f6`, **287 passed, 2 skipped**, 11.488s.
  No instrumentation, stress, retry or timeout/environment changes.

Documentation fixes: the second Current state heading is now historical;
native Mode coverage is accurately limited to conversion/source checks, with
no no-op Mode mutation kill claimed. Retained advisories remain: NTFS final
parent pins make the intermediate-retention mutant indistinguishable;
holding every intermediate pin is source-reviewed. Unix build/tests and
relevant mutations remain pending hosted. No local Unix execution is claimed.

The known ExecutionFailed did not recur and remains unexplained; this is no
fix, rate claim or waiver. No GitHub, owner, SCM/LSA, privilege, service, VM,
Lab Manager, private-state or ignored-fixture operation occurred. A fresh
independent Opus recheck and all hosted gates remain mandatory before S2
publication/merge. Native, installed and dedicated paths remain **unverified
on real hardware**; M2 is incomplete. Rollback is a reviewed source revert;
no installed state was created.
