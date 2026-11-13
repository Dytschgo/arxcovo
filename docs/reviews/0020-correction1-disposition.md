# ADR 0020 correction 1 disposition

Author: Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-12.
Correction attempt 1 of 3 after the
[independent Sol high review](0020-sol-independent-design-review.md)
(`8ff4196`, retained as `8ff3a54`) of the original proposal (`e3928f5`,
retained as `54032bf`). Correction base `d44035d`.

This is the author's disposition, **not a review and not acceptance**. ADR
0020 stays **proposed** pending a fresh independent Sol high recheck (fallback
grok-4.7 high). Opus does not self-accept. Docs only: the
[ADR](../adr/0020-linux-execution-hierarchy-and-janitor.md), the
[brief](../briefs/0020-linux-hierarchy-implementation.md) and the
[design evidence](0020-design-evidence.md) were corrected; the Sol review and
`PROGRESS.md` were not edited. Nothing was built, run or tested.

## Blocking findings

| Finding | Disposition | Where |
| --- | --- | --- |
| **S1** fixed sudo paths do not bind a trusted executable | **Corrected.** Trust boundary is the validated object reached through protected system ancestors, under an explicit privileged-writer assumption. Anchor `/` checked; every directory component opened `O_NOFOLLOW` and `fstat`ed (UID 0, no group/other write); the final object opened `O_PATH\|O_NOFOLLOW\|O_CLOEXEC` and `fstat`ed (regular, UID 0, no group/other write); execution by `execveat(fd, "", …, AT_EMPTY_PATH)`, never by path; empty environment; all candidates classified first, and any untrusted candidate means nothing runs. Normal `/bin` alias: only the exact root-owned `bin → usr/bin` or `/usr/bin` link drops C3 as an alias of C1; the link is classified, never followed; any other link is `untrusted`. Loader/library resolution is named as part of the privileged-writer assumption. Ordinary-user owned-tree tests use ELF fakes with markers: replaceable ancestors, intermediate and final links, the alias, check-to-exec replacement of the file and of a directory, and a close-on-exec script that must not run (S8, S9, S9b, S9c, S13). The false-pass consequence was rewritten. | ADR D8 (R5 probe), Consequences; brief E1 P10, E4 |
| **P1** re-listing after `dup` starts at the shared offset | **Corrected.** Every listing opens a fresh open file description with `openat(dir, ".", O_RDONLY\|O_DIRECTORY\|O_NOFOLLOW\|O_CLOEXEC)` and reads from offset 0; `dup`/`dup2`/`F_DUPFD`/`fdopendir` on the handle and cached streams are forbidden, which also keeps close-on-exec. Real owned-temp L9 lists one handle repeatedly, adds and removes entries between listings, and checks descriptor identity, `FD_CLOEXEC` and the handle offset. Fake H18 injects a leftover at final verification and requires `VerifyFailed` (quarantine). | ADR D4.3, D4.6, D5; brief E2 H18–H19, E3 listing, L6, L9 |
| **P2** unproved four-orphan bound and admission race | **Corrected; original claim withdrawn.** Explicit residue model covering every unconfirmed `kill_and_confirm` path and both create-path unlinks. Narrow proposed ADR 0019 delta A5: a per-runner, payload-free, monotone cleanup-residue flag; no change to `RunError`, `NotStarted`, `RunResult`, legacy `run`, `Report`, `Creation`, rows (cancelled row E unchanged) or outputs. A bridge in-slot gate checks residue and readiness inside the concurrency slot, and the post-call latch precedes slot release. With these, at most `max_concurrent_runs` residue-producing runner calls per process, with the proof sketch and its assumptions stated. Already admitted work continues and is counted; an empty residue leaf is not treated as a live-process escape. Barrier test T18 (limit 1, held after preflight) and bound test T19 (limit 4, repeated cancellation with failed cleanup, six further requests with zero port calls); T13 shows cancellation without residue never latches. | ADR Context 4–5, D6, D5, Bounds; brief E5, E6 |
| **P3** inconsistent populated postcondition and entry kinds | **Corrected.** One strict `events` parser everywhere; S9, leaf check and verify require populated `false` and allow extra keys such as `frozen`. Listing classification table: `.`/`..` consume the 512-record budget and are excluded from classification; directories are children; regular files with a control-file name are kernel control files; any other regular file, symlink, device, FIFO, socket or failed `DT_UNKNOWN` resolution is refused with nothing removed. Tests use realistic multi-key events (P3, H1, H20) and links/kinds in fake and real adapter tests (P9, H8, L10). Stated as fail-closed classification, not a claim that such entries occur on cgroupfs. | ADR D3 S9, D4.2–D4.4, Bounds; brief E1 P3/P9, E2 H8/H9/H20, E3 L10 |
| **P4** L8 expected precedence and fixture promise | **Corrected.** L8 expects `Hardening` (S0 before T1) with zero writes, with a skip marker if the test process unexpectedly parses as hardened; `Identity` is isolated by fake H10 with hardening `Standard`; H10b tests the precedence; no privileged test environment. P1 no longer promises every Stage D fixture unchanged: T2 is amended with explicit refusal fixtures for the intentional A1 unit-name tightening, plus a single-parser source pin. | ADR D3 S0, Relation A1; brief E1 P1, E2 H10/H10b, E3 L8 |

## Advisories and clarifications

| Item | Disposition |
| --- | --- |
| **S2** unify the panic-scan contract | Adopted the broader inventory explicitly: `exec/hierarchy.rs`, `exec/hierarchy/{parse,setup,linux}.rs`, `readiness.rs`, plus the changed `unix_shim.rs` and `cgroup.rs`; `fake.rs` excluded by name with an assertion; mutant self-tests for each denylist token; privacy rests on fixed payloads and H15 subprocess evidence, not `catch_unwind`. ADR D10; brief E6. |
| **P5** policy rights versus in-fence observations | Adopted. Values renamed to `exit_zero`/`exit_nonzero` (plus `absent`, `untrusted`, `unknown`); "ok" defined as an in-fence observation only; NNP's upstream guarantee and its non-exec limit cited; slice 9 explicitly does not detect user-name sudoers grants, polkit or other non-exec channels; no NNP removal or unfenced privileged probe. ADR D8; evidence. |
| Shutdown | Independent execution gate restated; checks immediately before S10, after `Ready` and before the row insert; behaviour at each barrier specified; tests H14 (S9/S10), W3–W3e; drain owns all spawned work. ADR D7; brief E7. |
| Cache ordering | Process-local monotone ticket taken before the start helper; update only on a greater ticket; `tools_observed_at` means "observing run started at or after". ADR D7; brief W5. |
| Deadlines | Janitor and sudo deadlines are logical polling deadlines; single syscalls, fork and reap can block past them; backstops are the runner's 60 s discovery timeout and leaf kill, the 10 s drain failure and systemd. Manual clock replaces tokio time pausing in tests. Aborting panics are not caught; ADR 0019 G-P2 stays. ADR D5, D7; brief E2, W3, W9. |

## Review decisions incorporated

| Item | Incorporated as |
| --- | --- |
| U1 | Discovery-time diagnostics with narrowed meaning; discovery is not the security gate; `Ready` before discovery only on that basis (D7, D8). No discovery-only admission state. |
| U2 | Mandatory exact `user.delegate`; cumulative with ownership, type and placement; no `trusted.delegate` fallback; inventory stays available (D2). |
| U3 | Conservative `Containment`/`Resource` latch kept; the bound now rests on A5 and the in-slot gate, not on the latch alone (D6). |
| U4 | Exact `CapBnd = 0xc1` and NNP=1; not a proof of effective/permitted sets or of cgroup writes; Stage F keeps those (D8). |
| U5 | Pending → `mechanism_unavailable`; no new token; first-cause CAS; restart-only reset (D5). |
| U6 | Kill the whole subtree first, including unknown-name processes; withhold only removal (D4). |
| U7 | No startup retry; agent refresh only while readiness permits; terminal latch needs restart (D7). |
| U8 | `unknown` → `rights_mismatch`; `untrusted` distinct from `absent`; spawn, signal and deadline cases tested (D8; brief S8, S9). |
| U9 | Unresolved; upstream text documents process termination only; Stage F verifies the package (D9; brief A9). |
| A1 | One parser with exact unit component; T2 fixture amendment; T8 is consistency evidence; privileged mounts and writers are a trust assumption; substituted handles tested before writes (D2; brief P1, H10). |
| A2 | Strict rights line; invalid table is row K, quarantine, zero tools; a valid table with a failing classification is deliberately row N with a terminal rights latch; tools released only after ADR 0017 assessment and full pin match (D8). |
| A3 | Atomic readiness with latches and the in-slot gate; janitor timeout/removal/verify/panic → quarantine; missing mechanism → `mechanism_unavailable`; a kill-write failure is not a "no live process" claim (D4.1, D5, D6). |
| A4 | Broader scanner inventory with S2 reconciliation (D10). |

## Residual decisions for the recheck

1. A5 as the only runner change (versus an honestly unbounded empty-leaf
   residual).
2. The exact `/bin` alias rule and `untrusted` for any other link layout.
3. The fail-closed control-file name grammar.
4. `VerifyFailed` as a quarantine.

## Retained gates

Production stays `MechanismUnavailable` on every OS; bridge construction and
wiring stay test-only. No Linux admin or macOS allowance, no report weakening,
no product approval flow, no packaging or CI change. PR53 (Stage C) and PR55
(Stage D) remain unmerged per the task context (GitHub state not refreshed),
and implementation waits for them and for an accepted contract. The parked CI
budget investigation neither blocks this design work nor waives any gate.
Native, installed and dedicated paths remain **unverified on real hardware**;
M2 is incomplete. `ExecutionFailed` / `E_PROTECTION` remain unknown: no fix,
waiver or retry.
