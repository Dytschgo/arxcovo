# Windows client endpoint validation (P-A1 follow-up): independent review

Reviewer: Anthropic / claude-opus-5-5 / high, as a cross-provider review sub-agent of a Sol (OpenAI / gpt-6.1-sol / medium) change. One agent did the review with no subagents. This was local only: no push, PR, status, watch, approval, merge or dispatch. This report sits in the prepared E2 checkout only as storage. I did not review, run or touch E2 source.

## Target

| item | value |
| --- | --- |
| base | `0ab20cbb421ab38fb614754211db8a5d428df34d` (retained Opus PASS of source `7be0f6b`) |
| head | `bd22e055ef630530c4cf89733a97d11148e3d8b7` (one commit, `bd22e05 fix(service): validate direct Windows client endpoints`) |
| diff | `crates/arxd/src/local_windows.rs` (+20/−5), `docs/PROGRESS.md` (+3) |
| read via | pinned Git objects and an owned-temp `git archive` export of the head |
| spec | advisory P-A1 in `docs/reviews/service-pipe-case-independent-review.md` ("call `validate_address(&config.address)?` at the start of Windows `connect`, as Unix does"); ADR 0009 fixed endpoint `\\.\pipe\arxcovo-arxd` |

## Verdict

- **Standards: pass.** No findings.
- **Spec: pass.** P-A1 is resolved. The SYSTEM case guard and the development identity behavior are unchanged.

## What was verified (source)

- The private pre-I/O helper was renamed from `validate_service_server` to `validate_client_endpoint`. Its first statement is now `validate_address(address)?`, followed by the unchanged `eq_ignore_ascii_case` check: fixed name means SYSTEM is required. `connect` still calls the helper as its first operation, before `encode_utf16` and `CreateFileW`. This matches Unix, where `connect` validates before `UnixStream::connect`.
- `validate_address` is unchanged. It requires the exact lowercase `\\.\pipe\arxcovo-` prefix and a `[A-Za-z0-9-]{1,64}` suffix. A directly built `ClientConfig` now gets the same namespace rule as `ClientConfig::load`. That closes prefix-case, `\\?\`, remote-server, separator and normalization aliases that could have skipped the SYSTEM requirement.
- The fixed `Layout::system()` endpoint `\\.\pipe\arxcovo-arxd` passes `validate_address`. The existing parent test therefore still reaches the "service identity required" category first. I confirmed this by reading the source only. As instructed, I did not run that test.
- Direct `ClientConfig`/`connect` callers in the crate are `fixture_connect`, `local_execution_tests::connect` and the service bridge test. Their Windows addresses are `arxcovo-{u64}`, `arxcovo-test` and `arxcovo-app-test-{pid}-{tempdir name with '.'→'-'}`. All of them satisfy `validate_address`, so the stricter helper should not break existing fixtures. I checked this with grep, not by running them.
- The new test passes SYSTEM as the expected identity for every address, so only address validation can produce the refusal. It also opens no pipe.

## Checks run by this reviewer (Windows, ordinary user, owned temp export of `bd22e05`)

- `--exact` `local::platform::tests::direct_client_endpoint_policy_refuses_nonlocal_or_invalid_addresses` and `…::service_pipe_casing_requires_system_without_pipe_io`: **2/2 pass**.
- Pure mutations to the helper and validator. In each run only those two pure tests were invoked, and each mutation was restored with `cmp` confirming it:
  - `validate_address(address)?` removed from the helper: new test **fails**.
  - case-insensitive prefix match in `validate_address`: new test **fails** (`\\.\PIPE\…`).
  - `\` allowed in the suffix: new test **fails** (`…arxd\extra`).
  - The restored head passes 2/2.
- `cargo fmt --all --check` passed. `cargo clippy --workspace --all-targets --locked -- -D warnings` passed with a fresh target dir.
- I did not run the parent transport tests or `connect` under any mutation. No pipe was opened. I did not run the full suite at this head, and the builder did not either. The 191-test result belongs to the prior head.

## Standards

No documented standard is broken. The new name describes what the helper does (endpoint plus identity policy before client I/O). The test is inside `#[cfg(test)]` and uses pure helpers only. No baseline smells of note.

Optional nit (not a finding): the new test asserts only `is_err()`, so it would not notice if the refusal category changed. Pinning `"invalid endpoint"` would make it stricter.

## Gaps and limits

- That `connect` calls the helper before I/O is checked by source inspection only. The test that would lock it at runtime is the parent fixed-endpoint test, which was not to be run under mutation.
- Installed, native and dedicated M2 paths are **unverified on real hardware**, and there is no M2 acceptance. I made no production constructor, SCM, LSA, privilege, account, owner, admin, service-manager, VM or Lab Manager call.

**Totals:** Standards has 0 findings. Spec has 0 findings, and P-A1 is closed.
