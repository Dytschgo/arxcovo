# Windows service-pipe case guard: independent review

Reviewer: Anthropic / claude-opus-5-5 / high, as a cross-provider review sub-agent of a Sol (OpenAI / gpt-6.1-sol / medium) change. One agent did the review with no subagents. This was local only: no push, PR, status, watch, approval, merge or dispatch.

## Target

| item | value |
| --- | --- |
| branch | `fix/service-pipe-case-guard` (read through pinned objects and an owned-temp `git archive` export; the coordinator checkout was not touched) |
| base | `b5e65561b001a3616fa8a6623512e7ff8aa0e346` (#10 main) |
| head | `7be0f6b0afb14c68b53c53dad0167a04c3d9293f` (`93dcef8` fix + `7be0f6b` docs) |
| diff | `crates/arxd/src/local_windows.rs` (+32/−2), `docs/PROGRESS.md` |
| spec | PR27 carried-forward advisory; ADR 0009 endpoint `\\.\pipe\arxcovo-arxd`; Microsoft [Pipe Names](https://learn.microsoft.com/en-us/windows/win32/ipc/pipe-names): "Pipe names are not case-sensitive". I cited that page from the brief and did not re-fetch it in this review. |

## Verdict

- **Standards: pass.**
- **Spec: pass.** One advisory hardening note, not blocking.

## What was verified

- `validate_service_server` is pure. `connect` calls it first, before `encode_utf16` and `CreateFileW`. It pins SYSTEM (`S-1-5-18`) whenever the address matches `\\.\pipe\arxcovo-arxd` under `eq_ignore_ascii_case`. Every other name keeps its configured identity, and the owner-SID check after opening is unchanged.
- The remaining aliasing is closed by the existing `validate_address`. `ClientConfig::load` runs it on every production config. It requires the exact lowercase prefix `\\.\pipe\arxcovo-` and restricts the suffix to `[A-Za-z0-9-]{1,64}`. That rejects prefix-case variants (`\\.\PIPE\…`), trailing dots and spaces that Win32 `\\.\` normalization would strip, slashes and `..`, and `\\?\`/`\\localhost\` forms. So once a config is valid, suffix case is the only alias left, and the change covers exactly that.
- The test asserts that three valid casings refuse a synthetic non-SYSTEM pin and accept SYSTEM. It also asserts that `arxcovo-arxd-extra` and `arxcovo-development` keep their configured identity. That gives positive and negative controls plus a prefix-extension guard. The test calls only the pure helpers and opens no pipe.

## Checks run by this reviewer (Windows, ordinary user, owned temp export of `7be0f6b`)

- `service_pipe_casing_requires_system_without_pipe_io`: pass.
- Pure mutations to the helper, each restored by re-exporting the file from `7be0f6b` (`cmp` confirmed):
  - exact `==` comparison (old behavior): **fails** at line 263.
  - SYSTEM pin removed (`&& false`): **fails** at line 263.
  - over-broad `to_ascii_lowercase().starts_with(...)`: **fails** at line 271 (`-extra` negative control).
- **Scope deviation, disclosed.** One earlier mutation removed the `validate_service_server` call from `connect`. That run also included the existing parent test `fixed_service_endpoint_requires_privileged_server_identity_before_connect`, which builds the fixed `Layout::system()` endpoint. With the guard removed, the test reached `connect`'s client-side `CreateFileW` attempt on `\\.\pipe\arxcovo-arxd` as an ordinary user, and it failed with an error other than "service identity required". The brief forbids an actual connect that opens the fixed service pipe during a pipe-test mutation, so this broke that rule. No token or request is sent by `connect`, and the post-open owner check would refuse a non-matching server. I did not repeat the run or probe further, and every later mutation ran only the pure test.
- I did not repeat the builder's fmt, Clippy or full 191-test runs on this head. They were run at the N3 head for slice 1 only.

## Spec

There are no missing or incorrect requirements. The advisory below is not blocking.

- **P-A1 (defense in depth).** Windows `connect` does not call `validate_address` itself. Unix `connect` does (`local_unix.rs:135`). Today every production `ClientConfig` comes through `load`, and in-crate fixtures build only validated addresses, so nothing is reachable. If a future in-crate caller builds `ClientConfig` directly, prefix-case or normalization aliases of the fixed name would skip the SYSTEM pin. Suggestion: call `validate_address(&config.address)?` at the start of Windows `connect`, as Unix does. That belongs in a separate PR.

## Standards

No documented standard is broken. The helper name says what it does. Inside `#[cfg(test)]`, the test uses `use super::*;`. There are no baseline smells of note.

Note: the PROGRESS "Current state" hunk is the same rewrite that slice 3 carries and that the N3 line already contains. It merges cleanly because the changes are identical, so it is not a finding.

## Limits

Installed, native and dedicated M2 paths are **unverified on real hardware**, and M2 stays open. I made no production constructor, SCM, LSA, privilege, account, owner, admin, service-manager, VM or Lab Manager call. The only exception is the parent test's existing `Layout::system()` path, and it opened the fixed pipe only in the disclosed deviation above.

**Totals:** Standards has 0 findings. Spec has 0 blocking findings and 1 advisory (P-A1: Windows `connect` does not run `validate_address` itself).
