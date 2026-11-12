# SYSTEM-refusal test port to current main: integration review

Reviewer: Anthropic / claude-opus-5-5 / high, as the cross-provider review sub-agent for a Sol (OpenAI / gpt-6.1-sol / medium) coordinator port. One agent, no subagents. Local only: no push, PR, status, watch, approval, merge, rerun or settings change.

This is an equivalence and integration recheck. The test's design, mutations and coverage analysis come from the retained [original review](service-system-guard-test-review.md). They keep their original pins and are not re-run here. The same provider wrote that original review, so this report claims no independence from it.

## Target

| item | value |
| --- | --- |
| base | `4c2264d6c3caee9507e11870ad361daea6c1dfdf` (PR42 merge on main) |
| head | `a1ce4cf2e84c0e17d2e7850125691c0d8310b108` (one commit, `test(service): protect non-SYSTEM identity refusal`) |
| merge-base | equals the base |
| diff | `native.rs` +13, `PROGRESS.md` +9, new evidence file +40, retained review +57. Additions only, 0 deletions, `git diff --check` clean. |
| original source | `6d946606bf8724ea4af4538973dcbf17366c3712` on `b5e65561b001a3616fa8a6623512e7ff8aa0e346` |
| original report | `51a84e1b9c220591382436b8dbf74172a592ad99` |
| spec | coordinator brief, retained review's spec line, AGENTS.md, KICKOFF routing and privacy rules, CONTRIBUTING |

## Verdict

- **Standards: PASS.** No findings.
- **Spec: PASS.** No blocking findings. One advisory (A1).

## Source identity (verified)

- The added `native.rs` lines are the same as the original commit's `native.rs` lines: `cmp` of the `+/-` lines from `b5e6556..6d94660` and `4c2264d..a1ce4cf` matched (13 lines). The only Rust change is that `#[cfg(windows)]` test in `service::native::tests`. No production statement changed.
- `native.rs` itself has no diff between `b5e6556` and `4c2264d`, so `require_service_identity` (lines 230–240) is unchanged. In `local.rs`, the only drift is two comment lines, and the `current_identity` re-export is unchanged. In `local_windows.rs`, the drift is limited to `validate_client_endpoint` and its tests (PR39). `current_identity` (`OpenProcessToken(TOKEN_QUERY)`, `GetTokenInformation(TokenUser)`, fixed `identity unavailable` errors) is unchanged. `layout::Error` still derives `Debug, PartialEq, Eq`. So the original analysis of the helper and the oracle still applies.
- The retained review is the same blob, `c5643ce9708e0e83cb5588e7c9e690d52c9ca78e`, at `51a84e1` and at `a1ce4cf`.
- Operation scope: the test makes only the current-process `TOKEN_QUERY` read and calls `require_service_identity()`. It does no startup, `Native`/Application construction, config/layout read, SCM, LSA, OwnerGuard, session, privilege, account, service or pipe work. Privacy: the SID becomes a `bool` before any assert. Only `Ok(())`/`Err(Permissions)` or the fixed `identity unavailable` text can be printed, so no SID is printed. A grep of the new evidence and PROGRESS text found no non-fixed SID.

## Standards

- S2-1 from the original review is resolved. The old PROGRESS hunk, including the N3 section and the rewritten current state, is completely absent. The new PROGRESS hunk adds only a fresh current-state block and renames the previous block to `## Historical checkpoint before SYSTEM-refusal test port`, following the file's existing pattern. All main history is kept: 0 deletions. `Current state` is still the first section, and it has exactly one `Queue priority` line.
- The commit covers one concern: the test plus its own records. The evidence file is ASCII with relative links. Baseline smells: none. The test name is accurate, and the comment states the privacy constraint.

## Spec

- The scope wording is honest. PROGRESS says the test "covers non-SYSTEM refusal; real SYSTEM acceptance and every startup guard call remain unverified." The evidence states that ordinary runners do not prove the exact SID constant, `current_identity` correctness (the shared oracle), every policy direction or guard calls at every startup site. This resolves the original S2-A1 wording advisory.
- Hardware labels and holds are present: installed/native/dedicated paths are marked **unverified on real hardware**, M2 is incomplete, E2 is held on `bdbf032b`, and no mutation run is claimed.
- **A1 (advisory, non-blocking).** The PROGRESS line "Main after #41 passed in 481s" has no run ID, while the historical block names run 43190243712 as in progress. I credited it as a coordinator fact and did not read it from GitHub. Adding the run ID would make the line traceable.

## Checks

- Credited from the coordinator, not repeated: `cargo fmt --all -- --check`, locked all-target Clippy with `-D warnings`, and exact nextest 1/1 with 242 not run (run `f706593d-1035-4090-9481-345537fe69f1`, 0.018s). PR43 macOS job 131095239676 (artifact script 5 run, 4 pass / 1 Linux-only skip) validates plist syntax only. It is not installed acceptance. Main run 43192357855 after #42 was pending, not green.
- Run by this reviewer, on Windows as an ordinary non-SYSTEM user at `a1ce4cf` with a clean tree: `cargo nextest run --locked -p arxd -E 'test(=service::native::tests::service_identity_guard_matches_process_identity_without_startup)'` passed 1/1 with 242 skipped (run `f4f8f108-43c6-492b-847a-17a7cfd4e01f`, 0.022s).
- Git-object checks: the pins, merge-base, `--numstat`, `--check`, test-line `cmp`, report blob identity, and helper/oracle drift diffs.
- Not run: the full suite (production is unchanged), mutations (credited to the retained report), and any native, installed, dedicated, SYSTEM, ignored-fixture or service/owner/admin/VM/Lab Manager/experiment operation. I made no GitHub reads or writes and did not read private-state.json.

## Limits

This review is not a merge approval. Publication waits for the queue disposition behind PR43, and merging requires all hosted gates. Real SYSTEM acceptance is untested. Installed, native and dedicated M2 paths are **unverified on real hardware**. M2 is incomplete, and E2 red run `bdbf032b` remains held.

**Totals:** Standards has 0 findings. Spec has 0 blocking findings and 1 advisory (A1: the #41 main-run line in PROGRESS has no run ID).
