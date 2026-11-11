# Handle-reuse marker failure during executor integration

Codex / gpt-6.1-sol / medium observed this on the first coordinator full run
of service-executor candidate `8461a3a79609e53a4d5c6964d77891fa2ab54858`.

Command: `cargo nextest run --workspace --locked --status-level fail --final-status-level fail`.
Run ID: `a41ba4fd-714f-4da4-bd9b-2eb765934dfd`.

The unchanged test `development::windows::tests::handle_slot_reuse_is_not_inheritance`
failed at `windows.rs:774`:

```text
assertion failed: result.stdout.contains("ARX-HANDLE-REUSED-DISTINCT")
```

Earlier cleanup, identity, stop reason and exit-code assertions passed. The
parent's marker assertion did not include captured stdout/stderr, so the child's
exact reason is not established. Summary: 119/145 tests ran, 118 passed, one
failed, two skipped directly; fail-fast left 26 tests unrun. This is not a full
passing coordinator result, despite the builder's earlier 145-pass result.

`crates/arxd/src/development/windows.rs` is byte-identical to main `0ceb174`.
No change to its source or any handle guard was made. No fix, passing rerun or
cause is claimed. Diagnose using the existing real-process path and a focused
repeatable feedback loop. Keep object-identity, genuine leak, standard-handle,
Job, token and cleanup assertions intact. Do not hide the failure with retries,
skips, global serialization or a weakened marker assertion.

Separately, the new service executable validator panicked on `Cé:\tool.exe`
because its drive-prefix check sliced across a UTF-8 boundary. The regression
failed first, then passed after comparing the colon as a byte. All ten focused
service-executor tests, formatting and locked all-target Clippy passed after
that correction. It does not explain or fix the handle marker failure.

No privileged/service/owner/LSA/SCM/account/VM operation ran. Installed behavior
and dedicated-account M2 acceptance remain **unverified on real hardware**.
